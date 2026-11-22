import FigLockFFI
import Foundation
import Testing

@testable import Shared

/// A container in a temporary folder, removed afterwards. Every value in these tests is generated.
private func makeContainer() throws -> (VaultContainer, () -> Void) {
    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent("figlock-test-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    return (VaultContainer(root: root), { try? FileManager.default.removeItem(at: root) })
}

private func vaultJson(id: String = "v1", revision: Int = 1) -> Data {
    Data("{\"vault_id\":\"\(id)\",\"revision\":\(revision)}".utf8)
}

private func isExcludedFromBackup(_ url: URL) throws -> Bool {
    try url.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup ?? false
}

@Test func aFreshPhoneHasNoVaultAndTheFoldersAreExcludedFromBackup() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()

    #expect(LocalVault(container: container, access: .readWrite).status() == .noVault)
    #expect(try isExcludedFromBackup(container.vaultFolder))
    #expect(try isExcludedFromBackup(container.deviceFolder))
}

@Test func theFoldersGetTheProtectionClassTheExtensionNeeds() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let vault = LocalVault(container: container, access: .readWrite)
    try vault.openStore().putReplace(path: "vault.json", bytes: vaultJson())

    for url in [container.vaultFolder, container.deviceFolder] {
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        // The simulator does not report a protection class at all; a device must report ours. The device case
        // is covered by the on-device run of FIG-172.
        if let actual = attributes[.protectionKey] as? FileProtectionType {
            #expect(actual == VaultContainer.protection, "\(url.lastPathComponent) protection")
        }
    }
}

@Test func preparingAgainChangesNothing() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let vault = LocalVault(container: container, access: .readWrite)
    try vault.openStore().putReplace(path: "vault.json", bytes: vaultJson())
    try vault.markVaultPresent()
    try container.prepare()
    #expect(vault.status() == .ready(vaultId: "v1", revision: 1))
    #expect(try isExcludedFromBackup(container.vaultFolder))
}

@Test func theAppWritesAndTheExtensionReadsTheSameVault() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let app = try LocalVault(container: container, access: .readWrite).openStore()
    try app.putReplace(path: "vault.json", bytes: vaultJson(revision: 3))
    try app.putNew(path: "devices/aa.json", bytes: Data("record".utf8))
    try app.putNew(path: "log/aa/00000001.op", bytes: Data("op".utf8))

    let extensionSide = LocalVault(container: container, access: .readOnly)
    let reader = try extensionSide.openStore()
    #expect(try reader.get(path: "devices/aa.json") == Data("record".utf8))
    #expect(try reader.list(prefix: "log/").map(\.path) == ["log/aa/00000001.op"])
    #expect(extensionSide.status() == .ready(vaultId: "v1", revision: 3))
}

@Test func theExtensionCannotWriteOrDelete() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let app = try LocalVault(container: container, access: .readWrite).openStore()
    try app.putReplace(path: "vault.json", bytes: vaultJson())

    let vault = LocalVault(container: container, access: .readOnly)
    let reader = try vault.openStore()
    #expect(throws: LocalVault.StoreError.readOnly) { try reader.putNew(path: "log/x", bytes: Data()) }
    #expect(throws: LocalVault.StoreError.readOnly) { try reader.putReplace(path: "vault.json", bytes: Data()) }
    #expect(throws: LocalVault.StoreError.readOnly) { try reader.delete(path: "vault.json") }
    #expect(throws: LocalVault.StoreError.readOnly) { try vault.markVaultPresent() }
    #expect(try app.get(path: "vault.json") == vaultJson(), "nothing changed")
}

@Test func writesAreAtomicAndCreateOnlyFilesAreNeverOverwritten() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let store = try LocalVault(container: container, access: .readWrite).openStore()
    try store.putNew(path: "log/aa/00000001.op", bytes: Data("first".utf8))
    #expect(throws: StoreFailure.Exists) {
        try store.putNew(path: "log/aa/00000001.op", bytes: Data("second".utf8))
    }
    #expect(try store.get(path: "log/aa/00000001.op") == Data("first".utf8))

    // Many writers racing for one name: exactly one wins and nobody ever reads a partial file.
    let winners = LockedCounter()
    DispatchQueue.concurrentPerform(iterations: 32) { index in
        if (try? store.putNew(path: "log/aa/00000002.op", bytes: Data("writer \(index)".utf8))) != nil {
            winners.increment()
        }
    }
    #expect(winners.value == 1)
    let text = try String(bytes: store.get(path: "log/aa/00000002.op"), encoding: .utf8) ?? ""
    #expect(text.hasPrefix("writer "))

    // No temporary file is left behind and listings never show one.
    let names = try store.list(prefix: "").map(\.path)
    #expect(names == ["log/aa/00000001.op", "log/aa/00000002.op"])
}

@Test func pathsOutsideTheVaultAreRefused() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let store = try LocalVault(container: container, access: .readWrite).openStore()
    for bad in ["../device/vault-present", "/etc/hosts", "log/../vault.json", "Log/A", ".hidden", ""] {
        #expect(throws: StoreFailure.InvalidPath, "\(bad)") { _ = try store.get(path: bad) }
        #expect(throws: StoreFailure.InvalidPath, "\(bad)") { try store.putReplace(path: bad, bytes: Data()) }
    }
}

@Test func aFolderThatWentMissingIsTheRecoveryCaseAndIsNeverRecreated() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let vault = LocalVault(container: container, access: .readWrite)
    try vault.openStore().putReplace(path: "vault.json", bytes: vaultJson())
    try vault.markVaultPresent()
    #expect(vault.status() == .ready(vaultId: "v1", revision: 1))

    try FileManager.default.removeItem(at: container.vaultFolder)
    #expect(vault.status() == .missing)
    #expect(throws: LocalVault.StoreError.missing) { _ = try vault.openStore() }
    try container.prepare()
    #expect(!FileManager.default.fileExists(atPath: container.vaultFolder.path), "prepare keeps it gone")
    #expect(throws: LocalVault.StoreError.missing) {
        _ = try LocalVault(container: container, access: .readOnly).openStore()
    }

    // The person chose to start over: forgetting the vault makes the phone a fresh one again.
    try vault.forgetVault()
    #expect(vault.status() == .noVault)
    try container.prepare()
    #expect(try vault.openStore().list(prefix: "").isEmpty)
}

@Test func aVaultJsonThatWentMissingFromAnExistingFolderIsAlsoMissing() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let vault = LocalVault(container: container, access: .readWrite)
    let store = try vault.openStore()
    try store.putReplace(path: "vault.json", bytes: vaultJson())
    try vault.markVaultPresent()
    try store.delete(path: "vault.json")
    #expect(vault.status() == .missing)
}

@Test func aReadOnlyHandleNeverCreatesTheFolder() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    let extensionSide = LocalVault(container: container, access: .readOnly)
    #expect(extensionSide.status() == .noVault)
    #expect(throws: LocalVault.StoreError.missing) { _ = try extensionSide.openStore() }
    #expect(!FileManager.default.fileExists(atPath: container.vaultFolder.path))
}

@Test func aDamagedVaultJsonIsReportedAndLeftAlone() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let vault = LocalVault(container: container, access: .readWrite)
    let store = try vault.openStore()
    try vault.markVaultPresent()

    for broken in ["", "{ truncated", "{}", "{\"vault_id\":\"v1\"}", "[1,2]", "\u{0}\u{1}\u{2}"] {
        try store.putReplace(path: "vault.json", bytes: Data(broken.utf8))
        #expect(vault.status() == .damaged, "\(broken.debugDescription)")
        #expect(try store.get(path: "vault.json") == Data(broken.utf8), "the file is not touched")
    }
    try store.putReplace(path: "vault.json", bytes: vaultJson(id: "v2", revision: 9))
    #expect(vault.status() == .ready(vaultId: "v2", revision: 9))
}

@Test func aVaultJsonThatCannotBeReadIsUnreadableNotDamaged() throws {
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    try container.prepare()
    let vault = LocalVault(container: container, access: .readWrite)
    try vault.openStore().putReplace(path: "vault.json", bytes: vaultJson())
    let file = container.vaultFolder.appendingPathComponent("vault.json")
    try FileManager.default.setAttributes([.posixPermissions: 0], ofItemAtPath: file.path)
    defer { try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path) }
    #expect(vault.status() == .unreadable)
}

@Test func anUnavailableAppGroupIsReportedNotInvented() throws {
    #expect(throws: VaultContainer.SetupError.containerUnavailable) {
        _ = try VaultContainer.appGroup("group.com.figlock.none", locate: { _ in nil })
    }
    let (container, cleanup) = try makeContainer()
    defer { cleanup() }
    let found = try VaultContainer.appGroup("group.com.figlock.ios", locate: { _ in container.root })
    #expect(found.vaultFolder == container.root.appendingPathComponent("vault", isDirectory: true))
}

private final class LockedCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    var value: Int { lock.withLock { count } }
    func increment() { lock.withLock { count += 1 } }
}
