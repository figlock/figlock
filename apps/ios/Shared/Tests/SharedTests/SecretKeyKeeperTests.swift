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
    try phone.keeper.removeAll(vaultId: phone.vaultId)
    #expect(phone.secrets.accounts.isEmpty)
    #expect(!phone.keeper.isOptedOut(vaultId: phone.vaultId))
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
