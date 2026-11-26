import FigLockFFI
import Foundation
import Testing

@testable import Shared

/// Unlock on a real vault made by the Rust core, with a software key standing in for the Secure Enclave.
/// Passwords and keys are generated for these tests.

@MainActor
@Test func thePasswordUnlocksAndLocksAgain() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = phone.makeLock()

    #expect(!lock.needsSecretKeyTyped())
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
    #expect(lock.isUnlocked)
    lock.lock()
    #expect(!lock.isUnlocked)
}

@MainActor
@Test func aWrongPasswordFailsAndCountsDownToAPause() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = phone.makeLock()

    #expect(await lock.unlock(password: "wrong-1", secretKey: nil) == .wrongCredentials(triesLeft: 2))
    #expect(await lock.unlock(password: "wrong-2", secretKey: nil) == .wrongCredentials(triesLeft: 1))
    guard case .paused(let until) = await lock.unlock(password: "wrong-3", secretKey: nil) else {
        Issue.record("the third wrong password should pause entry")
        return
    }
    #expect(until == phone.clock.date.addingTimeInterval(60))
    #expect(!lock.isUnlocked)

    // While paused even the right password is refused, so the pause cannot be skipped by luck.
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .paused(until: until))

    // A new AppLock (the app was killed and reopened) still sees the pause.
    #expect(phone.makeLock().passwordPausedUntil == until)

    phone.clock.advance(61)
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
    #expect(lock.passwordPausedUntil == nil)
}

@MainActor
@Test func aWrongSecretKeyLooksLikeAWrongPassword() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let other = try Phone.make()
    defer { other.cleanup() }
    let lock = phone.makeLock()

    #expect(
        await lock.unlock(password: Phone.password, secretKey: other.secretKey) == .wrongCredentials(triesLeft: 2))
    #expect(await lock.unlock(password: Phone.password, secretKey: "not a key") == .secretKeyNeeded)
}

@MainActor
@Test func aPhoneWithoutAKeptSecretKeyAsksForItOnceAndThenKeepsIt() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    try phone.keeper.discardStale(vaultId: phone.vaultId)
    let lock = phone.makeLock()

    #expect(lock.needsSecretKeyTyped())
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .secretKeyNeeded)
    #expect(await lock.unlock(password: Phone.password, secretKey: phone.secretKey) == .unlocked)
    #expect(!lock.needsSecretKeyTyped())
}

@MainActor
@Test func aPhoneWithoutDeviceKeysIsNotSetUp() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    try phone.secrets.delete("device-keys.\(phone.vaultId)")
    #expect(await phone.makeLock().unlock(password: Phone.password, secretKey: nil) == .notSetUp)
}

@MainActor
@Test func faceIDUnlocksAfterTheFirstPasswordUnlockTurnedItOn() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = phone.makeLock()
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
    // Turning it on is the person's choice, made after the first unlock (default Yes in the onboarding screens).
    let key = try deriveUnlockKey(
        vaultJson: try phone.vault.openStore().get(path: "vault.json"), password: Phone.password,
        secretKey: phone.secretKey)
    try phone.quick.enable(key: key, vaultId: phone.vaultId, deviceId: phone.deviceKeys.deviceId())
    lock.lock()

    #expect(lock.quickUnlockStatus == .ready)
    #expect(await lock.unlockWithFaceID() == .unlocked)
    #expect(lock.isUnlocked)
}

@MainActor
private func lockWithFaceIDOn(_ phone: Phone) async throws -> AppLock {
    let lock = phone.makeLock()
    _ = await lock.unlock(password: Phone.password, secretKey: nil)
    let key = try deriveUnlockKey(
        vaultJson: try phone.vault.openStore().get(path: "vault.json"), password: Phone.password,
        secretKey: phone.secretKey)
    try phone.quick.enable(key: key, vaultId: phone.vaultId, deviceId: phone.deviceKeys.deviceId())
    lock.lock()
    return lock
}

@MainActor
@Test func aCancelledFaceIDPromptIsNotAFailedAttempt() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = try await lockWithFaceIDOn(phone)

    phone.chip.behaviour = .cancelled
    for _ in 0..<10 { #expect(await lock.unlockWithFaceID() == .faceIDCancelled) }
    #expect(lock.quickUnlockStatus == .ready)
    #expect(!lock.isUnlocked)

    phone.chip.behaviour = .works
    #expect(await lock.unlockWithFaceID() == .unlocked)
}

@MainActor
@Test func fiveUnrecognisedAttemptsEndFaceIDAndTheKeyIsGone() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = try await lockWithFaceIDOn(phone)

    phone.chip.behaviour = .notRecognised
    for left in [4, 3, 2, 1] { #expect(await lock.unlockWithFaceID() == .faceIDNotRecognised(triesLeft: left)) }
    #expect(await lock.unlockWithFaceID() == .passwordNeeded(.tooManyFailures))
    #expect(!phone.chip.hasKey)
    #expect(lock.quickUnlockStatus == .needsPassword(.tooManyFailures))

    // Even a working chip is not asked again until the password has been typed.
    phone.chip.behaviour = .works
    let prompts = phone.chip.prompts
    #expect(await lock.unlockWithFaceID() == .passwordNeeded(.tooManyFailures))
    #expect(phone.chip.prompts == prompts)

    // The master password still works, and turns Face ID back on.
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
    #expect(lock.quickUnlockStatus == .ready)
    lock.lock()
    #expect(await lock.unlockWithFaceID() == .unlocked)
}

@MainActor
@Test func aChangedBiometricSetEndsFaceIDWithoutAPrompt() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = try await lockWithFaceIDOn(phone)

    phone.chip.biometricsState = Data("enrolled-2".utf8)
    let prompts = phone.chip.prompts
    #expect(await lock.unlockWithFaceID() == .passwordNeeded(.biometricsChanged))
    #expect(phone.chip.prompts == prompts)
    #expect(!phone.chip.hasKey)
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
}

@MainActor
@Test func aKeyTheSystemRemovedEndsFaceIDAndFallsBackToThePassword() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = try await lockWithFaceIDOn(phone)

    phone.chip.behaviour = .keyGone
    #expect(await lock.unlockWithFaceID() == .passwordNeeded(.biometricsChanged))
    #expect(lock.quickUnlockStatus == .needsPassword(.biometricsChanged))
}

@MainActor
@Test func aPhoneRestartAndLongIdleAndAgeEachNeedThePassword() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = try await lockWithFaceIDOn(phone)

    phone.clock.advance(4 * 3600 + 1)
    #expect(await lock.unlockWithFaceID() == .passwordNeeded(.idleTooLong))

    // Turned on again by the password; then the phone restarts.
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
    lock.lock()
    phone.boot.value = "boot-2"
    #expect(await lock.unlockWithFaceID() == .passwordNeeded(.phoneRestarted))
}

@MainActor
@Test func faceIDDoesNotOpenWithABlobMadeForAnotherDevice() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = phone.makeLock()
    let key = try deriveUnlockKey(
        vaultJson: try phone.vault.openStore().get(path: "vault.json"), password: Phone.password,
        secretKey: phone.secretKey)
    // The blob was sealed for a different device id, as when a restore put another phone's files here.
    try phone.quick.enable(key: key, vaultId: phone.vaultId, deviceId: "00000000000000000000000000000000")

    guard case .passwordNeeded = await lock.unlockWithFaceID() else {
        Issue.record("a blob made for another device must not open")
        return
    }
    #expect(!lock.isUnlocked)
    #expect(!phone.chip.hasKey)
}

@MainActor
@Test func theAppLocksWhenItWasInTheBackgroundLongerThanTheAutoLockTime() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = phone.makeLock()
    _ = await lock.unlock(password: Phone.password, secretKey: nil)

    lock.appDidEnterBackground()
    phone.clock.advance(60)
    lock.appWillEnterForeground()
    #expect(lock.isUnlocked)

    lock.appDidEnterBackground()
    phone.clock.advance(AppLock.defaultAutoLock)
    lock.appWillEnterForeground()
    #expect(!lock.isUnlocked)

    _ = await lock.unlock(password: Phone.password, secretKey: nil)
    lock.autoLockAfter = 0
    lock.appDidEnterBackground()
    #expect(!lock.isUnlocked)
}

@MainActor
@Test func aLockedSessionCannotReadItems() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = phone.makeLock()
    _ = await lock.unlock(password: Phone.password, secretKey: nil)
    let session = try #require(lock.session)
    lock.lock()
    #expect(throws: FigLockError.self) { try session.items(includeDeleted: false) }
}

@MainActor
@Test func aChipErrorThatIsNotAChangeKeepsTheKeyAndReportsAFailure() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = try await lockWithFaceIDOn(phone)

    phone.chip.behaviour = .failing
    #expect(await lock.unlockWithFaceID() == .failed)
    #expect(phone.chip.hasKey)
    #expect(phone.quick.status() == .ready)

    phone.chip.behaviour = .works
    #expect(await lock.unlockWithFaceID() == .unlocked)
}

@MainActor
@Test func aStorageErrorIsAFailureAndDoesNotCountAgainstThePassword() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = phone.makeLock()
    let devices = phone.container.vaultFolder.appendingPathComponent("devices")
    try FileManager.default.removeItem(at: devices)
    try Data().write(to: devices)

    for _ in 0..<5 { #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .failed) }
    #expect(lock.passwordPausedUntil == nil)

    try FileManager.default.removeItem(at: devices)
    try FileManager.default.createDirectory(at: devices, withIntermediateDirectories: false)
    #expect(await lock.unlock(password: "wrong-1", secretKey: nil) == .wrongCredentials(triesLeft: 2))
}

/// iOS reported the extension a Face ID state of the same size as the app's but a different value (FIG-247), so the
/// extension keeps a baseline of its own and compares with that.
@Test func theExtensionComparesTheFaceStateWithItsOwnBaselineNotTheApps() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let key = try deriveUnlockKey(
        vaultJson: try phone.vault.openStore().get(path: "vault.json"), password: Phone.password,
        secretKey: phone.secretKey)
    // The app turns Face ID on and saves the state it sees.
    try phone.quick.enable(key: key, vaultId: phone.vaultId, deviceId: phone.deviceKeys.deviceId())
    let boot = phone.boot
    let clock = phone.clock
    let extensionSide = QuickUnlock(
        container: phone.container, chip: phone.chip, bootSession: { boot.value }, now: { clock.date },
        biometricStateIsPerProcess: true)

    // The extension is told something else for the same faces, as on a real phone. With no baseline yet that is
    // not a change, and strict comparison (the app's) would call it one.
    phone.chip.biometricsState = Data("extension-view".utf8)
    #expect(extensionSide.status() == .ready)
    #expect(phone.quick.status() == .needsPassword(.biometricsChanged))

    // Its first Face ID unlock takes the baseline. It then stays ready for the same state.
    _ = try await extensionSide.unlock(
        vaultId: phone.vaultId, deviceId: phone.deviceKeys.deviceId(), reason: "Fill")
    #expect(extensionSide.status() == .ready)
    #expect(extensionSide.biometricNote().contains("extension baseline 14 bytes (same)"))

    // A state that differs from the extension's own baseline is a real change, with no prompt.
    phone.chip.biometricsState = Data("a new face".utf8)
    let prompts = phone.chip.prompts
    #expect(extensionSide.status() == .needsPassword(.biometricsChanged))
    await #expect(throws: QuickUnlock.Failure.needsPassword(.biometricsChanged)) {
        try await extensionSide.unlock(vaultId: phone.vaultId, deviceId: phone.deviceKeys.deviceId(), reason: "Fill")
    }
    #expect(phone.chip.prompts == prompts)

    // A state the extension cannot read at all is not taken for a change either.
    let again = try Phone.make()
    defer { again.cleanup() }
    let againKey = try deriveUnlockKey(
        vaultJson: try again.vault.openStore().get(path: "vault.json"), password: Phone.password,
        secretKey: again.secretKey)
    try again.quick.enable(key: againKey, vaultId: again.vaultId, deviceId: again.deviceKeys.deviceId())
    let againBoot = again.boot
    let againClock = again.clock
    let unreadable = QuickUnlock(
        container: again.container, chip: again.chip, bootSession: { againBoot.value }, now: { againClock.date },
        biometricStateIsPerProcess: true)
    again.chip.biometricsState = nil
    #expect(unreadable.status() == .ready)
}
