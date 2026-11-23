import Foundation

/// When Face ID may stand in for the master password (`crypto.md` 8). The limits are defaults a person can only
/// tighten. The state is local to this phone and never synced.
public struct QuickUnlockPolicy: Codable, Equatable, Sendable {
    /// A full master password unlock is needed after this long without any unlock.
    public static let idleLimit: TimeInterval = 4 * 3600
    /// ... and after this long since the last full master password unlock.
    public static let fullUnlockLimit: TimeInterval = 7 * 86400
    /// ... and after this many failed Face ID attempts in a row.
    public static let failureLimit = 5

    public enum Reason: String, Codable, Equatable, Sendable {
        /// The phone restarted since the last full unlock. Replaces "after an app restart", which does not exist
        /// on iOS where the system ends and restarts processes at will (ADR-0041 decision 6).
        case phoneRestarted
        case idleTooLong
        case fullUnlockTooOld
        case tooManyFailures
        /// Face ID or Touch ID enrolments changed, or the Secure Enclave key is gone.
        case biometricsChanged
        case lockedByUser
        case passwordChanged
    }

    public enum Decision: Equatable, Sendable {
        case allowed
        case needsPassword(Reason)
    }

    /// Identifies one run of the phone, from the boot session id. Changes at every restart.
    public var bootSession: String
    public var lastFullUnlock: Date
    public var lastUse: Date
    public var failedAttempts: Int
    /// What the system reported for the enrolled biometrics when quick unlock was turned on, to notice a change.
    public var biometricState: Data?
    /// Set when a rule above (or the person) ended quick unlock. It stays until a full unlock turns it on again.
    public var endedBy: Reason?

    public init(
        bootSession: String,
        lastFullUnlock: Date,
        lastUse: Date? = nil,
        failedAttempts: Int = 0,
        biometricState: Data? = nil,
        endedBy: Reason? = nil
    ) {
        self.bootSession = bootSession
        self.lastFullUnlock = lastFullUnlock
        self.lastUse = lastUse ?? lastFullUnlock
        self.failedAttempts = failedAttempts
        self.biometricState = biometricState
        self.endedBy = endedBy
    }

    /// Whether quick unlock may be tried now. `currentBiometricState` is what the system reports at this moment.
    public func decision(now: Date, bootSession current: String, currentBiometricState: Data?) -> Decision {
        if let endedBy { return .needsPassword(endedBy) }
        if current != bootSession { return .needsPassword(.phoneRestarted) }
        if failedAttempts >= Self.failureLimit { return .needsPassword(.tooManyFailures) }
        if now.timeIntervalSince(lastFullUnlock) >= Self.fullUnlockLimit {
            return .needsPassword(.fullUnlockTooOld)
        }
        if now.timeIntervalSince(lastUse) >= Self.idleLimit { return .needsPassword(.idleTooLong) }
        if let biometricState, biometricState != currentBiometricState {
            return .needsPassword(.biometricsChanged)
        }
        return .allowed
    }

    /// A Face ID unlock worked.
    public mutating func recordQuickUnlock(at now: Date) {
        lastUse = now
        failedAttempts = 0
    }

    /// Face ID did not release the key. Cancelling is not a failure: only a recognised mismatch counts.
    public mutating func recordFailure() {
        failedAttempts += 1
    }
}

/// Slows down master password guesses on this phone. The Argon2id cost already limits an offline attacker; this
/// stops someone holding an unlocked phone from guessing at the screen. The state survives a restart of the app,
/// so killing it does not reset the count.
public struct PasswordThrottle: Codable, Equatable, Sendable {
    /// Wrong passwords in a row that are free before entry pauses (the prototype says "3 tries").
    public static let freeAttempts = 3
    public static let firstPause: TimeInterval = 60
    public static let longestPause: TimeInterval = 3600

    public private(set) var failures: Int
    public private(set) var pausedUntil: Date?

    public init(failures: Int = 0, pausedUntil: Date? = nil) {
        self.failures = failures
        self.pausedUntil = pausedUntil
    }

    /// Tries left before entry pauses, or nil while it is already paused. After a pause ends, one more try is
    /// allowed and a wrong one pauses again for twice as long.
    public func triesLeft(now: Date) -> Int? {
        isPaused(now: now) ? nil : max(Self.freeAttempts - failures, 1)
    }

    public func isPaused(now: Date) -> Bool {
        guard let pausedUntil else { return false }
        return now < pausedUntil
    }

    public mutating func recordFailure(at now: Date) {
        failures += 1
        guard failures >= Self.freeAttempts else { return }
        // 1 minute at the third failure, doubling with each further one, up to an hour.
        let doublings = min(failures - Self.freeAttempts, 6)
        let pause = min(Self.firstPause * pow(2, Double(doublings)), Self.longestPause)
        pausedUntil = now.addingTimeInterval(pause)
    }

    public mutating func recordSuccess() {
        self = PasswordThrottle()
    }
}

extension Date {
    /// The start of the next whole minute, or this instant if it is already on a minute. A time shown without
    /// seconds must not be earlier than the moment it stands for, or someone acting at that minute is too early.
    public var roundedUpToMinute: Date {
        let seconds = timeIntervalSinceReferenceDate
        return Date(timeIntervalSinceReferenceDate: (seconds / 60).rounded(.up) * 60)
    }
}
