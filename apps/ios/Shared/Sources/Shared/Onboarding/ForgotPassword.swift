import FigLockFFI
import Foundation

/// "I forgot my password or Secret Key" for the vault on this iPhone (`pairing-recovery.md` 7.3, ADR-0029): the
/// Recovery Key opens the vault, the person chooses a new master password and gets a new Secret Key and a new
/// Emergency Kit. The same Rust `Vault::recover` the desktop uses. Nothing is written until the new kit was
/// confirmed, so backing out at any step leaves the vault and the phone as they were.
extension Onboarding {
    /// A Recovery Key that opened the vault, and then the new credentials made from it. Not stored anywhere.
    public final class Recovering: @unchecked Sendable {
        fileprivate let attempt: RecoveryAttempt
        fileprivate let vaultId: String
        fileprivate var prepared: RecoveredVault?

        fileprivate init(attempt: RecoveryAttempt, vaultId: String) {
            self.attempt = attempt
            self.vaultId = vaultId
        }

        /// The new Secret Key, once the new password was chosen.
        public func newSecretKeyText() -> String? { prepared?.secretKeyText() }

        /// The new Emergency Kit as a one page PDF; `date` is `YYYY-MM-DD`.
        public func emergencyKit(date: String) throws -> Data {
            guard let prepared else { throw Failure.failed }
            return try prepared.renderKit(vaultName: "My vault", storage: "On this iPhone", created: date)
        }

        /// The forced check that the new kit was saved: the last two groups of the new Secret Key.
        public func verify(typed: String) -> Bool { prepared?.verifyKitGroups(typed: typed) ?? false }
    }

    /// Open the vault on this phone with a Recovery Key code. Reads only; the key is proven with a trial run.
    public func startRecovery(code: String, deviceName: String) async throws -> Recovering {
        guard case .ready(let vaultId, _) = vault.status() else { throw Failure.failed }
        guard recoveryKeyIsValid(code: code) else { throw Failure.badRecoveryKey }
        let store = try vault.openStore()
        guard let keys = try keeper.deviceKeys(vaultId: vaultId) else { throw Failure.failed }
        let nowMs = nowMs
        do {
            let vaultJson = try store.get(path: "vault.json")
            guard try vaultHasRecoveryKey(vaultJson: vaultJson) else { throw Failure.noRecoveryKey }
            let records = try store.list(prefix: "devices/").compactMap { try? store.get(path: $0.path) }
            let attempt = try await Task.detached(priority: .userInitiated) {
                try FigLockFFI.startRecovery(
                    code: code, vaultJson: vaultJson, records: records, device: keys, deviceName: deviceName,
                    nowMs: nowMs)
            }.value
            return Recovering(attempt: attempt, vaultId: vaultId)
        } catch let failure as Failure {
            throw failure
        } catch FigLockError.InvalidInput {
            throw Failure.badRecoveryKey
        } catch FigLockError.Failed {
            // The same answer for a key of another vault, a tampered file and a key that does not fit.
            throw Failure.recoveryFailed
        } catch {
            throw Failure.failed
        }
    }

    /// Choose the new master password and make the new Secret Key, in memory.
    public func prepareRecovery(_ recovering: Recovering, newPassword: String) async throws {
        let nowMs = nowMs
        do {
            recovering.prepared = try await Task.detached(priority: .userInitiated) {
                try recovering.attempt.prepare(newPassword: newPassword, nowMs: nowMs)
            }.value
        } catch {
            throw Failure.recoveryFailed
        }
    }

    /// Store the new `vault.json`, forget everything that belonged to the old credentials (the kept Secret Key, Face
    /// ID's blob, a password pause) and return the new Secret Key for the unlock that follows. Refused until
    /// `prepareRecovery` ran. A failed write leaves the vault as it was.
    public func commitRecovery(_ recovering: Recovering) throws -> String {
        guard let prepared = recovering.prepared, case .ready(let vaultId, _) = vault.status(),
            vaultId == recovering.vaultId
        else { throw Failure.failed }
        do {
            let store = try vault.openStore()
            if let record = prepared.newRecord() {
                do {
                    try store.putNew(path: "devices/\(prepared.deviceKeys().deviceId()).json", bytes: record)
                } catch StoreFailure.Exists {}
            }
            try store.putReplace(path: "vault.json", bytes: prepared.vaultJson())
        } catch {
            throw Failure.failed
        }
        try? keeper.discardStale(vaultId: vaultId)
        quick.end(.passwordChanged)
        try? FileManager.default.removeItem(
            at: vault.container.deviceFolder.appendingPathComponent("password-throttle.json"))
        recovering.prepared = nil
        return prepared.secretKeyText()
    }
}
