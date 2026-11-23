import FigLockFFI
import Foundation
import Testing

@testable import Shared

func temporaryFolder(_ name: String = "folder") throws -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("figlock-\(name)-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

func hidden(_ url: URL, _ name: String) -> URL { url.appendingPathComponent(name) }

// MARK: The remote list

@Test func remotesAreKeptPerPhoneAndEachHasItsOwnSyncRecord() throws {
    let root = try temporaryFolder("phone")
    defer { try? FileManager.default.removeItem(at: root) }
    let container = VaultContainer(root: root)
    try container.prepare()
    let list = RemoteList(container: container)
    #expect(list.entries().isEmpty)

    let folder = RemoteEntry.folder(name: "FigLock", bookmark: Data([1, 2, 3]))
    list.add(.drive)
    list.add(folder)
    #expect(list.entries().map(\.id) == ["drive", folder.id])
    #expect(RemoteList(container: container).entry(id: folder.id)?.bookmark == Data([1, 2, 3]))

    FileSyncState(container: container, remote: .drive).markSynced(nowMs: 10)
    FileSyncState(container: container, remote: folder).markSynced(nowMs: 20)
    #expect(FileSyncState(container: container, remote: .drive).lastSyncMs() == 10)
    #expect(FileSyncState(container: container, remote: folder).lastSyncMs() == 20)
    // The file Drive always used stays Drive's, so a phone from before keeps its record.
    #expect(FileSyncState(container: container).lastSyncMs() == 10)

    list.remove(id: folder.id)
    #expect(list.entries().map(\.id) == ["drive"])
    #expect(FileSyncState(container: container, remote: folder).lastSyncMs() == nil, "its record went with it")
    #expect(FileSyncState(container: container, remote: .drive).lastSyncMs() == 10, "another remote is untouched")
}

@Test func signingOutOfDriveForgetsItsRecordButNotAFolders() throws {
    let root = try temporaryFolder("phone")
    defer { try? FileManager.default.removeItem(at: root) }
    let container = VaultContainer(root: root)
    try container.prepare()
    let list = RemoteList(container: container)
    let folder = RemoteEntry.folder(name: "FigLock", bookmark: Data())
    // A phone from before the list existed: signed in, with a record and no remotes.json.
    FileSyncState(container: container).markSynced(nowMs: 5)
    FileSyncState(container: container, remote: folder).markSynced(nowMs: 6)
    list.add(folder)

    list.reconcileDrive(signedIn: true)
    #expect(list.entries().map(\.id) == [folder.id, "drive"])
    #expect(FileSyncState(container: container).lastSyncMs() == 5)

    list.reconcileDrive(signedIn: false)
    #expect(list.entries().map(\.id) == [folder.id])
    #expect(FileSyncState(container: container).lastSyncMs() == nil, "a different account starts fresh")
    #expect(FileSyncState(container: container, remote: folder).lastSyncMs() == 6)
}

// MARK: The coordinated folder store

@Test func theFolderStoreFollowsTheStorageContract() throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let store = try CoordinatedFolderStore(root: root)

    #expect(throws: StoreFailure.NotFound) { try store.get(path: "vault.json") }
    try store.putNew(path: "vault.json", bytes: Data("one".utf8))
    #expect(throws: StoreFailure.Exists) { try store.putNew(path: "vault.json", bytes: Data("two".utf8)) }
    #expect(try store.get(path: "vault.json") == Data("one".utf8), "create only never overwrites")
    try store.putReplace(path: "vault.json", bytes: Data("two".utf8))
    #expect(try store.get(path: "vault.json") == Data("two".utf8))
    try store.putReplace(path: "devices/aa.json", bytes: Data("new".utf8))

    try store.putNew(path: "log/ab/00000001.op", bytes: Data("op".utf8))
    try store.putNew(path: "log/ab/00000002.op", bytes: Data("op".utf8))
    try store.putNew(path: "snap/ab12-1", bytes: Data("s".utf8))
    #expect(try store.list(prefix: "log/").map(\.path) == ["log/ab/00000001.op", "log/ab/00000002.op"])
    #expect(try store.list(prefix: "log/ab/").count == 2)
    #expect(try store.list(prefix: "snap/ab12-").map(\.path) == ["snap/ab12-1"])
    let everything = try store.list(prefix: "").map(\.path)
    #expect(everything == everything.sorted())
    #expect(try store.list(prefix: "nothing/").isEmpty)
    #expect(try store.list(prefix: "vault.json").map(\.path) == ["vault.json"])
    #expect(try store.list(prefix: "").allSatisfy(\.available))

    try store.delete(path: "log/ab/00000001.op")
    try store.delete(path: "log/ab/00000001.op")  // a missing path succeeds
    #expect(try store.list(prefix: "log/").count == 1)
}

@Test func theFolderStoreRefusesPathsOutsideTheRules() throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let store = try CoordinatedFolderStore(root: root)
    for path in ["../escape", "/abs", "a//b", ".hidden", "UPPER", "a/b/c/d/e", "", "log/.tmp", "sp ace"] {
        #expect(throws: StoreFailure.InvalidPath, "\(path)") { try store.putNew(path: path, bytes: Data()) }
        #expect(throws: StoreFailure.InvalidPath, "\(path)") { _ = try store.get(path: path) }
        #expect(throws: StoreFailure.InvalidPath, "\(path)") { try store.delete(path: path) }
    }
    #expect(throws: StoreFailure.InvalidPath) { _ = try store.list(prefix: "../") }
    #expect(throws: StoreFailure.TooLarge) {
        try store.putNew(path: "big", bytes: Data(count: 64 * 1024 * 1024 + 1))
    }
}

@Test func theFolderStoreNeverCreatesItsRoot() throws {
    let root = try temporaryFolder()
    let store = try CoordinatedFolderStore(root: root)
    try FileManager.default.removeItem(at: root)
    #expect(throws: StoreFailure.Other) { try store.putNew(path: "vault.json", bytes: Data()) }
    #expect(throws: StoreFailure.Other) { try store.putReplace(path: "vault.json", bytes: Data()) }
    #expect(throws: StoreFailure.Other) { _ = try store.list(prefix: "") }
    #expect(!FileManager.default.fileExists(atPath: root.path), "a missing folder is never made again")
    #expect(throws: CoordinatedFolderStore.SetupError.notAFolder) { _ = try CoordinatedFolderStore(root: root) }
}

@Test func anEvictedFileIsNotDownloadedYetNeverMissingOrAGap() throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let store = try CoordinatedFolderStore(root: root)
    try store.putNew(path: "log/ab/00000001.op", bytes: Data("op".utf8))
    // The hidden placeholder an evicted file leaves on older systems.
    try FileManager.default.createDirectory(
        at: root.appendingPathComponent("log/ab"), withIntermediateDirectories: true)
    try Data().write(to: root.appendingPathComponent(".vault.json.icloud"))
    try Data().write(to: root.appendingPathComponent("log/ab/.00000002.op.icloud"))

    let listed = try store.list(prefix: "")
    let byPath = Dictionary(uniqueKeysWithValues: listed.map { ($0.path, $0) })
    #expect(Set(byPath.keys) == ["vault.json", "log/ab/00000001.op", "log/ab/00000002.op"])
    #expect(byPath["vault.json"]?.available == false)
    #expect(byPath["log/ab/00000002.op"]?.available == false)
    #expect(byPath["log/ab/00000001.op"]?.available == true)
    #expect(throws: StoreFailure.NotYetAvailable) { _ = try store.get(path: "vault.json") }
    #expect(throws: StoreFailure.Exists, "an evicted file is not overwritten by a create") {
        try store.putNew(path: "vault.json", bytes: Data())
    }
    try store.delete(path: "vault.json")
    #expect(try store.list(prefix: "").map(\.path) == ["log/ab/00000001.op", "log/ab/00000002.op"])
}

@Test func temporaryFilesAndOtherAppsFilesAreNotPartOfTheVault() throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let store = try CoordinatedFolderStore(root: root)
    try store.putNew(path: "vault.json", bytes: Data("v".utf8))
    try Data().write(to: hidden(root, ".DS_Store"))
    try Data().write(to: hidden(root, ".tmp-1234"))
    try Data().write(to: hidden(root, "Not A Vault File.txt"))
    #expect(try store.list(prefix: "").map(\.path) == ["vault.json"])
}

// MARK: The folder checks of ADR-0043

@Test func aFolderMayNotBeContainOrSitInsideTheLocalVault() throws {
    let phone = try temporaryFolder("phone")
    defer { try? FileManager.default.removeItem(at: phone) }
    let vault = phone.appendingPathComponent("vault", isDirectory: true)
    try FileManager.default.createDirectory(at: vault.appendingPathComponent("log"), withIntermediateDirectories: true)

    #expect(throws: FolderProblem.insideVault, "the vault folder itself") {
        try FolderRemote.check(folder: vault, localVault: vault)
    }
    #expect(throws: FolderProblem.insideVault, "inside it") {
        try FolderRemote.check(folder: vault.appendingPathComponent("log"), localVault: vault)
    }
    #expect(throws: FolderProblem.insideVault, "containing it") {
        try FolderRemote.check(folder: phone, localVault: vault)
    }
    // A symbolic link into the vault is the same folder.
    let link = phone.appendingPathComponent("link")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: vault)
    #expect(throws: FolderProblem.insideVault, "through a symbolic link") {
        try FolderRemote.check(folder: link, localVault: vault)
    }
    let other = try temporaryFolder("icloud")
    defer { try? FileManager.default.removeItem(at: other) }
    try FolderRemote.check(folder: other, localVault: vault)
    // A folder whose name only starts like the vault's is a different folder.
    let sibling = phone.appendingPathComponent("vault-2", isDirectory: true)
    try FileManager.default.createDirectory(at: sibling, withIntermediateDirectories: true)
    try FolderRemote.check(folder: sibling, localVault: vault)

    #expect(throws: FolderProblem.missing) {
        try FolderRemote.check(folder: phone.appendingPathComponent("gone"), localVault: vault)
    }
}

@Test func anExistingVaultGoesOnlyToAnEmptyFolderOrItsOwn() async throws {
    let seeded = try SeededVault.make([("note", [("title", "a")])])
    defer { seeded.cleanup() }
    guard case .ready(let vaultId, _) = seeded.vault.status() else {
        Issue.record("no vault")
        return
    }

    let empty = try temporaryFolder()
    let busy = try temporaryFolder()
    let same = try temporaryFolder()
    let foreign = try temporaryFolder()
    defer { for url in [empty, busy, same, foreign] { try? FileManager.default.removeItem(at: url) } }

    try FolderRemote.checkCanHold(vaultId: vaultId, in: CoordinatedFolderStore(root: empty))

    try Data("x".utf8).write(to: busy.appendingPathComponent("holiday-photo.jpg"))
    #expect(throws: FolderProblem.notEmpty, "a folder of other files is refused") {
        try FolderRemote.checkCanHold(vaultId: vaultId, in: CoordinatedFolderStore(root: busy))
    }

    let local = try seeded.vault.openStore()
    try CoordinatedFolderStore(root: same).putNew(path: "vault.json", bytes: local.get(path: "vault.json"))
    try FolderRemote.checkCanHold(vaultId: vaultId, in: CoordinatedFolderStore(root: same))

    let other = try createVault(password: "x", deviceName: "Other", kdf: minimumKdfSettings(), nowMs: 1_800_000_000_000)
    try CoordinatedFolderStore(root: foreign).putNew(path: "vault.json", bytes: other.vaultJson())
    #expect(throws: FolderProblem.differentVault) {
        try FolderRemote.checkCanHold(vaultId: vaultId, in: CoordinatedFolderStore(root: foreign))
    }
}
