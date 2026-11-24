import FigLockFFI
import Foundation
import Testing

@testable import Shared

// MARK: Forgot password (FIG-229)

/// A phone that holds the Recovery Key fixture's vault, with its own device keys, locked.
@MainActor
private func phoneWithRecoveryFixture() throws -> (Blank, String) {
    let phone = try Blank.make()
    let store = try phone.vault.openStore()
    try store.putNew(path: "vault.json", bytes: RecoveryFixture.vaultJson)
    try phone.vault.markVaultPresent()
    guard case .ready(let vaultId, _) = phone.vault.status() else { throw Onboarding.Failure.failed }
    try phone.keeper.saveDeviceKeys(try DeviceKeysHandle.generate(), vaultId: vaultId)
    try phone.keeper.keep(RecoveryFixture.oldSecretKey, vaultId: vaultId)
    return (phone, vaultId)
}

@MainActor
@Test func theRecoveryKeySetsANewPasswordAndSecretKeyAndStoresNothingUntilTheKitIsChecked() async throws {
    let (phone, vaultId) = try phoneWithRecoveryFixture()
    defer { phone.cleanup() }
    let onboarding = phone.onboarding
    let before = try phone.vault.openStore().get(path: "vault.json")

    let recovering = try await onboarding.startRecovery(code: RecoveryFixture.recoveryKey, deviceName: "Test iPhone")
    try await onboarding.prepareRecovery(recovering, newPassword: "generated-new-password-2")
    let newKey = try #require(recovering.newSecretKeyText())
    #expect(newKey != RecoveryFixture.oldSecretKey)
    #expect(try recovering.emergencyKit(date: "2026-11-25").starts(with: Data("%PDF".utf8)))
    #expect(!recovering.verify(typed: "AAAA-AAAA"))
    #expect(recovering.verify(typed: lastTwoGroups(of: newKey)))

    // Preparing wrote nothing: the old vault and the old Secret Key are still what the phone has.
    #expect(try phone.vault.openStore().get(path: "vault.json") == before)
    #expect(try phone.keeper.secretKey(vaultId: vaultId) == RecoveryFixture.oldSecretKey)

    #expect(try onboarding.commitRecovery(recovering) == newKey)
    #expect(try phone.vault.openStore().get(path: "vault.json") != before)
    #expect(try phone.keeper.secretKey(vaultId: vaultId) == nil)

    // The new credentials open the vault; the old ones no longer do.
    let lock = phone.lock
    #expect(
        await lock.unlock(password: RecoveryFixture.oldPassword, secretKey: RecoveryFixture.oldSecretKey) != .unlocked)
    #expect(await lock.unlock(password: "generated-new-password-2", secretKey: newKey) == .unlocked)
    #expect(try phone.keeper.secretKey(vaultId: vaultId) == newKey)
}

@MainActor
@Test func aWrongTamperedOrMissingRecoveryKeyChangesNothing() async throws {
    let (phone, vaultId) = try phoneWithRecoveryFixture()
    defer { phone.cleanup() }
    let onboarding = phone.onboarding
    let before = try phone.vault.openStore().get(path: "vault.json")

    await #expect(throws: Onboarding.Failure.badRecoveryKey) {
        _ = try await onboarding.startRecovery(code: "not a recovery key", deviceName: "Test iPhone")
    }
    // A well formed key of some other vault (a made-up one with a valid check) does not open this one.
    let other = try #require(RecoveryFixture.recoveryKey.replacingOccurrences(of: "JJFG", with: "JJFH") as String?)
    await #expect(throws: Onboarding.Failure.self) {
        _ = try await onboarding.startRecovery(code: other, deviceName: "Test iPhone")
    }
    #expect(try phone.vault.openStore().get(path: "vault.json") == before)
    #expect(try phone.keeper.secretKey(vaultId: vaultId) == RecoveryFixture.oldSecretKey)

    // Backing out after the key was accepted, before the commit, leaves the vault as it was.
    let recovering = try await onboarding.startRecovery(code: RecoveryFixture.recoveryKey, deviceName: "Test iPhone")
    try await onboarding.prepareRecovery(recovering, newPassword: "generated-new-password-2")
    #expect(try phone.vault.openStore().get(path: "vault.json") == before)
    let lock = phone.lock
    #expect(await lock.unlock(password: RecoveryFixture.oldPassword, secretKey: nil) == .unlocked)
}

@MainActor
@Test func aVaultWithoutARecoveryKeyIsToldSo() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let onboarding = phone.onboarding
    let pending = try await onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await onboarding.commitCreation(pending, enableFaceID: false)
    await #expect(throws: Onboarding.Failure.noRecoveryKey) {
        _ = try await onboarding.startRecovery(code: RecoveryFixture.recoveryKey, deviceName: "Test iPhone")
    }
}
