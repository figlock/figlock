import FigLockFFI
import Foundation
import Testing

@testable import Shared

@Test func theSecretKeyIsKeptUntilThePersonStops() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let keeper = phone.keeper

    #expect(try keeper.secretKey(vaultId: phone.vaultId) == phone.secretKey)
    #expect(!(try keeper.needsTyping(vaultId: phone.vaultId)))

    try keeper.stopKeeping(vaultId: phone.vaultId)
    #expect(keeper.isOptedOut(vaultId: phone.vaultId))
    #expect(try keeper.needsTyping(vaultId: phone.vaultId))
}

@Test func theChoiceNotToKeepItHoldsAcrossUnlocksAndRelaunches() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    try phone.keeper.stopKeeping(vaultId: phone.vaultId)

    // A full unlock, an update banner and a rotation all call `keep`: none of them may put it back.
    #expect(try phone.keeper.keep(phone.secretKey, vaultId: phone.vaultId) == false)
    #expect(try phone.keeper.secretKey(vaultId: phone.vaultId) == nil)
    // A new keeper over the same files (the app relaunched) still knows.
    #expect(phone.keeper.isOptedOut(vaultId: phone.vaultId))
}

@Test func keepingItAgainIsOneActionAndClearsTheChoice() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    try phone.keeper.stopKeeping(vaultId: phone.vaultId)
    try phone.keeper.keepAgain(phone.secretKey, vaultId: phone.vaultId)

    #expect(!phone.keeper.isOptedOut(vaultId: phone.vaultId))
    #expect(try phone.keeper.secretKey(vaultId: phone.vaultId) == phone.secretKey)
    #expect(try phone.keeper.keep(phone.secretKey, vaultId: phone.vaultId))
}

@Test func theChoiceIsPerVault() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    try phone.keeper.stopKeeping(vaultId: phone.vaultId)
    #expect(!phone.keeper.isOptedOut(vaultId: "another-vault"))
}

@Test func aRotationDropsTheStaleKeyButNotTheChoice() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    try phone.keeper.discardStale(vaultId: phone.vaultId)
    #expect(try phone.keeper.needsTyping(vaultId: phone.vaultId))
    #expect(!phone.keeper.isOptedOut(vaultId: phone.vaultId))
}

@Test func removingTheVaultRemovesEverythingKept() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    try phone.keeper.stopKeeping(vaultId: phone.vaultId)
    try phone.keeper.removeForRemoval(vaultId: phone.vaultId)
    // Every key and the Secret Key are gone; only this phone's own device id stays (ADR-0044).
    #expect(phone.secrets.accounts == ["earlier-device.\(phone.vaultId)"])
    #expect(try phone.keeper.deviceKeys(vaultId: phone.vaultId) == nil)
    #expect(try phone.keeper.secretKey(vaultId: phone.vaultId) == nil)
    #expect(try phone.keeper.earlierDeviceId(vaultId: phone.vaultId) == phone.deviceKeys.deviceId())
    #expect(!phone.keeper.isOptedOut(vaultId: phone.vaultId))
}

@Test func aPhoneAddedAgainMarksItsEarlierEntryAsItsOwnWithNewKeys() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let store = try phone.vault.openStore()
    let records = try store.list(prefix: "devices/").map { try store.get(path: $0.path) }
    let before = try phone.deviceKeys.deviceId()

    try phone.keeper.removeForRemoval(vaultId: phone.vaultId)
    let fresh = try DeviceKeysHandle.generate()
    try phone.keeper.saveDeviceKeys(fresh, vaultId: phone.vaultId)
    let loaded = try #require(try phone.keeper.deviceKeys(vaultId: phone.vaultId))
    #expect(try loaded.deviceId() != before, "new keys, not the old ones")

    // The unlock after the re-add writes a record that names the earlier id.
    let unlocked = try unlockVault(
        vaultJson: store.get(path: "vault.json"), password: Phone.password, secretKey: phone.secretKey,
        device: loaded, deviceName: "iPhone", deviceRecords: records, nowMs: 1_800_000_200_000)
    let record = try #require(unlocked.newDeviceRecord)
    let body = try #require(try JSONSerialization.jsonObject(with: record) as? [String: Any])
    #expect(body["replaces"] as? String == before)
    #expect(body["device_id"] as? String == (try loaded.deviceId()))
    // Another device that reads the records lists the earlier entry as this phone's.
    let entries = try unlocked.session.devices()
    #expect(entries.first { $0.id == before }?.replacedBy == (try loaded.deviceId()))
}

@Test func aFirstInstallWritesNoReplacesAndAnotherVaultIsNotAffected() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    // A rolled back setup keeps nothing, not even the id.
    try phone.keeper.removeAll(vaultId: phone.vaultId)
    #expect(phone.secrets.accounts.isEmpty)
    #expect(try phone.keeper.earlierDeviceId(vaultId: "another-vault") == nil)
    #expect(try phone.keeper.deviceKeys(vaultId: "another-vault") == nil)
}

@Test func turningFaceIDOffDeletesTheKeyTheBlobAndTheState() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let key = try deriveUnlockKey(
        vaultJson: try phone.vault.openStore().get(path: "vault.json"), password: Phone.password,
        secretKey: phone.secretKey)
    try phone.quick.enable(key: key, vaultId: phone.vaultId, deviceId: phone.deviceKeys.deviceId())
    #expect(phone.quick.status() == .ready)
    #expect(phone.chip.hasKey)

    phone.quick.disable()
    #expect(phone.quick.status() == .off)
    #expect(!phone.chip.hasKey)
    #expect(!phone.quick.isWanted)
    await #expect(throws: QuickUnlock.Failure.off) {
        try await phone.quick.unlock(vaultId: phone.vaultId, deviceId: phone.deviceKeys.deviceId(), reason: "x")
    }
}

@Test func aBlobFromAnotherDeviceOrVaultDoesNotOpen() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let key = try deriveUnlockKey(
        vaultJson: try phone.vault.openStore().get(path: "vault.json"), password: Phone.password,
        secretKey: phone.secretKey)
    try phone.quick.enable(key: key, vaultId: phone.vaultId, deviceId: phone.deviceKeys.deviceId())

    await #expect(throws: QuickUnlock.Failure.needsPassword(.biometricsChanged)) {
        try await phone.quick.unlock(
            vaultId: phone.vaultId, deviceId: "00000000000000000000000000000000", reason: "x")
    }
    #expect(!phone.chip.hasKey)
}
