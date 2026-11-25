import FigLockFFI
import Foundation
import Testing

@testable import Shared

// Settings > Recovery Key (FIG-237, ADR-0029): turning it on, replacing it and turning it off on a real vault. Passwords
// and keys are generated for these tests.

@MainActor
private func unlocked(_ phone: Phone) async throws -> VaultSession {
    let lock = phone.makeLock()
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
    return try #require(lock.session)
}

@MainActor
@Test func aVaultStartsWithoutARecoveryKeyAndSettingsCanTurnItOnAndReplaceIt() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let session = try await unlocked(phone)
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)
    let store = try phone.vault.openStore()
    #expect(!service.hasRecoveryKey(session: session))
    #expect(try !vaultHasRecoveryKey(vaultJson: store.get(path: "vault.json")))

    let first = try newRecoveryDraft()
    try await service.setRecoveryKey(first, password: Phone.password, typedSecretKey: nil, session: session)
    #expect(service.hasRecoveryKey(session: session))
    let afterFirst = try store.get(path: "vault.json")
    #expect(try vaultHasRecoveryKey(vaultJson: afterFirst))

    // Replacing: the new code opens the vault, the earlier one no longer does (the same Rust `recover`).
    let second = try newRecoveryDraft()
    #expect(first.code() != second.code())
    try await service.setRecoveryKey(second, password: Phone.password, typedSecretKey: nil, session: session)
    let records = try store.list(prefix: "devices/").map { try store.get(path: $0.path) }
    let stored = try store.get(path: "vault.json")
    #expect(stored != afterFirst)
    func recovers(_ code: String) -> Bool {
        (try? startRecovery(
            code: code, vaultJson: stored, records: records, device: phone.deviceKeys, deviceName: "Test iPhone",
            nowMs: 1_800_000_200_000)) != nil
    }
    #expect(recovers(second.code()))
    #expect(!recovers(first.code()))
}

@MainActor
@Test func aWrongPasswordOrSecretKeyChangesNothingAboutTheRecoveryKey() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let session = try await unlocked(phone)
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)
    let store = try phone.vault.openStore()
    try await service.setRecoveryKey(
        try newRecoveryDraft(), password: Phone.password, typedSecretKey: nil, session: session)
    let before = try store.get(path: "vault.json")

    await #expect(throws: SettingsService.Failure.wrongCredentials) {
        try await service.setRecoveryKey(
            try newRecoveryDraft(), password: "generated-wrong-password", typedSecretKey: nil, session: session)
    }
    await #expect(throws: SettingsService.Failure.wrongCredentials) {
        try await service.removeRecoveryKey(password: "generated-wrong-password", typedSecretKey: nil, session: session)
    }
    // Another vault's Secret Key is as wrong as a wrong password; a malformed one is asked for again.
    let stranger = try createVault(
        password: "generated-other-password", deviceName: "x", kdf: minimumKdfSettings(), nowMs: 1_800_000_000_000)
    await #expect(throws: SettingsService.Failure.wrongCredentials) {
        try await service.removeRecoveryKey(
            password: Phone.password, typedSecretKey: try stranger.secretKeyText(), session: session)
    }
    await #expect(throws: SettingsService.Failure.secretKeyNeeded) {
        try await service.removeRecoveryKey(password: Phone.password, typedSecretKey: "FL1-nope", session: session)
    }
    #expect(try store.get(path: "vault.json") == before, "nothing was written")
    #expect(service.hasRecoveryKey(session: session))
}

@MainActor
@Test func removingTheRecoveryKeyTakesItOutOfTheVaultAndNeedsTheCredentials() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let session = try await unlocked(phone)
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)
    let store = try phone.vault.openStore()
    let draft = try newRecoveryDraft()
    try await service.setRecoveryKey(draft, password: Phone.password, typedSecretKey: nil, session: session)

    try await service.removeRecoveryKey(password: Phone.password, typedSecretKey: nil, session: session)
    #expect(!service.hasRecoveryKey(session: session))
    let stored = try store.get(path: "vault.json")
    #expect(try !vaultHasRecoveryKey(vaultJson: stored))
    // Nothing is left to remove.
    await #expect(throws: SettingsService.Failure.wrongCredentials) {
        try await service.removeRecoveryKey(password: Phone.password, typedSecretKey: nil, session: session)
    }
    // The vault still opens with the master password.
    let lock = phone.makeLock()
    #expect(await lock.unlock(password: Phone.password, secretKey: nil) == .unlocked)
}

@MainActor
@Test func aPhoneThatKeepsNoSecretKeyNeedsItTypedToChangeTheRecoveryKey() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let session = try await unlocked(phone)
    try phone.keeper.discardStale(vaultId: phone.vaultId)
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)
    await #expect(throws: SettingsService.Failure.secretKeyNeeded) {
        try await service.setRecoveryKey(
            try newRecoveryDraft(), password: Phone.password, typedSecretKey: nil, session: session)
    }
    try await service.setRecoveryKey(
        try newRecoveryDraft(), password: Phone.password, typedSecretKey: phone.secretKey, session: session)
    #expect(service.hasRecoveryKey(session: session))
}

@MainActor
@Test func theNewRecoveryKeyIsCheckedByItsLastTwoGroupsAndHasAPrintablePage() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let service = SettingsService(vault: phone.vault, keeper: phone.keeper)
    let draft = try newRecoveryDraft()
    #expect(recoveryKeyIsValid(code: draft.code()))
    let tail = draft.code().split(separator: "-").suffix(2).joined(separator: "-")
    #expect(draft.verifyTail(typed: tail))
    #expect(draft.verifyTail(typed: tail.lowercased()))
    #expect(!draft.verifyTail(typed: "AAAA-AAAA"))
    let page = try service.recoveryKitPage(draft, storage: "On this iPhone")
    #expect(page.starts(with: Data("%PDF".utf8)))
}
