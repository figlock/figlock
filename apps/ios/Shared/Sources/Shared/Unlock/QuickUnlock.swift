import FigLockFFI
import Foundation

/// Face ID unlock of the vault: the Account Unlock Key wrapped to a Secure Enclave key (`crypto.md` 8, ADR-0025).
///
/// The blob and the policy state live in the device folder of the container, which is excluded from backups; the
/// key lives in the chip. The wrap and unwrap are the Rust functions of `figlock-core`; this type only moves
/// bytes between the chip, the files and those functions, and enforces the policy limits.
public struct QuickUnlock: Sendable {
    public enum Status: Equatable, Sendable {
        /// The person has not turned it on, or the device cannot do it.
        case off
        /// Face ID can be tried now.
        case ready
        /// It was on, a rule ended it, and the master password is needed once to turn it on again.
        case needsPassword(QuickUnlockPolicy.Reason)
    }

    public enum Failure: Error, Equatable, Sendable {
        case off
        case needsPassword(QuickUnlockPolicy.Reason)
        /// The person dismissed the prompt. Nothing is counted against them.
        case cancelled
        /// Face ID did not recognise the person. `triesLeft` is how many more prompts before the password is needed.
        case notRecognised(triesLeft: Int)
        case failed
    }

    private struct Saved: Codable {
        var wanted: Bool
        var policy: QuickUnlockPolicy?
        /// What the AutoFill extension itself read for the enrolled faces at its first Face ID unlock. iOS reports the
        /// state per process, so the extension cannot compare with the one the app saved in `policy` (FIG-247).
        var extensionBiometricState: Data?
    }

    private let chip: any QuickUnlockChip
    private let stateFile: URL
    private let blobFile: URL
    private let bootSession: @Sendable () -> String
    private let now: @Sendable () -> Date
    private let biometricStateIsPerProcess: Bool

    /// `biometricStateIsPerProcess` is true for the AutoFill extension. iOS gave it a Face ID state of the same size as
    /// the app's but a different value, so comparing across the two processes always looked like "the faces changed"
    /// and ended Face ID (FIG-247). The extension therefore keeps a baseline of its own, taken at its first Face ID
    /// unlock, and compares with that. The app, which turns Face ID on and sees its own state, keeps the strict check
    /// and ends Face ID itself when the faces change, which removes the key the extension would use.
    public init(
        container: VaultContainer,
        chip: any QuickUnlockChip,
        bootSession: @escaping @Sendable () -> String = BootSession.current,
        now: @escaping @Sendable () -> Date = { Date() },
        biometricStateIsPerProcess: Bool = false
    ) {
        self.biometricStateIsPerProcess = biometricStateIsPerProcess
        self.chip = chip
        self.stateFile = container.deviceFolder.appendingPathComponent("quick-unlock.json")
        self.blobFile = container.deviceFolder.appendingPathComponent("quick-unlock.blob")
        self.bootSession = bootSession
        self.now = now
    }

    public var isAvailable: Bool { chip.isAvailable }

    // MARK: State

    private func load() -> Saved {
        guard let data = try? Data(contentsOf: stateFile), let saved = try? JSONDecoder().decode(Saved.self, from: data)
        else { return Saved(wanted: false, policy: nil) }
        return saved
    }

    private func save(_ saved: Saved) throws {
        try FileManager.default.createDirectory(
            at: stateFile.deletingLastPathComponent(), withIntermediateDirectories: true)
        let data = try JSONEncoder().encode(saved)
        try data.write(to: stateFile, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    /// Whether the person wants Face ID, whatever state the key is in.
    public var isWanted: Bool { load().wanted }

    public func status() -> Status {
        let saved = load()
        guard saved.wanted, chip.isAvailable else { return .off }
        guard let policy = saved.policy, FileManager.default.fileExists(atPath: blobFile.path) else {
            return .needsPassword(saved.policy?.endedBy ?? .passwordChanged)
        }
        let decision = policy.decision(
            now: now(), bootSession: bootSession(), currentBiometricState: biometricState(for: saved, policy: policy))
        switch decision {
        case .allowed: return .ready
        case .needsPassword(let reason): return .needsPassword(reason)
        }
    }

    /// The value handed to the policy as "the enrolled faces now". In the extension it is the policy's own saved value
    /// while the extension's baseline holds (or there is none yet, or the state cannot be read), and a different
    /// value once the extension sees a change against its own baseline.
    private func biometricState(for saved: Saved, policy: QuickUnlockPolicy) -> Data? {
        let state = chip.biometricState()
        guard biometricStateIsPerProcess else { return state }
        guard let baseline = saved.extensionBiometricState, let state, state != baseline else {
            return policy.biometricState
        }
        return (policy.biometricState ?? Data()) + Data([0])
    }

    /// What the Face ID check saw, for the AutoFill screen when it says Face ID changed: sizes only, never the data.
    public func biometricNote() -> String {
        let loaded = load()
        let now = chip.biometricState()
        let describe: (Data?) -> String = { $0.map { "\($0.count) bytes" } ?? "none" }
        let same: (Data?) -> String = { $0 == now ? "same" : "different" }
        return "app saved \(describe(loaded.policy?.biometricState)) (\(same(loaded.policy?.biometricState))), "
            + "extension baseline \(describe(loaded.extensionBiometricState)) (\(same(loaded.extensionBiometricState))), "
            + "now \(describe(now))"
    }

    // MARK: Turn on, off

    /// Turn Face ID on, or on again after a full unlock. `key` is the unlock key the master password just gave.
    /// Creates a fresh hardware key and wraps `key` to it. No prompt. Throws when the device cannot do it, and
    /// leaves nothing half done: either the key, the blob and the state are all there or Face ID stays off.
    public func enable(key: UnlockKey, vaultId: String, deviceId: String) throws {
        guard chip.isAvailable else { throw ChipError.unavailable }
        do {
            let publicPoint = try chip.createKey()
            let blob = try key.sealForQuickUnlock(qkPublic: publicPoint, vaultId: vaultId, deviceId: deviceId)
            try blob.write(to: blobFile, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            let stamp = now()
            let policy = QuickUnlockPolicy(
                bootSession: bootSession(), lastFullUnlock: stamp, biometricState: chip.biometricState())
            try save(Saved(wanted: true, policy: policy))
        } catch {
            removeKeyMaterial()
            throw error
        }
    }

    /// A full master password unlock worked while Face ID is wanted but was ended by a rule: turn it on again.
    /// Returns whether it did.
    @discardableResult
    public func renewAfterFullUnlock(key: UnlockKey, vaultId: String, deviceId: String) -> Bool {
        let saved = load()
        guard saved.wanted, chip.isAvailable else { return false }
        if case .ready = status() {
            // Still valid: only the full unlock clock restarts.
            var policy = saved.policy
            policy?.lastFullUnlock = now()
            policy?.lastUse = now()
            policy?.failedAttempts = 0
            try? save(Saved(wanted: true, policy: policy))
            return true
        }
        return (try? enable(key: key, vaultId: vaultId, deviceId: deviceId)) != nil
    }

    /// The person turned Face ID off, or the vault was removed from this phone: key, blob and state all go.
    public func disable() {
        removeKeyMaterial()
        try? FileManager.default.removeItem(at: stateFile)
    }

    /// A rule ended it (`crypto.md` 8): the key and blob go, the person's wish stays, so the next full unlock
    /// can turn it on again.
    public func end(_ reason: QuickUnlockPolicy.Reason) {
        removeKeyMaterial()
        var saved = load()
        guard saved.wanted else { return }
        var policy =
            saved.policy ?? QuickUnlockPolicy(bootSession: bootSession(), lastFullUnlock: now())
        policy.endedBy = reason
        saved.policy = policy
        try? save(saved)
    }

    private func removeKeyMaterial() {
        try? chip.deleteKey()
        try? FileManager.default.removeItem(at: blobFile)
    }

    /// `saved` with the new policy. After a Face ID unlock that worked, the extension also takes its baseline of the
    /// enrolled faces if it has none yet (see `biometricStateIsPerProcess`).
    private func updated(_ saved: Saved, with policy: QuickUnlockPolicy, takingBaseline: Bool) -> Saved {
        var result = saved
        result.policy = policy
        if takingBaseline, biometricStateIsPerProcess, result.extensionBiometricState == nil {
            result.extensionBiometricState = chip.biometricState()
        }
        return result
    }

    // MARK: Unlock

    /// Ask for Face ID and return the unlock key. The check order is: the limits of `crypto.md` 8 first (no prompt
    /// when one is hit), then the chip.
    public func unlock(vaultId: String, deviceId: String, reason: String) async throws -> UnlockKey {
        let saved = load()
        guard saved.wanted, chip.isAvailable else { throw Failure.off }
        guard var policy = saved.policy, let blob = try? Data(contentsOf: blobFile) else {
            throw Failure.needsPassword(saved.policy?.endedBy ?? .passwordChanged)
        }
        let decision = policy.decision(
            now: now(), bootSession: bootSession(), currentBiometricState: biometricState(for: saved, policy: policy))
        switch decision {
        case .allowed: break
        case .needsPassword(let why):
            end(why)
            throw Failure.needsPassword(why)
        }

        do {
            let qkPublic = try chip.publicPoint()
            let ephemeral = try quickUnlockEphemeralPublic(blob: blob)
            let shared = try await chip.agree(ephemeralPublic: ephemeral, reason: reason)
            let key = try openQuickUnlock(
                blob: blob, qkPublic: qkPublic, shared: shared, vaultId: vaultId, deviceId: deviceId)
            policy.recordQuickUnlock(at: now())
            try? save(updated(saved, with: policy, takingBaseline: true))
            return key
        } catch ChipError.cancelled {
            throw Failure.cancelled
        } catch ChipError.authFailed {
            policy.recordFailure()
            try? save(updated(saved, with: policy, takingBaseline: false))
            if policy.failedAttempts >= QuickUnlockPolicy.failureLimit {
                end(.tooManyFailures)
                throw Failure.needsPassword(.tooManyFailures)
            }
            throw Failure.notRecognised(triesLeft: QuickUnlockPolicy.failureLimit - policy.failedAttempts)
        } catch ChipError.keyMissing {
            // The system removed the key (passcode or biometrics reset), or the blob belongs to another chip.
            end(.biometricsChanged)
            throw Failure.needsPassword(.biometricsChanged)
        } catch is FigLockError {
            // A blob that does not open (damaged, another vault, another device) is as good as gone.
            end(.biometricsChanged)
            throw Failure.needsPassword(.biometricsChanged)
        } catch {
            // Lockout, a busy keychain and the like are not the end of quick unlock: keep the key and try later.
            throw Failure.failed
        }
    }
}

public enum BootSession {
    /// The system's id of this boot, which changes at every restart of the phone.
    @Sendable public static func current() -> String {
        var size = 0
        guard sysctlbyname("kern.bootsessionuuid", nil, &size, nil, 0) == 0, size > 0 else { return "unknown" }
        var buffer = [CChar](repeating: 0, count: size)
        guard sysctlbyname("kern.bootsessionuuid", &buffer, &size, nil, 0) == 0 else { return "unknown" }
        return String(cString: buffer)
    }
}
