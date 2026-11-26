import FigLockFFI
import Foundation

/// The vault as the AutoFill extension sees it (ADR-0041 decision 7): opened read-only, only through Face ID, never
/// with a password. This type has no path that runs a key derivation or writes a file of the vault.
public struct AutofillAccess: Sendable {
    public enum Failure: Error, Equatable, Sendable {
        /// This phone has no usable vault: the app has not been set up.
        case noVault
        /// Face ID is off or ended; the app must unlock with the master password first.
        case needsApp(QuickUnlockPolicy.Reason?)
        case cancelled
        case notRecognised(triesLeft: Int)
        case failed
    }

    private let vault: LocalVault
    private let keeper: SecretKeyKeeper
    private let quick: QuickUnlock
    private let deviceName: String
    private let now: @Sendable () -> Date

    public init(
        vault: LocalVault, keeper: SecretKeyKeeper, quick: QuickUnlock, deviceName: String = "iPhone",
        now: @escaping @Sendable () -> Date = { Date() }
    ) {
        precondition(vault.access == .readOnly, "the extension opens the vault read-only")
        self.vault = vault
        self.keeper = keeper
        self.quick = quick
        self.deviceName = deviceName
        self.now = now
    }

    /// Whether Face ID can be asked for now, without prompting.
    public func canUnlock() -> Failure? {
        guard case .ready = vault.status() else { return .noVault }
        switch quick.status() {
        case .ready: return nil
        case .off: return .needsApp(nil)
        case .needsPassword(let reason): return .needsApp(reason)
        }
    }

    private struct Stored {
        let vaultId: String
        let store: any BlobStoreFfi
        let vaultJson: Data
        let records: [Data]
        let keys: DeviceKeysHandle
    }

    private func readStored() throws -> Stored {
        guard case .ready(let vaultId, _) = vault.status() else { throw Failure.noVault }
        do {
            let store = try vault.openStore()
            let vaultJson = try store.get(path: "vault.json")
            let records = try store.list(prefix: "devices/").compactMap { try? store.get(path: $0.path) }
            guard let keys = try keeper.deviceKeys(vaultId: vaultId) else { throw Failure.noVault }
            return Stored(vaultId: vaultId, store: store, vaultJson: vaultJson, records: records, keys: keys)
        } catch let failure as Failure {
            throw failure
        } catch {
            throw Failure.failed
        }
    }

    private func faceIDKey(_ stored: Stored, reason: String) async throws -> UnlockKey {
        do {
            return try await quick.unlock(vaultId: stored.vaultId, deviceId: stored.keys.deviceId(), reason: reason)
        } catch let failure as QuickUnlock.Failure {
            switch failure {
            case .off: throw Failure.needsApp(nil)
            case .needsPassword(let why): throw Failure.needsApp(why)
            case .cancelled: throw Failure.cancelled
            case .notRecognised(let left): throw Failure.notRecognised(triesLeft: left)
            case .failed: throw Failure.failed
            }
        }
    }

    /// Ask for Face ID, open the vault and read the op files. The session is the caller's to `close()`.
    public func open(reason: String) async throws -> AutofillVault {
        if let blocked = canUnlock() { throw blocked }
        let stored = try readStored()
        let key = try await faceIDKey(stored, reason: reason)
        let session: VaultSession
        do {
            // A new device record, if the outcome offers one, is ignored: the extension never writes.
            session =
                try unlockVaultWithKey(
                    vaultJson: stored.vaultJson, key: key, device: stored.keys, deviceName: deviceName,
                    deviceRecords: stored.records, nowMs: UInt64(now().timeIntervalSince1970 * 1000)
                ).session
        } catch {
            quick.end(.passwordChanged)
            throw Failure.needsApp(.passwordChanged)
        }
        let now = self.now
        let store = stored.store
        let (listing, failedFiles): (ItemListing?, Int) = await Task.detached(priority: .userInitiated) {
            var applied: [String: UInt64] = [:]
            let report = try? VaultLog(store: store).apply(to: session, applied: &applied, now: now)
            return (try? session.items(includeDeleted: false), report?.failed.count ?? 0)
        }.value
        guard let listing else {
            session.lock()
            throw Failure.failed
        }
        return AutofillVault(session: session, items: listing.items.map(VaultItem.init), failedFiles: failedFiles)
    }
}

/// An open, read-only vault in the extension. It holds one secret at a time: `credential` reads the password,
/// hands it back and keeps nothing.
public final class AutofillVault: @unchecked Sendable {
    public struct Credential: Sendable {
        public let username: String
        public let password: String
    }

    private let session: VaultSession
    public let logins: [VaultItem]
    /// Op files that could not be applied: the list may be missing recent changes. The app shows the detail.
    public let failedFiles: Int

    init(session: VaultSession, items: [VaultItem], failedFiles: Int = 0) {
        self.failedFiles = failedFiles
        self.session = session
        self.logins = items.filter { $0.kind == .login && !$0.isDeleted }
    }

    public func matches(identifiers: [String]) -> [VaultItem] {
        ServiceMatch.matches(logins, identifiers: identifiers)
    }

    /// What to offer for a request: the matching logins, or every login when none match (see `ServiceMatch`).
    public func suggestions(identifiers: [String]) -> ServiceMatch.Suggestions {
        ServiceMatch.suggestions(logins, identifiers: identifiers)
    }

    /// Logins whose title, username or website contains every word of `query` (case blind), newest first.
    public func search(_ query: String) -> [VaultItem] {
        let words = query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard !words.isEmpty else { return logins.sorted { $0.updated > $1.updated } }
        return logins.filter { item in
            let haystack = ([item.title, item.fields["username"] ?? ""] + item.websites).joined(separator: " ")
                .lowercased()
            return words.allSatisfy { haystack.contains($0) }
        }.sorted { $0.updated > $1.updated }
    }

    public func credential(itemId: String) throws -> Credential {
        guard let item = logins.first(where: { $0.id == itemId }) else { throw AutofillAccess.Failure.failed }
        let password = try session.revealField(itemId: itemId, field: "password")
        return Credential(username: item.fields["username"] ?? "", password: password)
    }

    /// Drop the keys. Call when done; the extension process is also short lived.
    public func close() { session.lock() }
}
