import FigLockFFI
import Foundation

/// Joining a vault by pairing (`pairing-recovery.md` 3).
extension Onboarding {
    /// A join waiting for the trusted device (`pairing-recovery.md` 3.2). Holds the invite keys and the new device
    /// keys in memory only; the phone saves nothing until `finish` has worked.
    public final class Pairing: @unchecked Sendable {
        public let fingerprint: String
        private let handle: JoinHandle
        private let keys: DeviceKeysHandle
        private let remote: any BlobStoreFfi
        private let onboarding: Onboarding
        private let entry: RemoteEntry

        init(
            handle: JoinHandle, keys: DeviceKeysHandle, remote: any BlobStoreFfi, onboarding: Onboarding,
            entry: RemoteEntry
        ) {
            self.entry = entry
            self.handle = handle
            self.keys = keys
            self.remote = remote
            self.onboarding = onboarding
            fingerprint = handle.fingerprint()
        }

        /// Whether the trusted device has answered. The answer and this device's record are two files and a
        /// remote can show one first, so an answer only counts once both are there.
        public func answered() throws -> Bool {
            for path in [handle.responsePath(), handle.recordPath()] {
                do {
                    _ = try remote.get(path: path)
                } catch StoreFailure.NotFound {
                    return false
                } catch let failure as StoreFailure {
                    throw Onboarding.map(failure)
                }
            }
            return true
        }

        /// Open the answer with the password and store the vault on this phone. The Secret Key that came in
        /// the answer is kept through `SecretKeyKeeper.keep`, which honors the ADR-0038 opt-out.
        public func finish(password: String) async throws {
            let handle = handle
            let remote = remote
            let nowMs = onboarding.nowMs
            let outcome: JoinOutcome
            do {
                outcome = try await Task.detached(priority: .userInitiated) {
                    let response = try remote.get(path: handle.responsePath())
                    let vaultJson = try remote.get(path: "vault.json")
                    let records = try remote.list(prefix: "devices/").compactMap { try? remote.get(path: $0.path) }
                    return try handle.finish(
                        response: response, vaultJson: vaultJson, records: records, password: password, nowMs: nowMs)
                }.value
            } catch is FigLockError {
                throw Failure.joinFailed
            } catch let failure as StoreFailure {
                throw Onboarding.map(failure)
            } catch {
                throw Failure.failed
            }
            outcome.session.lock()
            try await onboarding.adopt(remote: remote, keys: keys, secretKey: outcome.secretKeyText, entry: entry)
        }
    }

    /// Ask to join the vault in `remote` with the invite `code` shown on the trusted device. Stores the request in
    /// `inbox/` of the remote; the trusted device compares `fingerprint` and approves.
    public func beginPairing(
        code: String, remote: any BlobStoreFfi, deviceName: String, entry: RemoteEntry = .drive
    ) async throws -> Pairing {
        guard case .noVault = vault.status() else { throw Failure.alreadyHasVault }
        guard inviteCodeIsValid(code: code) else { throw Failure.invalidCode }
        let vaultJson: Data
        do {
            vaultJson = try remote.get(path: "vault.json")
        } catch StoreFailure.NotFound {
            throw Failure.noVaultInStorage
        } catch let failure as StoreFailure {
            throw Self.map(failure)
        }
        let invite: Data
        do {
            invite = try remote.get(path: try invitePath(code: code, vaultJson: vaultJson))
        } catch StoreFailure.NotFound {
            throw Failure.invalidCode
        } catch let failure as StoreFailure {
            throw Self.map(failure)
        } catch {
            throw Failure.invalidCode
        }
        let keys = try DeviceKeysHandle.generate()
        let start: JoinStart
        do {
            start = try startJoin(
                code: code, inviteBytes: invite, device: keys, deviceName: deviceName, nowMs: nowMs)
        } catch {
            throw Failure.invalidCode
        }
        do {
            try remote.putNew(path: start.request.path, bytes: start.request.bytes)
        } catch StoreFailure.Exists {
            throw Failure.alreadyAnswered
        } catch let failure as StoreFailure {
            throw Self.map(failure)
        }
        return Pairing(handle: start.handle, keys: keys, remote: remote, onboarding: self, entry: entry)
    }

    /// Store what a finished join produced: the vault files, this phone's keys and the Secret Key. All or nothing.
    func adopt(remote: any BlobStoreFfi, keys: DeviceKeysHandle, secretKey: String, entry: RemoteEntry) async throws {
        let vaultId = try? vaultIdentity(vaultJson: try remote.get(path: "vault.json")).vaultId
        do {
            try await pull(from: remote, entry: entry)
            guard case .ready(let readyId, _) = vault.status(), readyId == vaultId else { throw Failure.failed }
            try keeper.saveDeviceKeys(keys, vaultId: readyId)
            try keeper.keep(secretKey, vaultId: readyId)
            try vault.markVaultPresent()
            try remember(vaultId: readyId)
            RemoteList(container: vault.container).add(entry)
        } catch {
            rollBack(vaultId: vaultId, keepKeys: false, wasMissing: false)
            throw (error as? Failure) ?? Failure.failed
        }
    }
}
