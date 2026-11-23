import Foundation
import Testing

@testable import Shared

private let start = Date(timeIntervalSince1970: 1_800_000_000)

private func policy() -> QuickUnlockPolicy {
    QuickUnlockPolicy(bootSession: "boot-1", lastFullUnlock: start, biometricState: Data("a".utf8))
}

private typealias Decision = QuickUnlockPolicy.Decision
private typealias Policy = QuickUnlockPolicy

private func decide(_ state: Policy, after secs: Double, boot: String = "boot-1", bio: String = "a") -> Decision {
    state.decision(now: start.addingTimeInterval(secs), bootSession: boot, currentBiometricState: Data(bio.utf8))
}

@Test func faceIDIsAllowedRightAfterAFullUnlock() {
    #expect(decide(policy(), after: 60) == .allowed)
}

@Test func aPhoneRestartNeedsThePassword() {
    #expect(decide(policy(), after: 60, boot: "boot-2") == .needsPassword(.phoneRestarted))
}

@Test func fourHoursWithoutUseNeedsThePasswordButUseKeepsItAlive() {
    var state = policy()
    #expect(decide(state, after: 4 * 3600) == .needsPassword(.idleTooLong))
    state.recordQuickUnlock(at: start.addingTimeInterval(3 * 3600))
    #expect(decide(state, after: 6 * 3600) == .allowed)
}

@Test func sevenDaysSinceTheFullUnlockNeedsThePasswordEvenWithUse() {
    var state = policy()
    state.recordQuickUnlock(at: start.addingTimeInterval(6.9 * 86400 - 60))
    #expect(decide(state, after: 6.9 * 86400) == .allowed)
    state.recordQuickUnlock(at: start.addingTimeInterval(7 * 86400 - 60))
    #expect(decide(state, after: 7 * 86400) == .needsPassword(.fullUnlockTooOld))
}

@Test func fiveFailedAttemptsNeedThePasswordAndASuccessResetsTheCount() {
    var state = policy()
    for _ in 0..<4 { state.recordFailure() }
    #expect(decide(state, after: 1) == .allowed)
    state.recordQuickUnlock(at: start.addingTimeInterval(1))
    #expect(state.failedAttempts == 0)
    for _ in 0..<5 { state.recordFailure() }
    #expect(decide(state, after: 2) == .needsPassword(.tooManyFailures))
}

@Test func aChangedBiometricSetNeedsThePassword() {
    #expect(decide(policy(), after: 60, bio: "b") == .needsPassword(.biometricsChanged))
}

@Test func aEndedPolicyStaysEndedWhateverElseIsTrue() {
    var state = policy()
    state.endedBy = .lockedByUser
    #expect(decide(state, after: 1) == .needsPassword(.lockedByUser))
}

@Test func passwordEntryPausesAfterThreeWrongPasswordsAndDoublesFromThere() {
    var throttle = PasswordThrottle()
    #expect(throttle.triesLeft(now: start) == 3)
    throttle.recordFailure(at: start)
    throttle.recordFailure(at: start)
    #expect(throttle.triesLeft(now: start) == 1)
    #expect(!throttle.isPaused(now: start))
    throttle.recordFailure(at: start)
    #expect(throttle.isPaused(now: start))
    #expect(throttle.pausedUntil == start.addingTimeInterval(60))
    #expect(throttle.triesLeft(now: start) == nil)

    let later = start.addingTimeInterval(61)
    #expect(!throttle.isPaused(now: later))
    #expect(throttle.triesLeft(now: later) == 1)
    throttle.recordFailure(at: later)
    #expect(throttle.pausedUntil == later.addingTimeInterval(120))

    throttle.recordSuccess()
    #expect(throttle == PasswordThrottle())
}

@Test func thePauseNeverGrowsPastAnHour() {
    var throttle = PasswordThrottle()
    for _ in 0..<30 { throttle.recordFailure(at: start) }
    #expect(throttle.pausedUntil == start.addingTimeInterval(3600))
}

@Test func aRetryTimeIsRoundedUpToTheNextMinute() {
    let minute = Date(timeIntervalSinceReferenceDate: 600)
    #expect(minute.addingTimeInterval(15).roundedUpToMinute == minute.addingTimeInterval(60))
    #expect(minute.addingTimeInterval(0.5).roundedUpToMinute == minute.addingTimeInterval(60))
    #expect(minute.roundedUpToMinute == minute)
}
