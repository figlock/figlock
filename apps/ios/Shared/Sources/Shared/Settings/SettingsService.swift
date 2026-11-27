import FigLockFFI
import Foundation

/// What Settings does to the vault and to this phone, apart from the screens: revoking a device, the Emergency Kit,
/// deleting the Drive copy. Each call is one deliberate action (ADR-0018, ADR-0022, ADR-0035).
public struct SettingsService: Sendable {
    public enum Failure: Error, Equatable, Sendable {
        /// The password or the Secret Key is wrong. One answer for both.
        case wrongCredentials
        /// This phone keeps no Secret Key and none was typed (or it is not well formed).
        case secretKeyNeeded
        /// The device cannot be revoked from here: it is this iPhone, already revoked, or unknown.
        case notAllowed
        /// The vault was locked or changed under the call.
        case failed
        case signIn
        case offline
        /// The remote holds a different vault than this phone.
        case foreignVault
        /// The Drive copy has a `vault.json` that is not valid, so it cannot be matched to this vault.
        case damaged
        /// The deletion ran but files were still in Drive after it, or the copy could not be matched. Safe to
        /// run again.
        case incomplete

        /// Writing the new `vault.json` failed after the vault accepted the change, so it is unknown what is stored.
        /// The caller locks the app, and the next unlock reads what is really there.
        case changeUncertain
    }

    /// What a revocation did.
    public struct Revoked: Equatable, Sendable {
        /// Whether the new vault state reached the remote. False when there is no remote or it was unreachable;
        /// the next sync sends it.
        public let pushed: Bool
    }

    public static let vaultName = "My vault"

    private let vault: LocalVault
    private let keeper: SecretKeyKeeper
    private let now: @Sendable () -> Date

    public init(vault: LocalVault, keeper: SecretKeyKeeper, now: @escaping @Sendable () -> Date = { Date() }) {
        self.vault = vault
        self.keeper = keeper
        self.now = now
    }

    private var nowMs: UInt64 { UInt64(now().timeIntervalSince1970 * 1000) }

    private func vaultId() -> String? {
        if case .ready(let id, _) = vault.status() { return id }
        return nil
    }

    // MARK: Revoke

    /// Revoke `deviceId` (ADR-0018). Needs the password and the Secret Key (the kept one, or `typedSecretKey`),
    /// which the vault checks. The new files are written to this phone's folder in the order a reader needs
    /// (snapshot, records, `vault.json` last), then sent to `remote` when there is one.
    public func revoke(
        deviceId: String, password: String, typedSecretKey: String?, session: VaultSession,
        remote: (any BlobStoreFfi)?
    ) async throws -> Revoked {
        guard
            let target = try? session.devices().first(where: { $0.id == deviceId }),
            target.revokedMs == nil, !target.isThisDevice
        else { throw Failure.notAllowed }
        guard let id = vaultId() else { throw Failure.failed }
        let secretKey: String
        if let typedSecretKey, !typedSecretKey.isEmpty {
            secretKey = typedSecretKey
        } else if let kept = try? keeper.secretKey(vaultId: id) {
            secretKey = kept
        } else {
            throw Failure.secretKeyNeeded
        }
        guard FigLockCore.isValidSecretKey(secretKey) else { throw Failure.secretKeyNeeded }

        let store = try vault.openStore()
        let nowMs = self.nowMs
        let state = FileSyncState(container: vault.container)
        return try await Task.detached(priority: .userInitiated) {
            // Edits not yet stored go first: a revocation needs a clean log.
            if (try? session.hasPending()) == true, let file = try session.commit() {
                try store.putNew(path: file.path, bytes: file.bytes)
            }
            let files: RevocationFiles
            do {
                files = try session.revokeDevice(
                    deviceId: deviceId, password: password, secretKey: secretKey, nowMs: nowMs)
            } catch FigLockError.Failed {
                throw Failure.wrongCredentials
            } catch FigLockError.InvalidInput {
                throw Failure.secretKeyNeeded
            } catch {
                throw Failure.failed
            }
            try Self.write(files, to: store)
            guard let remote else { return Revoked(pushed: false) }
            let engine = SyncEngine(
                local: store, remote: remote, state: state, clock: SyncRunClock(), ownDevice: try? session.deviceId())
            return Revoked(pushed: (try? engine.run(confirmReupload: false)) != nil)
        }.value
    }

    /// The order of `Session::write_revocation` in the Rust session: the snapshot first (an existing one is fine),
    /// the re-signed records, the removed record, and `vault.json` last, so a device that sees the new epoch finds
    /// the rest already there.
    static func write(_ files: RevocationFiles, to store: any BlobStoreFfi) throws {
        do {
            try store.putNew(path: files.snapshot.path, bytes: files.snapshot.bytes)
        } catch StoreFailure.Exists {}
        for record in files.records { try store.putReplace(path: record.path, bytes: record.bytes) }
        for path in files.remove { try store.delete(path: path) }
        try store.putReplace(path: "vault.json", bytes: files.vaultJson)
    }

    // MARK: Recovery Key (ADR-0029)

    /// Whether the vault has a Recovery Key. Never what it is: FigLock does not keep it.
    public func hasRecoveryKey(session: VaultSession) -> Bool { (try? session.hasRecoveryKey()) ?? false }

    /// The Secret Key to check a change against: the typed one, else the one this phone keeps.
    private func secretKeyText(typed: String?) throws -> String {
        let text: String
        if let typed, !typed.isEmpty {
            text = typed
        } else if let id = vaultId(), let kept = try? keeper.secretKey(vaultId: id) {
            text = kept
        } else {
            throw Failure.secretKeyNeeded
        }
        guard FigLockCore.isValidSecretKey(text) else { throw Failure.secretKeyNeeded }
        return text
    }

    /// The printable Recovery Key page for `draft` as a PDF, dated today.
    public func recoveryKitPage(_ draft: RecoveryDraft, storage: String) throws -> Data {
        let date = String(ISO8601DateFormatter().string(from: now()).prefix(10))
        return try draft.renderKit(vaultName: Self.vaultName, storage: storage, created: date)
    }

    /// Turn the Recovery Key on, or replace it with `draft`, after the password and the Secret Key (the typed
    /// one, or the kept one) were checked by the vault. A wrong pair changes nothing. The new `vault.json` is
    /// written to this phone's folder; the sync that follows sends it on.
    public func setRecoveryKey(
        _ draft: RecoveryDraft, password: String, typedSecretKey: String?, session: VaultSession
    ) async throws {
        let secretKey = try secretKeyText(typed: typedSecretKey)
        let store = try vault.openStore()
        try await Task.detached(priority: .userInitiated) {
            let vaultJson: Data
            do {
                vaultJson = try session.setRecoveryKey(draft: draft, password: password, secretKey: secretKey)
            } catch FigLockError.InvalidInput {
                throw Failure.secretKeyNeeded
            } catch FigLockError.Failed {
                throw Failure.wrongCredentials
            } catch {
                throw Failure.failed
            }
            try Self.writeVaultJson(vaultJson, to: store)
        }.value
    }

    /// Turn the Recovery Key off. Removing it needs no secret of the vault, so the password and the Secret
    /// Key are proven against the stored `vault.json` first. Codes made earlier open nothing from then on.
    public func removeRecoveryKey(password: String, typedSecretKey: String?, session: VaultSession) async throws {
        let secretKey = try secretKeyText(typed: typedSecretKey)
        let store = try vault.openStore()
        try await Task.detached(priority: .userInitiated) {
            let stored: Data
            let vaultJson: Data
            do {
                stored = try store.get(path: "vault.json")
                vaultJson = try session.clearRecoveryKey(vaultJson: stored, password: password, secretKey: secretKey)
            } catch FigLockError.InvalidInput {
                throw Failure.secretKeyNeeded
            } catch FigLockError.Failed {
                throw Failure.wrongCredentials
            } catch {
                throw Failure.failed
            }
            try Self.writeVaultJson(vaultJson, to: store)
        }.value
    }

    private static func writeVaultJson(_ bytes: Data, to store: any BlobStoreFfi) throws {
        do {
            try store.putReplace(path: "vault.json", bytes: bytes)
        } catch {
            throw Failure.changeUncertain
        }
    }

    // MARK: Emergency Kit

    /// The Secret Key this phone keeps, or nil when it keeps none (the person stopped it, ADR-0038).
    public func keptSecretKey() -> String? {
        guard let id = vaultId() else { return nil }
        return try? keeper.secretKey(vaultId: id)
    }

    /// The Emergency Kit as a one page PDF, dated today, for the kept Secret Key.
    public func emergencyKit(storage: String) throws -> Data {
        guard let key = keptSecretKey() else { throw Failure.secretKeyNeeded }
        let date = String(ISO8601DateFormatter().string(from: now()).prefix(10))
        return try renderEmergencyKit(
            secretKey: key, vaultName: Self.vaultName, storage: storage, created: date)
    }

    // MARK: Delete the Drive copy (ADR-0035)

    /// The phrase to type before the Drive copy of this vault is deleted.
    public static var deleteFromDrivePhrase: String { deleteRemotePhrase() }

    public static func isDeleteFromDrivePhrase(_ typed: String) -> Bool { deleteRemoteConfirmed(typed: typed) }

    /// Delete this vault's copy from Google Drive. Never the copy on this phone, the keys or the Google sign in.
    /// `typed` is checked again by the sync engine, so a call without the phrase deletes nothing. Returns how many
    /// files went.
    public func deleteDriveCopy(typed: String, remote: any BlobStoreFfi) async throws -> Int {
        guard Self.isDeleteFromDrivePhrase(typed) else { throw Failure.failed }
        let store = try vault.openStore()
        let state = FileSyncState(container: vault.container)
        do {
            let files = try await Task.detached(priority: .userInitiated) {
                let engine = SyncEngine(
                    local: store, remote: remote, state: state, clock: SyncRunClock(), ownDevice: nil)
                return try engine.deleteRemote(typed: typed)
            }.value
            return Int(files)
        } catch let failure as SyncFailure {
            switch failure {
            case .Other, .Busy: throw Failure.incomplete
            default: throw Onboarding.map(failure).asSettingsFailure
            }
        } catch let failure as StoreFailure {
            throw Onboarding.map(failure).asSettingsFailure
        } catch {
            throw Failure.failed
        }
    }
}

extension Onboarding.Failure {
    fileprivate var asSettingsFailure: SettingsService.Failure {
        switch self {
        case .signIn: .signIn
        case .offline: .offline
        case .foreignVault: .foreignVault
        case .damaged: .damaged
        default: .failed
        }
    }
}
