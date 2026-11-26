import FigLockFFI
import Foundation
import Observation

/// The lock of the app: whether the vault is open, and every way of opening it (ADR-0041 decision 6).
///
/// Opening is the master password (with the Secret Key when this phone does not keep it) or Face ID through
/// `QuickUnlock`. Locking drops the session, so the keys are gone from memory, when the person asks, when the app
/// has been in the background longer than the auto-lock time, or when the app starts.
@MainActor
@Observable
public final class AppLock {
    public enum State {
        case locked
        case unlocked(VaultSession)
    }

    /// What the unlock screen shows after a try.
    public enum Outcome: Equatable, Sendable {
        case unlocked
        /// Wrong master password or Secret Key. One message for both, so the screen cannot tell them apart.
        case wrongCredentials(triesLeft: Int?)
        /// Password entry is paused until this time. Face ID still works when it is on.
        case paused(until: Date)
        /// The Secret Key is needed and was not typed, or is not well formed.
        case secretKeyNeeded
        case faceIDCancelled
        case faceIDNotRecognised(triesLeft: Int)
        /// A rule ended Face ID; the password is needed once.
        case passwordNeeded(QuickUnlockPolicy.Reason)
        /// This phone has no device keys for the vault: onboarding has not finished.
        case notSetUp
        case failed
    }

    /// The default auto-lock time (the prototype's Settings default); FIG-168 makes it a setting.
    public static let defaultAutoLock: TimeInterval = 5 * 60

    public private(set) var state: State = .locked
    /// Bumped when the vault on this phone appears or goes (onboarding, removal), so screens that read it redraw.
    public private(set) var vaultVersion = 0
    /// Why the screen is locked or what it should say, from the last try.
    public private(set) var lastOutcome: Outcome?
    public var autoLockAfter: TimeInterval

    private let vault: LocalVault
    private let keeper: SecretKeyKeeper
    private let quick: QuickUnlock
    private let throttleFile: URL
    private let deviceName: String
    private let now: @Sendable () -> Date
    private var backgroundedAt: Date?

    public init(
        vault: LocalVault,
        keeper: SecretKeyKeeper,
        quick: QuickUnlock,
        deviceName: String,
        autoLockAfter: TimeInterval = AppLock.defaultAutoLock,
        now: @escaping @Sendable () -> Date = { Date() }
    ) {
        self.vault = vault
        self.keeper = keeper
        self.quick = quick
        self.deviceName = deviceName
        self.autoLockAfter = autoLockAfter
        self.now = now
        self.throttleFile = vault.container.deviceFolder.appendingPathComponent("password-throttle.json")
    }

    public var session: VaultSession? {
        if case .unlocked(let session) = state { return session }
        return nil
    }

    public var isUnlocked: Bool { session != nil }

    /// What Face ID can do right now, for the unlock screen.
    public var quickUnlockStatus: QuickUnlock.Status { quick.status() }

    /// Whether this iPhone can do Face ID quick unlock at all: a face or fingerprint is enrolled (ADR-0046).
    public var quickUnlockIsAvailable: Bool { quick.isAvailable }

    /// Whether the person has Face ID on, whatever state its key is in (a rule may have ended it for now).
    public var quickUnlockIsWanted: Bool { quick.isWanted }

    /// The person turned Face ID off: the key, the blob and the state all go.
    public func turnOffFaceID() { quick.disable() }

    /// Whether the unlock screen has to show the Secret Key field.
    public func needsSecretKeyTyped() -> Bool {
        guard case .ready(let vaultId, _) = vault.status() else { return false }
        return (try? keeper.needsTyping(vaultId: vaultId)) ?? true
    }

    public func vaultDidChange() { vaultVersion += 1 }

    // MARK: Locking

    public func lock() {
        session?.lock()
        state = .locked
    }

    public func appDidEnterBackground() {
        guard isUnlocked else { return }
        backgroundedAt = now()
        if autoLockAfter <= 0 { lock() }
    }

    public func appWillEnterForeground() {
        defer { backgroundedAt = nil }
        guard isUnlocked, let backgroundedAt else { return }
        if now().timeIntervalSince(backgroundedAt) >= autoLockAfter { lock() }
    }

    // MARK: Password throttle

    private func loadThrottle() -> PasswordThrottle {
        guard let data = try? Data(contentsOf: throttleFile),
            let throttle = try? JSONDecoder().decode(PasswordThrottle.self, from: data)
        else { return PasswordThrottle() }
        return throttle
    }

    private func saveThrottle(_ throttle: PasswordThrottle) {
        try? FileManager.default.createDirectory(
            at: throttleFile.deletingLastPathComponent(), withIntermediateDirectories: true)
        if let data = try? JSONEncoder().encode(throttle) {
            try? data.write(to: throttleFile, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        }
    }

    /// When password entry is paused, until when.
    public var passwordPausedUntil: Date? {
        let throttle = loadThrottle()
        return throttle.isPaused(now: now()) ? throttle.pausedUntil : nil
    }

    // MARK: Opening

    private struct Stored {
        let vaultId: String
        let vaultJson: Data
        let deviceRecords: [Data]
        let deviceKeys: DeviceKeysHandle
    }

    private func readStored() throws -> Stored? {
        guard case .ready(let vaultId, _) = vault.status() else { return nil }
        let store = try vault.openStore()
        let vaultJson = try store.get(path: "vault.json")
        let records = try store.list(prefix: "devices/").compactMap { try? store.get(path: $0.path) }
        guard let keys = try keeper.deviceKeys(vaultId: vaultId) else { return nil }
        return Stored(vaultId: vaultId, vaultJson: vaultJson, deviceRecords: records, deviceKeys: keys)
    }

    /// Unlock with the master password. `secretKey` is the typed text, or nil to use the kept one.
    @discardableResult
    public func unlock(password: String, secretKey typed: String?) async -> Outcome {
        let attemptTime = now()
        var throttle = loadThrottle()
        if throttle.isPaused(now: attemptTime), let until = throttle.pausedUntil {
            return record(.paused(until: until))
        }
        do {
            guard let stored = try readStored() else { return record(.notSetUp) }
            let secretKey: String
            if let typed, !typed.isEmpty {
                secretKey = typed
            } else if let kept = try keeper.secretKey(vaultId: stored.vaultId) {
                secretKey = kept
            } else {
                return record(.secretKeyNeeded)
            }
            if !FigLockCore.isValidSecretKey(secretKey) { return record(.secretKeyNeeded) }

            let now = self.now
            let name = deviceName
            let keyAndOutcome: (UnlockKey, UnlockOutcome)
            do {
                keyAndOutcome = try await Task.detached(priority: .userInitiated) {
                    // Argon2id is memory hard and takes a moment: not on the main thread.
                    let key = try deriveUnlockKey(
                        vaultJson: stored.vaultJson, password: password, secretKey: secretKey)
                    let outcome = try unlockVaultWithKey(
                        vaultJson: stored.vaultJson, key: key, device: stored.deviceKeys, deviceName: name,
                        deviceRecords: stored.deviceRecords, nowMs: UInt64(now().timeIntervalSince1970 * 1000))
                    return (key, outcome)
                }.value
            } catch is FigLockError {
                throttle.recordFailure(at: attemptTime)
                saveThrottle(throttle)
                if let until = throttle.pausedUntil, throttle.isPaused(now: attemptTime) {
                    return record(.paused(until: until))
                }
                return record(.wrongCredentials(triesLeft: throttle.triesLeft(now: attemptTime)))
            }
            let (key, outcome) = keyAndOutcome

            if let record = outcome.newDeviceRecord {
                let deviceId = stored.deviceKeys.deviceId()
                try? vault.openStore().putNew(path: "devices/\(deviceId).json", bytes: record)
            }
            throttle.recordSuccess()
            saveThrottle(throttle)
            // Keep the Secret Key only while the person has not opted out (ADR-0038); a typed key is kept now.
            if typed != nil { _ = try? keeper.keep(secretKey, vaultId: stored.vaultId) }
            quick.renewAfterFullUnlock(key: key, vaultId: stored.vaultId, deviceId: stored.deviceKeys.deviceId())
            state = .unlocked(outcome.session)
            return record(.unlocked)
        } catch {
            return record(.failed)
        }
    }

    /// Unlock with Face ID. The system asks the person; nothing is typed.
    @discardableResult
    public func unlockWithFaceID() async -> Outcome {
        do {
            guard let stored = try readStored() else { return record(.notSetUp) }
            let deviceId = stored.deviceKeys.deviceId()
            let key = try await quick.unlock(
                vaultId: stored.vaultId, deviceId: deviceId, reason: "Unlock FigLock")
            let outcome: UnlockOutcome
            do {
                outcome = try unlockVaultWithKey(
                    vaultJson: stored.vaultJson, key: key, device: stored.deviceKeys, deviceName: deviceName,
                    deviceRecords: stored.deviceRecords, nowMs: UInt64(now().timeIntervalSince1970 * 1000))
            } catch is FigLockError {
                // The vault no longer opens with this key: a password change or a rotation on another device.
                quick.end(.passwordChanged)
                return record(.passwordNeeded(.passwordChanged))
            }
            state = .unlocked(outcome.session)
            return record(.unlocked)
        } catch let failure as QuickUnlock.Failure {
            switch failure {
            case .off, .failed: return record(.failed)
            case .cancelled: return record(.faceIDCancelled)
            case .notRecognised(let left): return record(.faceIDNotRecognised(triesLeft: left))
            case .needsPassword(let reason): return record(.passwordNeeded(reason))
            }
        } catch {
            return record(.failed)
        }
    }

    private func record(_ outcome: Outcome) -> Outcome {
        lastOutcome = outcome
        return outcome
    }
}
