import FigLockFFI
import Foundation
import Testing

@testable import Shared

// Settings: the auto-lock choice, device names, revoking a device on a real vault, the Emergency Kit and the Drive
// copy deletion. Passwords and keys are generated for these tests.

// MARK: Auto-lock and device names

@MainActor
@Test func autoLockOptionsHaveNoNeverAndUnknownValuesFallBackToTheDefault() {
    #expect(AutoLockOption.allCases.map(\.seconds) == [0, 60, 300, 900, 3600])
    #expect(AutoLockOption.standard.seconds == AppLock.defaultAutoLock)
    #expect(AutoLockOption.from(seconds: nil) == .standard)
    #expect(AutoLockOption.from(seconds: 86_400) == .standard)
    #expect(AutoLockOption.from(seconds: 900) == .fifteenMinutes)
}

@Test func theAutoLockChoiceAndDeviceNamesSurviveARestart() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let first = DeviceSettings(container: phone.container)
    #expect(first.autoLock == .standard)
    try first.setAutoLock(.oneMinute)
    try first.setNickname("  Kitchen iPad  ", for: "aa")

    let second = DeviceSettings(container: phone.container)
    #expect(second.autoLock == .oneMinute)
    #expect(second.nicknames == ["aa": "Kitchen iPad"])
    try second.setNickname("", for: "aa")
    #expect(second.nicknames.isEmpty, "an empty name takes the nickname away")

    second.clear()
    #expect(DeviceSettings(container: phone.container).autoLock == .standard)
}

@Test func aNicknameIsTrimmedAndCut() {
    #expect(DeviceSettings.cleaned("   ") == nil)
    #expect(DeviceSettings.cleaned(String(repeating: "x", count: 100))?.count == DeviceSettings.maxNameLength)
}

@MainActor
@Test func theAutoLockTimeFromSettingsDecidesWhenTheAppLocks() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let lock = phone.makeLock()
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)

    lock.autoLockAfter = AutoLockOption.oneMinute.seconds
    lock.appDidEnterBackground()
    phone.clock.advance(59)
    lock.appWillEnterForeground()
    #expect(lock.isUnlocked)

    lock.appDidEnterBackground()
    phone.clock.advance(61)
    lock.appWillEnterForeground()
    #expect(!lock.isUnlocked)

    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
    lock.autoLockAfter = AutoLockOption.immediately.seconds
    lock.appDidEnterBackground()
    #expect(!lock.isUnlocked, "immediately locks as soon as the app leaves the screen")
}

// MARK: Device list

private func entry(
    _ id: String, name: String?, revoked: UInt64? = nil, current: Bool = false, replacedBy: String? = nil
) -> DeviceEntry {
    DeviceEntry(
        id: id, name: name, fingerprint: "fp-\(id)", revokedMs: revoked, isThisDevice: current, replacedBy: replacedBy)
}

@Test func theDeviceListPutsThisPhoneFirstAndRevokedLast() {
    let rows = DeviceList.rows(
        entries: [
            entry("c", name: "Work Mac"), entry("r1", name: nil, revoked: 1_000), entry("a", name: "iPad"),
            entry("r2", name: "Old Mac", revoked: 2_000), entry("b", name: "iPhone", current: true),
        ],
        nicknames: ["a": "Kitchen iPad"])
    #expect(rows.map(\.id) == ["b", "a", "c", "r2", "r1"])
    #expect(rows[1].displayName == "Kitchen iPad")
    #expect(rows[1].vaultName == "iPad")
    #expect(rows[4].displayName == "Unnamed device", "a revoked device from before ADR-0036 has no name")
    #expect(rows[3].isRevoked && !rows[0].isRevoked)
}

@Test func aPhoneAddedAgainShowsOnceAndItsEarlierEntriesAreNotListed() {
    // The same iPhone added to the vault three times (ADR-0044): one row, the older live ones are not listed.
    let rows = DeviceList.rows(
        entries: [
            entry("p1", name: "iPhone", replacedBy: "p2"), entry("p2", name: "iPhone", replacedBy: "p3"),
            entry("p3", name: "iPhone", current: true), entry("m", name: "Mac"),
            entry("old", name: "iPhone", revoked: 1_000, replacedBy: "p3"),
        ],
        nicknames: [:])
    #expect(rows.map(\.id) == ["p3", "m", "old"])
    #expect(rows.filter { !$0.isRevoked }.map(\.id) == ["p3", "m"])
    #expect(rows[2].isRevoked, "a revoked entry stays in the revoked list")
}

// MARK: Revoking

/// This phone's vault with a second device added to it, and the second device's id.
@MainActor
private func twoDevices(_ phone: Phone) async throws -> (session: VaultSession, other: String) {
    let lock = phone.makeLock()
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
    let session = try #require(lock.session)
    let store = try phone.vault.openStore()
    let tablet = try unlockVault(
        vaultJson: store.get(path: "vault.json"), password: Phone.password, secretKey: phone.secretKey,
        device: DeviceKeysHandle.generate(), deviceName: "Tablet",
        deviceRecords: try store.list(prefix: "devices/").map { try store.get(path: $0.path) },
        nowMs: 1_800_000_100_000)
    let record = try #require(tablet.newDeviceRecord)
    let other = try tablet.session.deviceId()
    try store.putNew(path: "devices/\(other).json", bytes: record)
    try session.addDeviceRecord(bytes: record)
    return (session, other)
}

@MainActor
@Test func revokingADeviceWritesTheNewVaultStateAndKeepsTheName() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let (session, other) = try await twoDevices(phone)
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)
    let store = try phone.vault.openStore()
    let before = try store.get(path: "vault.json")

    let result = try await service.revoke(
        deviceId: other, password: Phone.password, typedSecretKey: nil, session: session, remote: nil)
    #expect(result == SettingsService.Revoked(pushed: false))

    #expect(try store.get(path: "vault.json") != before, "vault.json is at the new epoch")
    #expect(try store.list(prefix: "devices/").map(\.path).allSatisfy { !$0.contains(other) })
    #expect(try !store.list(prefix: "snap/").isEmpty)
    let row = try #require(try session.devices().first { $0.id == other })
    #expect(row.revokedMs != nil)
    #expect(row.name == "Tablet", "the name outlives the record (ADR-0036)")
    #expect(try session.epoch() == 2)

    // The phone still opens the vault at the new epoch.
    let lock = phone.makeLock()
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
    #expect(try lock.session?.devices().count == 2)
}

@MainActor
@Test func aRevocationIsPushedToTheRemoteWhenThereIsOne() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let (session, other) = try await twoDevices(phone)
    let remoteFolder = FileManager.default.temporaryDirectory.appendingPathComponent("remote-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: remoteFolder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: remoteFolder) }
    let remote = try FolderStore.open(path: remoteFolder.path)
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)

    let result = try await service.revoke(
        deviceId: other, password: Phone.password, typedSecretKey: nil, session: session, remote: remote)
    #expect(result.pushed)
    let local = try phone.vault.openStore()
    #expect(try remote.get(path: "vault.json") == local.get(path: "vault.json"))
    #expect(try remote.list(prefix: "devices/").allSatisfy { !$0.path.contains(other) })
}

@MainActor
@Test func revokingChangesNothingWhenTheCredentialsOrTheTargetAreWrong() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let (session, other) = try await twoDevices(phone)
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)
    let store = try phone.vault.openStore()
    let before = try store.get(path: "vault.json")
    let myself = try session.deviceId()

    func attempt(_ id: String, _ password: String, _ key: String? = nil) async -> SettingsService.Failure? {
        do {
            _ = try await service.revoke(
                deviceId: id, password: password, typedSecretKey: key, session: session, remote: nil)
            return nil
        } catch {
            return error as? SettingsService.Failure
        }
    }

    #expect(await attempt(other, "not-the-password") == .wrongCredentials)
    let stranger = try Phone.make()
    defer { stranger.cleanup() }
    #expect(await attempt(other, Phone.password, stranger.secretKey) == .wrongCredentials)
    #expect(await attempt(other, Phone.password, "not a key") == .secretKeyNeeded)
    #expect(await attempt(myself, Phone.password) == .notAllowed, "a device cannot revoke itself")
    #expect(await attempt(String(repeating: "ab", count: 16), Phone.password) == .notAllowed)

    #expect(try store.get(path: "vault.json") == before)
    #expect(try session.devices().allSatisfy { $0.revokedMs == nil })

    // Once revoked, a second attempt is refused as well.
    _ = try await service.revoke(
        deviceId: other, password: Phone.password, typedSecretKey: nil, session: session, remote: nil)
    #expect(await attempt(other, Phone.password) == .notAllowed)
}

@MainActor
@Test func aPhoneThatKeepsNoSecretKeyNeedsItTypedToRevoke() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let (session, other) = try await twoDevices(phone)
    try phone.keeper.discardStale(vaultId: phone.vaultId)
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)

    await #expect(throws: SettingsService.Failure.secretKeyNeeded) {
        _ = try await service.revoke(
            deviceId: other, password: Phone.password, typedSecretKey: nil, session: session, remote: nil)
    }
    _ = try await service.revoke(
        deviceId: other, password: Phone.password, typedSecretKey: phone.secretKey, session: session, remote: nil)
    #expect(try session.devices().first { $0.id == other }?.revokedMs != nil)
}

// MARK: Emergency Kit and Secret Key

@MainActor
@Test func theSecretKeyShowsAfterAuthenticationAndMasksAgainAfterTenSeconds() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)
    let gate = SleepGate()

    let denied = SecretKeyReveal(service: service, authenticator: FakeAuthenticator(allow: false))
    #expect(await denied.show() == false)
    #expect(denied.shown == nil)

    let reveal = SecretKeyReveal(
        service: service, authenticator: FakeAuthenticator(allow: true), sleep: { await gate.sleep($0) })
    #expect(await reveal.show())
    #expect(reveal.shown == phone.secretKey)
    await waitFor { await gate.sleeping == 1 }
    #expect(await gate.requested == [SecretKeyReveal.visibleFor])
    await gate.fire()
    await waitFor { reveal.shown == nil }
    #expect(reveal.shown == nil)
}

@MainActor
@Test func theScenePausingForFaceIDDoesNotThrowAwayTheSecretKey() async throws {
    // The Face ID sheet takes the scene to inactive while it is up; the reveal it asks for must still show (FIG-233).
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let authenticator = GatedAuthenticator()
    let reveal = SecretKeyReveal(
        service: SettingsService(vault: phone.vault, keeper: phone.keeper), authenticator: authenticator,
        sleep: { _ in })

    let pending = Task { await reveal.show() }
    await waitFor { await authenticator.pending > 0 }
    reveal.cleanUpForScenePhase(isBackground: false)
    await authenticator.release()
    #expect(await pending.value)
    #expect(reveal.shown == phone.secretKey)

    // With nothing pending, leaving the foreground masks it again.
    reveal.cleanUpForScenePhase(isBackground: false)
    #expect(reveal.shown == nil)
}

@MainActor
@Test func theAppGoingToTheBackgroundDuringFaceIDStillCancelsTheSecretKey() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let authenticator = GatedAuthenticator()
    let reveal = SecretKeyReveal(
        service: SettingsService(vault: phone.vault, keeper: phone.keeper), authenticator: authenticator,
        sleep: { _ in })

    let pending = Task { await reveal.show() }
    await waitFor { await authenticator.pending > 0 }
    reveal.cleanUpForScenePhase(isBackground: true)
    await authenticator.release()
    #expect(await pending.value == false)
    #expect(reveal.shown == nil)
}

@MainActor
@Test func aPhoneWithoutAKeptSecretKeyShowsNothing() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    try phone.keeper.discardStale(vaultId: phone.vaultId)
    let reveal = SecretKeyReveal(
        service: SettingsService(vault: phone.vault, keeper: phone.keeper),
        authenticator: FakeAuthenticator(allow: true))
    #expect(!reveal.isKept)
    #expect(await reveal.show() == false)
    #expect(await reveal.prepareKit(storage: "On this iPhone") == nil)
}

@MainActor
@Test func theKitIsAPdfWithTheKeyAndIsRemovedOnCleanUp() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let folder = FileManager.default.temporaryDirectory.appendingPathComponent("kit-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: folder) }
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)

    let denied = SecretKeyReveal(service: service, authenticator: FakeAuthenticator(allow: false), kitFolder: folder)
    #expect(await denied.prepareKit(storage: "On this iPhone") == nil)
    #expect(try FileManager.default.contentsOfDirectory(atPath: folder.path).isEmpty)

    let reveal = SecretKeyReveal(service: service, authenticator: FakeAuthenticator(allow: true), kitFolder: folder)
    let url = try #require(await reveal.prepareKit(storage: "On this iPhone"))
    #expect(try Data(contentsOf: url).starts(with: Data("%PDF".utf8)))
    reveal.cleanUp()
    #expect(!FileManager.default.fileExists(atPath: url.path))
    #expect(reveal.kitFile == nil)
}

@MainActor
@Test func removingTheVaultWorksWhenNotSignedInToGoogle() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let driveSecrets = MemorySecrets()
    let onboarding = Onboarding(
        vault: phone.vault, keeper: phone.keeper, quick: phone.quick, driveGrants: driveSecrets)
    try onboarding.removeFromThisPhone()
    #expect(phone.vault.status() == .noVault)
    #expect(driveSecrets.accounts.isEmpty)
}

// MARK: Delete the Drive copy

@MainActor
@Test func theDriveCopyIsDeletedOnlyWithThePhraseAndTheLocalVaultStays() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let remoteFolder = FileManager.default.temporaryDirectory.appendingPathComponent("drive-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: remoteFolder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: remoteFolder) }
    let remote = try FolderStore.open(path: remoteFolder.path)
    let local = try phone.vault.openStore()
    try remote.putNew(path: "vault.json", bytes: local.get(path: "vault.json"))
    for entry in try local.list(prefix: "devices/") {
        try remote.putNew(path: entry.path, bytes: local.get(path: entry.path))
    }
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)

    await #expect(throws: SettingsService.Failure.failed) {
        _ = try await service.deleteDriveCopy(typed: "delete", remote: remote)
    }
    #expect(try !remote.list(prefix: "").isEmpty, "a wrong phrase deletes nothing")

    #expect(SettingsService.isDeleteFromDrivePhrase("  DELETE FROM GOOGLE DRIVE "))
    let files = try await service.deleteDriveCopy(typed: SettingsService.deleteFromDrivePhrase, remote: remote)
    #expect(files >= 2)
    #expect(try remote.list(prefix: "").isEmpty)
    #expect(try local.get(path: "vault.json").count > 0, "the vault on this phone stays")
    #expect(try phone.keeper.deviceKeys(vaultId: phone.vaultId) != nil)
}

// MARK: Remove from this phone

@MainActor
@Test func removingTheVaultKeepsTheCloudCopyAndForgetsThisPhone() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let remoteFolder = FileManager.default.temporaryDirectory.appendingPathComponent("drive-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: remoteFolder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: remoteFolder) }
    let remote = try FolderStore.open(path: remoteFolder.path)
    try remote.putNew(path: "vault.json", bytes: phone.vault.openStore().get(path: "vault.json"))

    let settings = DeviceSettings(container: phone.container)
    try settings.setAutoLock(.oneHour)
    // Signed in to Google Drive on this phone: the refresh token goes with the vault, the Drive copy stays.
    let driveSecrets = MemorySecrets()
    try driveSecrets.write("google-drive-refresh-token", Data("generated-refresh-token".utf8))
    let onboarding = Onboarding(
        vault: phone.vault, keeper: phone.keeper, quick: phone.quick, driveGrants: driveSecrets)
    try onboarding.removeFromThisPhone()
    settings.clear()
    #expect(driveSecrets.accounts.isEmpty, "removing the vault signs out of Google")
    #expect(try remote.get(path: "vault.json").count > 0, "the cloud copy is untouched")

    #expect(phone.vault.status() == .noVault)
    #expect((try? phone.keeper.secretKey(vaultId: phone.vaultId)) == nil)
    #expect(try phone.keeper.deviceKeys(vaultId: phone.vaultId) == nil)
    #expect(settings.autoLock == .standard)
    #expect(try remote.get(path: "vault.json").count > 0, "the cloud copy is never touched")
}
