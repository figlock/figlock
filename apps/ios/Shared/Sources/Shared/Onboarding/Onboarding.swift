import FigLockFFI
import Foundation

/// Getting a vault onto this iPhone: create one, join one by pairing, open one from Google Drive, and the
/// recovery from a vault folder that went missing (ADR-0026, ADR-0041 decision 8, `pairing-recovery.md` 3).
///
/// Every path ends the same way: the vault files are in the local container, this phone's device keys and the
/// Secret Key are in the keychain, and `LocalVault.status()` says `ready`. The caller then unlocks through
/// `AppLock`, which turns Face ID on again when the person asked for it. Nothing is stored until the step that
/// makes it complete has worked, so an abandoned attempt leaves no trace (apart from a pairing request file in the
/// remote's `inbox/`).
public struct Onboarding: Sendable {
    /// What the screens word. No text from the input, a path or a key is carried.
    public enum Failure: Error, Equatable, Sendable {
        /// The storage has no FigLock vault.
        case noVaultInStorage
        /// The pairing code is malformed, expired, or belongs to another vault. One case for all of them, so a
        /// stranger's file cannot be told apart from a typo.
        case invalidCode
        /// Someone already answered that invite (the request file exists).
        case alreadyAnswered
        /// The trusted device refused, or the answer is not valid, or the master password is wrong.
        case joinFailed
        case offline
        /// iCloud lists the vault but has not downloaded it yet. Not missing: try again shortly.
        case waitingForDownload
        case signIn
        /// The remote holds a different vault than this phone remembers.
        case foreignVault
        case damaged
        /// This phone already has a vault; onboarding does not replace it.
        case alreadyHasVault
        case failed
    }

    /// A vault found in a remote storage, for the "found one vault" screen.
    public struct RemoteVault: Equatable, Sendable {
        public let vaultId: String
        public let revision: UInt64
    }

    let vault: LocalVault
    let keeper: SecretKeyKeeper
    let quick: QuickUnlock
    let now: @Sendable () -> Date
    /// The system's QuickType suggestions, emptied when the vault leaves this phone (ADR-0041 decision 7).
    let identities: CredentialIdentitySync?

    public init(
        vault: LocalVault, keeper: SecretKeyKeeper, quick: QuickUnlock,
        identities: CredentialIdentitySync? = nil, now: @escaping @Sendable () -> Date = { Date() }
    ) {
        self.vault = vault
        self.keeper = keeper
        self.quick = quick
        self.identities = identities
        self.now = now
    }

    var nowMs: UInt64 { UInt64(now().timeIntervalSince1970 * 1000) }

    /// The local vault folder, which no remote folder may be, contain or sit inside (ADR-0043).
    public var vaultFolder: URL { vault.container.vaultFolder }

    /// Whether this iPhone can offer Face ID at all (the simulator cannot).
    public var quickAvailable: Bool { quick.isAvailable }

    // MARK: Remembering the vault id

    /// The id of the vault this phone has, kept next to the presence marker so a missing vault folder (no
    /// `vault.json` to read it from) can still be cleaned up and restored.
    private var vaultIdFile: URL { vault.container.deviceFolder.appendingPathComponent("vault-id") }

    public var rememberedVaultId: String? {
        guard let data = try? Data(contentsOf: vaultIdFile) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    func remember(vaultId: String) throws {
        try Data(vaultId.utf8).write(
            to: vaultIdFile, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    // MARK: Create

    /// A vault made in memory and not stored yet.
    public final class PendingVault: @unchecked Sendable {
        fileprivate let created: CreatedVault
        fileprivate let password: String
        fileprivate let deviceName: String

        fileprivate init(created: CreatedVault, password: String, deviceName: String) {
            self.created = created
            self.password = password
            self.deviceName = deviceName
        }

        /// The Secret Key, `FL1-XXXX-...`, for the "write this down" screen.
        public func secretKeyText() throws -> String { try created.secretKeyText() }

        /// The Emergency Kit as a one page PDF; `date` is `YYYY-MM-DD`.
        public func emergencyKit(vaultName: String, date: String) throws -> Data {
            try created.renderKit(vaultName: vaultName, storage: "On this iPhone", created: date)
        }

        /// The forced check that the kit was saved: the last two groups of the Secret Key (8 characters).
        public func verify(typed: String) -> Bool {
            (try? created.verifyKitGroups(typed: typed)) ?? false
        }
    }

    /// Make a vault from a new master password. Argon2id at the real cost takes a moment, so this runs off the
    /// main thread. Nothing is stored.
    public func startCreation(password: String, deviceName: String) async throws -> PendingVault {
        guard case .noVault = vault.status() else { throw Failure.alreadyHasVault }
        let nowMs = nowMs
        do {
            let created = try await Task.detached(priority: .userInitiated) {
                try createVault(password: password, deviceName: deviceName, kdf: defaultKdfSettings(), nowMs: nowMs)
            }.value
            return PendingVault(created: created, password: password, deviceName: deviceName)
        } catch {
            throw Failure.failed
        }
    }

    /// Store a verified vault. Returns whether Face ID is on afterwards (it is only turned on when asked and when
    /// this iPhone can do it; a failure there never fails the creation, the vault exists either way).
    @discardableResult
    public func commitCreation(_ pending: PendingVault, enableFaceID: Bool) async throws -> Bool {
        guard case .noVault = vault.status() else { throw Failure.alreadyHasVault }
        let created = pending.created
        let secretKey: String
        let known = try? vaultIdentity(vaultJson: created.vaultJson()).vaultId
        do {
            let vaultJson = try created.vaultJson()
            let vaultId = try vaultIdentity(vaultJson: vaultJson).vaultId
            let keys = created.deviceKeys()
            secretKey = try created.secretKeyText()
            // Keys first: a vault file without keys is a phone that cannot unlock it, the other way round is
            // just keys nobody uses. Any failure from here on undoes all of it.
            try keeper.saveDeviceKeys(keys, vaultId: vaultId)
            try keeper.keep(secretKey, vaultId: vaultId)
            let store = try vault.openStore()
            try store.putNew(path: "vault.json", bytes: vaultJson)
            try store.putNew(path: "devices/\(keys.deviceId()).json", bytes: try created.deviceRecord())
            try vault.markVaultPresent()
            try remember(vaultId: vaultId)
        } catch {
            rollBack(vaultId: known, keepKeys: false, wasMissing: false)
            throw Failure.failed
        }
        guard enableFaceID else { return false }
        return await turnOnFaceID(password: pending.password, secretKey: secretKey)
    }

    /// Undo a half finished onboarding: the local vault folder, the keys this attempt saved, the presence marker and
    /// the remembered id. A restore after a missing folder keeps the phone's own keys and its marker, so the phone
    /// goes back to `missing` and not to a phone that never had a vault.
    func rollBack(vaultId: String?, keepKeys: Bool, wasMissing: Bool) {
        try? FileManager.default.removeItem(at: vault.container.vaultFolder)
        if let vaultId, !keepKeys { try? keeper.removeAll(vaultId: vaultId) }
        clearSyncRecords()
        guard !wasMissing else { return }
        try? vault.forgetVault()
        try? FileManager.default.removeItem(at: vaultIdFile)
    }

    // MARK: Face ID

    /// Turn Face ID on after a full unlock. Derives the unlock key again, because the creation and join calls
    /// do not hand it out. Returns whether it is on.
    @discardableResult
    public func turnOnFaceID(password: String, secretKey: String? = nil) async -> Bool {
        guard quick.isAvailable, case .ready(let vaultId, _) = vault.status(),
            let keys = try? keeper.deviceKeys(vaultId: vaultId),
            let secretKey = secretKey ?? ((try? keeper.secretKey(vaultId: vaultId)) ?? nil),
            let vaultJson = try? vault.openStore().get(path: "vault.json")
        else { return false }
        let deviceId = keys.deviceId()
        let quick = quick
        return await Task.detached(priority: .userInitiated) {
            guard let key = try? deriveUnlockKey(vaultJson: vaultJson, password: password, secretKey: secretKey)
            else { return false }
            return (try? quick.enable(key: key, vaultId: vaultId, deviceId: deviceId)) != nil
        }.value
    }

    // MARK: Remote storage

    /// The vault in `remote`, or nil when it holds none.
    public func inspect(remote: any BlobStoreFfi) throws -> RemoteVault? {
        do {
            let identity = try vaultIdentity(vaultJson: try remote.get(path: "vault.json"))
            return RemoteVault(vaultId: identity.vaultId, revision: identity.revision)
        } catch StoreFailure.NotFound {
            return nil
        } catch let failure as StoreFailure {
            throw Self.map(failure)
        } catch SyncFailure.Damaged {
            throw Failure.damaged
        } catch {
            throw Failure.failed
        }
    }

    /// Copy everything in `remote` into the local vault folder with the one Rust sync engine, which also refuses a
    /// remote that holds a different vault than the local one.
    func pull(from remote: any BlobStoreFfi, entry: RemoteEntry = .drive) async throws {
        let local = try vault.openStore()
        let state = FileSyncState(container: vault.container, remote: entry)
        do {
            try await Task.detached(priority: .userInitiated) {
                let engine = SyncEngine(
                    local: local, remote: remote, state: state, clock: SyncRunClock(), ownDevice: nil)
                _ = try engine.run(confirmReupload: false)
            }.value
        } catch let failure as SyncFailure {
            throw Self.map(failure)
        } catch let failure as StoreFailure {
            throw Self.map(failure)
        } catch {
            throw Failure.failed
        }
    }

    static func map(_ failure: StoreFailure) -> Failure {
        switch failure {
        case .Unauthorized: .signIn
        case .Unavailable: .offline
        case .NotYetAvailable: .waitingForDownload
        default: .failed
        }
    }

    static func map(_ failure: SyncFailure) -> Failure {
        switch failure {
        case .SignIn: .signIn
        case .Offline: .offline
        case .NotDownloaded: .waitingForDownload
        case .ForeignVault: .foreignVault
        case .Damaged: .damaged
        default: .failed
        }
    }

    // MARK: Open from Drive, restore

    /// Download the vault in `remote` into this phone. A new phone gets fresh device keys; a phone that already
    /// has keys for this vault (a restore after the folder went missing) keeps them, so its record in the vault
    /// still matches. The person then unlocks with the master password and the Secret Key; the unlock adds this
    /// device's record when the vault has none.
    public func openFromRemote(_ remote: any BlobStoreFfi, entry: RemoteEntry = .drive) async throws {
        guard let found = try inspect(remote: remote) else { throw Failure.noVaultInStorage }
        let wasMissing: Bool
        switch vault.status() {
        case .noVault: wasMissing = false
        case .missing:
            // The keys on this phone belong to the vault it had; a different vault, or one that cannot be
            // compared because the remembered id is lost, is not a restore.
            guard rememberedVaultId == found.vaultId else { throw Failure.foreignVault }
            wasMissing = true
            try vault.container.createVaultFolder()
        default: throw Failure.alreadyHasVault
        }
        let hadKeys = ((try? keeper.deviceKeys(vaultId: found.vaultId)) ?? nil) != nil
        do {
            try await pull(from: remote, entry: entry)
            guard case .ready(let vaultId, _) = vault.status(), vaultId == found.vaultId else { throw Failure.failed }
            if !hadKeys { try keeper.saveDeviceKeys(try DeviceKeysHandle.generate(), vaultId: vaultId) }
            try vault.markVaultPresent()
            try remember(vaultId: vaultId)
            RemoteList(container: vault.container).add(entry)
        } catch {
            rollBack(vaultId: found.vaultId, keepKeys: hadKeys, wasMissing: wasMissing)
            throw (error as? Failure) ?? Failure.failed
        }
    }

    // MARK: Recovery

    /// Forget every remote of this phone and what each remembered about its syncing. The remotes' own copies of
    /// the vault are never touched.
    func clearSyncRecords() {
        let device = vault.container.deviceFolder
        RemoteList(container: vault.container).removeAll()
        let names = (try? FileManager.default.contentsOfDirectory(atPath: device.path)) ?? []
        for name in names where name.hasPrefix("sync-state") || name == "remotes.json" {
            try? FileManager.default.removeItem(at: device.appendingPathComponent(name))
        }
    }

    /// "Remove this vault from this iPhone": forgets the keys, the Secret Key, Face ID, the list of remotes and the
    /// local copy of the vault. The vault in Google Drive or an iCloud Drive folder and the Emergency Kit are never
    /// touched. Afterwards `status()` is `noVault`.
    public func removeFromThisPhone() throws {
        let id = rememberedVaultId ?? { if case .ready(let id, _) = vault.status() { id } else { nil } }()
        quick.disable()
        if let identities { Task { @MainActor in identities.clear() } }
        if let id { try keeper.removeAll(vaultId: id) }
        let container = vault.container
        try? FileManager.default.removeItem(at: container.vaultFolder)
        clearSyncRecords()
        for name in ["password-throttle.json", "vault-id"] {
            try? FileManager.default.removeItem(at: container.deviceFolder.appendingPathComponent(name))
        }
        try vault.forgetVault()
        try container.prepare()
    }
}
