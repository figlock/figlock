import FigLockFFI
import Foundation
import Testing

@testable import Shared

// MARK: An iCloud Drive folder as the remote (ADR-0043)

@MainActor
@Test func aVaultOpenedFromAFolderKeepsTheFolderAsItsOwnRemoteWithItsOwnRecord() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    try phone.seedRemote()
    let folder = try CoordinatedFolderStore(root: phone.remoteFolder)
    let entry = RemoteEntry.folder(name: "FigLock", bookmark: Data([9]))

    try await phone.onboarding.openFromRemote(folder, entry: entry)

    #expect(phone.vault.status() != .noVault)
    let list = RemoteList(container: phone.container)
    #expect(list.entries().map(\.id) == [entry.id], "the folder is the remote, Drive is not")
    #expect(FileSyncState(container: phone.container, remote: entry).syncedBefore())
    #expect(!FileSyncState(container: phone.container, remote: .drive).syncedBefore())
}

@MainActor
@Test func aFolderWithoutAVaultIsNotOpenedAndNothingIsWrittenToIt() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    let folder = try CoordinatedFolderStore(root: phone.remoteFolder)
    await #expect(throws: Onboarding.Failure.noVaultInStorage) {
        try await phone.onboarding.openFromRemote(folder, entry: .folder(name: "x", bookmark: Data()))
    }
    #expect(try folder.list(prefix: "").isEmpty)
    #expect(RemoteList(container: phone.container).entries().isEmpty)
    #expect(phone.vault.status() == .noVault)
}

@MainActor
@Test func removingTheVaultForgetsEveryRemoteAndItsRecordButNotTheirCopies() async throws {
    let phone = try Blank.make()
    defer { phone.cleanup() }
    try phone.seedRemote()
    let entry = RemoteEntry.folder(name: "FigLock", bookmark: Data([9]))
    try await phone.onboarding.openFromRemote(CoordinatedFolderStore(root: phone.remoteFolder), entry: entry)
    RemoteList(container: phone.container).add(.drive)
    FileSyncState(container: phone.container, remote: .drive).markSynced(nowMs: 1)

    try phone.onboarding.removeFromThisPhone()

    #expect(RemoteList(container: phone.container).entries().isEmpty)
    #expect(!FileSyncState(container: phone.container, remote: entry).syncedBefore())
    #expect(!FileSyncState(container: phone.container, remote: .drive).syncedBefore())
    #expect(try CoordinatedFolderStore(root: phone.remoteFolder).get(path: "vault.json").count > 0)
}

// MARK: Names of picked folders (FIG-217)

@Test func aFilesLocationRootIsNamedForPeopleNotForTheSystem() {
    let root = URL(fileURLWithPath: "/private/var/mobile/Containers/Shared/AppGroup/ABC/File Provider Storage")
    #expect(FolderRemote.displayName(for: root) == "Files folder")
    #expect(FolderRemote.displayName(for: URL(fileURLWithPath: "/")) == "Files folder")
}

@Test func aFolderThePersonNamedKeepsItsName() {
    let folder = URL(fileURLWithPath: "/private/var/mobile/Mobile Documents/com~apple~CloudDocs/FigLock")
    #expect(FolderRemote.displayName(for: folder) == "FigLock")
}
