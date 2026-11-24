import FigLockFFI
import Foundation
import Testing

@testable import Shared

/// A phone with no vault yet and a folder standing in for the Google Drive remote. Values are generated.
struct Blank {
    static let password = "generated-onboarding-password-1"

    let container: VaultContainer
    let vault: LocalVault
    let secrets = MemorySecrets()
    let chip = FakeChip()
    let clock = TestClock()
    let remoteFolder: URL
    let remote: any BlobStoreFfi

    var keeper: SecretKeyKeeper { SecretKeyKeeper(store: secrets, container: container) }

    var quick: QuickUnlock {
        let clock = clock
        return QuickUnlock(container: container, chip: chip, bootSession: { "boot-1" }, now: { clock.date })
    }

    var onboarding: Onboarding {
        let clock = clock
        return Onboarding(vault: vault, keeper: keeper, quick: quick, now: { clock.date })
    }

    @MainActor
    var lock: AppLock {
        let clock = clock
        return AppLock(vault: vault, keeper: keeper, quick: quick, deviceName: "Test iPhone", now: { clock.date })
    }

    func cleanup() {
        try? FileManager.default.removeItem(at: container.root)
        try? FileManager.default.removeItem(at: remoteFolder)
    }

    static func make() throws -> Blank {
        let base = FileManager.default.temporaryDirectory
            .appendingPathComponent("figlock-onboarding-\(UUID().uuidString)", isDirectory: true)
        let root = base.appendingPathComponent("phone", isDirectory: true)
        let remoteFolder = base.appendingPathComponent("remote", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: remoteFolder, withIntermediateDirectories: true)
        let container = VaultContainer(root: root)
        try container.prepare()
        return Blank(
            container: container, vault: LocalVault(container: container, access: .readWrite),
            remoteFolder: remoteFolder, remote: try FolderStore.open(path: remoteFolder.path))
    }

    /// Put a vault (cheap Argon2id) into the remote the way another device would have. Returns its Secret Key.
    @discardableResult
    func seedRemote() throws -> String {
        let created = try createVault(
            password: Self.password, deviceName: "Studio Mac", kdf: minimumKdfSettings(), nowMs: 1_800_000_000_000)
        try remote.putNew(path: "vault.json", bytes: created.vaultJson())
        try remote.putNew(path: "devices/\(created.deviceKeys().deviceId()).json", bytes: created.deviceRecord())
        return try created.secretKeyText()
    }
}

func lastTwoGroups(of secretKey: String) -> String {
    secretKey.split(separator: "-").suffix(2).joined(separator: "-")
}

// MARK: Create

@Test func creatingAVaultStoresNothingUntilTheKitCheckPasses() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }

    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    #expect(phone.vault.status() == .noVault)
    #expect(phone.secrets.accounts.isEmpty)

    let secretKey = try pending.secretKeyText()
    #expect(FigLockCore.isValidSecretKey(secretKey))
    #expect(try pending.emergencyKit(vaultName: "My vault", date: "2026-11-22").starts(with: Data("%PDF".utf8)))
    #expect(!pending.verify(typed: "AAAA-AAAA"))
    #expect(!pending.verify(typed: ""))
    #expect(pending.verify(typed: lastTwoGroups(of: secretKey)))
    // The check is lenient about case, spaces and dashes, like Secret Key entry everywhere.
    #expect(pending.verify(typed: lastTwoGroups(of: secretKey).lowercased().replacingOccurrences(of: "-", with: " ")))
}

@MainActor
@Test func aCreatedVaultOpensWithFaceIDOnAndTheKeysKept() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let onboarding = phone.onboarding

    let pending = try await onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    let faceID = try await onboarding.commitCreation(pending, enableFaceID: true)

    #expect(faceID)
    guard case .ready(let vaultId, _) = phone.vault.status() else {
        Issue.record("the vault should be stored")
        return
    }
    #expect(onboarding.rememberedVaultId == vaultId)
    #expect(try phone.keeper.secretKey(vaultId: vaultId) == pending.secretKeyText())
    #expect(try phone.keeper.deviceKeys(vaultId: vaultId) != nil)
    #expect(phone.quick.status() == .ready)

    let lock = phone.lock
    #expect(await lock.unlock(password: Blank.password, secretKey: nil) == .unlocked)
    lock.lock()
    #expect(await lock.unlockWithFaceID() == .unlocked)
}

@Test func faceIDStaysOffWhenNotAskedFor() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    #expect(try await phone.onboarding.commitCreation(pending, enableFaceID: false) == false)
    #expect(phone.quick.status() == .off)
    #expect(!phone.chip.hasKey)
}

@Test func aSecondVaultIsNotCreatedOverTheFirst() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await phone.onboarding.commitCreation(pending, enableFaceID: false)
    await #expect(throws: Onboarding.Failure.alreadyHasVault) {
        _ = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    }
}

@Test func committingTheSamePendingVaultTwiceLeavesTheFirstOneUntouched() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await phone.onboarding.commitCreation(pending, enableFaceID: false)
    guard case .ready(let vaultId, _) = phone.vault.status() else {
        Issue.record("the vault should be stored")
        return
    }
    let secretKey = try phone.keeper.secretKey(vaultId: vaultId)
    await #expect(throws: Onboarding.Failure.alreadyHasVault) {
        try await phone.onboarding.commitCreation(pending, enableFaceID: false)
    }
    guard case .ready(let after, _) = phone.vault.status() else {
        Issue.record("the first vault should still be stored")
        return
    }
    #expect(after == vaultId)
    #expect(try phone.keeper.secretKey(vaultId: vaultId) == secretKey)
    #expect(try phone.keeper.deviceKeys(vaultId: vaultId) != nil)
    #expect(phone.onboarding.rememberedVaultId == vaultId)
}

// MARK: Open from Drive

@Test func aRemoteWithoutAVaultIsReportedAsSuch() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    #expect(try phone.onboarding.inspect(remote: phone.remote) == nil)
    await #expect(throws: Onboarding.Failure.noVaultInStorage) {
        try await phone.onboarding.openFromRemote(phone.remote)
    }
    #expect(phone.vault.status() == .noVault)
}

@MainActor
@Test func openingTheVaultFromDriveNeedsThePasswordAndSecretKeyOnce() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let secretKey = try phone.seedRemote()

    let found = try #require(try phone.onboarding.inspect(remote: phone.remote))
    try await phone.onboarding.openFromRemote(phone.remote)
    guard case .ready(let vaultId, _) = phone.vault.status() else {
        Issue.record("the vault should be downloaded")
        return
    }
    #expect(vaultId == found.vaultId)
    // New device keys, and no Secret Key yet: the unlock screen has to ask for it.
    #expect(try phone.keeper.deviceKeys(vaultId: vaultId) != nil)
    #expect(try phone.keeper.needsTyping(vaultId: vaultId))

    let lock = phone.lock
    #expect(await lock.unlock(password: Blank.password, secretKey: "FL1-AAAA-AAAA-AAAA-AAAA-AAAA") != .unlocked)
    #expect(await lock.unlock(password: Blank.password, secretKey: secretKey) == .unlocked)
    // The unlock added this phone's record, and the Secret Key is kept for next time.
    let records = try phone.vault.openStore().list(prefix: "devices/")
    #expect(records.count == 2)
    #expect(!(try phone.keeper.needsTyping(vaultId: vaultId)))
}

@Test func aPhoneThatHasAVaultDoesNotDownloadAnother() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await phone.onboarding.commitCreation(pending, enableFaceID: false)
    try phone.seedRemote()
    await #expect(throws: Onboarding.Failure.alreadyHasVault) {
        try await phone.onboarding.openFromRemote(phone.remote)
    }
}

// MARK: Pairing

@Test func pairingRefusesBadCodesAndEmptyStorageWithoutWritingAnything() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }

    await #expect(throws: Onboarding.Failure.invalidCode) {
        _ = try await phone.onboarding.beginPairing(code: "nope", remote: phone.remote, deviceName: "Test iPhone")
    }
    await #expect(throws: Onboarding.Failure.noVaultInStorage) {
        _ = try await phone.onboarding.beginPairing(
            code: "ABCD-EFGH-JKMN-PQRS-TVWX", remote: phone.remote, deviceName: "Test iPhone")
    }

    try phone.seedRemote()
    // A well formed code whose invite is not in the storage looks the same as a typo.
    await #expect(throws: Onboarding.Failure.invalidCode) {
        _ = try await phone.onboarding.beginPairing(
            code: "ABCD-EFGH-JKMN-PQRS-TVWX", remote: phone.remote, deviceName: "Test iPhone")
    }
    #expect(try phone.remote.list(prefix: "inbox/").isEmpty)
    #expect(phone.vault.status() == .noVault)
    #expect(phone.secrets.accounts.isEmpty)
}

// MARK: Recovery

@Test func aMissingVaultFolderCanBeRestoredFromTheRemoteKeepingThePhonesKeys() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await phone.onboarding.commitCreation(pending, enableFaceID: false)
    guard case .ready(let vaultId, _) = phone.vault.status() else {
        Issue.record("vault expected")
        return
    }
    let keysBefore = try #require(try phone.keeper.deviceKeys(vaultId: vaultId)).toBytes()
    // What Drive holds: a copy of the vault files.
    let local = try phone.vault.openStore()
    for entry in try local.list(prefix: "") {
        try phone.remote.putNew(path: entry.path, bytes: try local.get(path: entry.path))
    }

    try FileManager.default.removeItem(at: phone.container.vaultFolder)
    #expect(phone.vault.status() == .missing)
    // Opening never recreates it empty on its own.
    #expect(phone.vault.status() == .missing)

    try await phone.onboarding.openFromRemote(phone.remote)
    #expect(phone.vault.status() == .ready(vaultId: vaultId, revision: 1))
    #expect(try phone.keeper.deviceKeys(vaultId: vaultId)?.toBytes() == keysBefore)
}

@Test func aMissingVaultIsNotRestoredFromADifferentVault() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await phone.onboarding.commitCreation(pending, enableFaceID: false)
    try FileManager.default.removeItem(at: phone.container.vaultFolder)
    try phone.seedRemote()

    await #expect(throws: Onboarding.Failure.foreignVault) {
        try await phone.onboarding.openFromRemote(phone.remote)
    }
    #expect(phone.vault.status() == .missing)
}

@Test func removingTheVaultLeavesACleanPhoneThatCanStartOver() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await phone.onboarding.commitCreation(pending, enableFaceID: true)
    #expect(phone.quick.status() == .ready)

    try phone.onboarding.removeFromThisPhone()

    #expect(phone.vault.status() == .noVault)
    #expect(phone.secrets.accounts.allSatisfy { $0.hasPrefix("earlier-device.") }, "only the phone's own id stays")
    #expect(phone.quick.status() == .off)
    #expect(!phone.chip.hasKey)
    #expect(phone.onboarding.rememberedVaultId == nil)

    let again = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await phone.onboarding.commitCreation(again, enableFaceID: false)
    if case .ready = phone.vault.status() {} else { Issue.record("a new vault should be stored") }
}

@Test func removingAMissingVaultWorksFromTheRememberedId() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await phone.onboarding.commitCreation(pending, enableFaceID: false)
    try FileManager.default.removeItem(at: phone.container.vaultFolder)
    #expect(phone.vault.status() == .missing)

    try phone.onboarding.removeFromThisPhone()
    #expect(phone.vault.status() == .noVault)
    #expect(phone.secrets.accounts.allSatisfy { $0.hasPrefix("earlier-device.") }, "only the phone's own id stays")
}

// MARK: All or nothing

/// Secrets that refuse to write one kind of item, to break an onboarding half way.
private struct RefusingSecrets: SecretStore {
    let base: MemorySecrets
    let refusing: String

    func read(_ account: String) throws -> Data? { try base.read(account) }
    func delete(_ account: String) throws { try base.delete(account) }

    func write(_ account: String, _ data: Data) throws {
        if account.hasPrefix(refusing) { throw SecretStoreError(status: -25308) }
        try base.write(account, data)
    }
}

private func onboarding(_ phone: Blank, refusing prefix: String) -> Onboarding {
    let keeper = SecretKeyKeeper(
        store: RefusingSecrets(base: phone.secrets, refusing: prefix), container: phone.container)
    return Onboarding(vault: phone.vault, keeper: keeper, quick: phone.quick)
}

@Test func aFailedOpenFromDriveLeavesNoHalfInstalledVault() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    try phone.seedRemote()

    // The vault downloads, then storing the device keys fails.
    await #expect(throws: Onboarding.Failure.failed) {
        try await onboarding(phone, refusing: "device-keys.").openFromRemote(phone.remote)
    }
    #expect(phone.vault.status() == .noVault)
    #expect(phone.secrets.accounts.isEmpty)
    #expect(phone.onboarding.rememberedVaultId == nil)

    // And the phone can simply try again.
    try await phone.onboarding.openFromRemote(phone.remote)
    if case .ready = phone.vault.status() {} else { Issue.record("the second try should work") }
}

@Test func aFailedCreationLeavesNothingBehind() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let failing = onboarding(phone, refusing: "secret-key.")
    let pending = try await failing.startCreation(password: Blank.password, deviceName: "Test iPhone")

    await #expect(throws: Onboarding.Failure.failed) {
        try await failing.commitCreation(pending, enableFaceID: false)
    }
    #expect(phone.vault.status() == .noVault)
    #expect(phone.secrets.accounts.isEmpty)
    #expect(failing.rememberedVaultId == nil)

    // The same pending vault can be stored once the keychain works again.
    try await phone.onboarding.commitCreation(pending, enableFaceID: false)
    if case .ready = phone.vault.status() {} else { Issue.record("the vault should be stored on the retry") }
}

@Test func aMissingVaultWithALostRememberedIdIsNotRestored() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let pending = try await phone.onboarding.startCreation(password: Blank.password, deviceName: "Test iPhone")
    try await phone.onboarding.commitCreation(pending, enableFaceID: false)
    let local = try phone.vault.openStore()
    for entry in try local.list(prefix: "") {
        try phone.remote.putNew(path: entry.path, bytes: try local.get(path: entry.path))
    }
    try FileManager.default.removeItem(at: phone.container.vaultFolder)
    try FileManager.default.removeItem(at: phone.container.deviceFolder.appendingPathComponent("vault-id"))

    await #expect(throws: Onboarding.Failure.foreignVault) {
        try await phone.onboarding.openFromRemote(phone.remote)
    }
    #expect(phone.vault.status() == .missing)
}
