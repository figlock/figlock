import FigLockFFI
import Foundation
import Testing

@testable import Shared

// MARK: Several remotes, each on its own

/// A phone with a vault and a coordinator over remote folders standing in for iCloud Drive and Drive.
private struct Rig {
    let seeded: SeededVault
    let list: RemoteList
    let coordinator: RemoteSyncCoordinator
    let clock: MutableClock
    let driveFolder: URL
    var driveOn = true

    final class MutableClock: @unchecked Sendable {
        private let lock = NSLock()
        private var value = Date(timeIntervalSince1970: 1_800_000_000)
        var now: Date { lock.withLock { value } }
        func advance(_ seconds: TimeInterval) { lock.withLock { value += seconds } }
    }

    func cleanup() {
        seeded.cleanup()
        try? FileManager.default.removeItem(at: driveFolder)
    }

    static func make(iCloudOn: Bool = true) throws -> Rig {
        let seeded = try SeededVault.make([("note", [("title", "a")])])
        let list = RemoteList(container: seeded.container)
        let drive = try temporaryFolder("drive")
        let store = try FolderStore.open(path: drive.path)
        let clock = MutableClock()
        let coordinator = RemoteSyncCoordinator(
            .init(
                vault: seeded.vault, list: list, drive: { store }, deviceId: { nil }, now: { clock.now },
                iCloudOn: { iCloudOn }))
        return Rig(seeded: seeded, list: list, coordinator: coordinator, clock: clock, driveFolder: drive)
    }

    /// A folder remote over a new folder, added the way Settings does.
    func addFolder() throws -> (entry: RemoteEntry, url: URL) {
        let url = try temporaryFolder("icloud")
        let entry = RemoteEntry.folder(name: url.lastPathComponent, bookmark: try url.bookmarkData())
        list.add(entry)
        return (entry, url)
    }
}

private func phase(_ rig: Rig, _ entry: RemoteEntry) async -> RemoteState.Phase {
    await rig.coordinator.state(of: entry).phase
}

@Test func aVaultReachesAFolderAndDriveAtOnce() async throws {
    let rig = try Rig.make()
    defer { rig.cleanup() }
    let (folder, url) = try rig.addFolder()
    defer { try? FileManager.default.removeItem(at: url) }
    rig.list.add(.drive)

    await rig.coordinator.syncAll(force: true)
    #expect(await phase(rig, folder) == .synced)
    #expect(await phase(rig, .drive) == .synced)
    let local = try rig.seeded.vault.openStore().get(path: "vault.json")
    #expect(try CoordinatedFolderStore(root: url).get(path: "vault.json") == local)
    #expect(try FolderStore.open(path: rig.driveFolder.path).get(path: "vault.json") == local)
    #expect(await rig.coordinator.state(of: folder).lastSyncMs != nil)
}

@Test func oneRemoteFailingNeverBlocksTheOther() async throws {
    let rig = try Rig.make()
    defer { rig.cleanup() }
    let (folder, url) = try rig.addFolder()
    rig.list.add(.drive)
    try FileManager.default.removeItem(at: url)  // the folder is gone

    await rig.coordinator.syncAll(force: true)
    #expect(await phase(rig, folder) == .unavailable(.missing))
    #expect(await phase(rig, .drive) == .synced, "Drive synced although the folder is gone")
    #expect(!FileManager.default.fileExists(atPath: url.path), "a sync never makes the folder again")
    #expect(try FolderStore.open(path: rig.driveFolder.path).get(path: "vault.json").count > 0)
}

@Test func driveSignedOutLeavesTheFolderWorking() async throws {
    let rig = try Rig.make()
    defer { rig.cleanup() }
    let (folder, url) = try rig.addFolder()
    defer { try? FileManager.default.removeItem(at: url) }
    // A coordinator whose Drive has no sign-in.
    let signedOut = RemoteSyncCoordinator(
        .init(vault: rig.seeded.vault, list: rig.list, drive: { nil }, deviceId: { nil }, now: { rig.clock.now }))
    rig.list.add(.drive)

    await signedOut.syncAll(force: true)
    #expect(await signedOut.state(of: .drive).phase == .signInNeeded)
    #expect(await signedOut.state(of: folder).phase == .synced)
}

@Test func eachRemoteBacksOffOnItsOwn() async throws {
    let rig = try Rig.make()
    defer { rig.cleanup() }
    let (folder, url) = try rig.addFolder()
    defer { try? FileManager.default.removeItem(at: url) }
    let (broken, brokenURL) = try rig.addFolder()
    try FileManager.default.removeItem(at: brokenURL)

    await rig.coordinator.syncAll(force: false)
    let failed = await rig.coordinator.state(of: broken)
    #expect(failed.failures == 1)
    let retry = try #require(failed.retryAfter)
    #expect(retry > rig.clock.now)
    #expect(await rig.coordinator.state(of: folder).failures == 0)

    // Not due yet: nothing runs, so the count does not move. A person asking for it does run it.
    await rig.coordinator.syncAll(force: false)
    #expect(await rig.coordinator.state(of: broken).failures == 1)
    await rig.coordinator.sync(broken, force: true, confirmReupload: false)
    #expect(await rig.coordinator.state(of: broken).failures == 2)

    // The folder comes back and the next due run succeeds and clears the back-off.
    try FileManager.default.createDirectory(at: brokenURL, withIntermediateDirectories: true)
    rig.clock.advance(24 * 3600)
    await rig.coordinator.syncAll(force: false)
    let recovered = await rig.coordinator.state(of: broken)
    #expect(recovered.phase == .synced && recovered.failures == 0 && recovered.retryAfter == nil)
    try? FileManager.default.removeItem(at: brokenURL)
}

@Test func aFolderInsideTheLocalVaultIsRefusedEveryTimeItIsOpened() async throws {
    let rig = try Rig.make()
    defer { rig.cleanup() }
    let inside = rig.seeded.container.vaultFolder.appendingPathComponent("log", isDirectory: true)
    try FileManager.default.createDirectory(at: inside, withIntermediateDirectories: true)
    let entry = RemoteEntry.folder(name: "log", bookmark: try inside.bookmarkData())
    rig.list.add(entry)

    await rig.coordinator.syncAll(force: true)
    #expect(await phase(rig, entry) == .unavailable(.insideVault))
}

@Test func anICloudFolderWithICloudOffSaysSoInsteadOfFailing() async throws {
    let rig = try Rig.make(iCloudOn: false)
    defer { rig.cleanup() }
    let root = try temporaryFolder("mobile")
    defer { try? FileManager.default.removeItem(at: root) }
    // The shape of an iCloud Drive path on a phone.
    let icloud = root.appendingPathComponent("Library/Mobile Documents/com~apple~CloudDocs/FigLock", isDirectory: true)
    try FileManager.default.createDirectory(at: icloud, withIntermediateDirectories: true)
    let entry = RemoteEntry.folder(name: "FigLock", bookmark: try icloud.bookmarkData())
    rig.list.add(entry)
    let plain = try rig.addFolder()
    defer { try? FileManager.default.removeItem(at: plain.url) }

    await rig.coordinator.syncAll(force: true)
    #expect(await phase(rig, entry) == .unavailable(.iCloudOff))
    #expect(await phase(rig, plain.entry) == .synced, "a folder outside iCloud does not need iCloud")
}

@Test func aBookmarkThatCannotBeOpenedAsksToPickTheFolderAgain() async throws {
    let rig = try Rig.make()
    defer { rig.cleanup() }
    let entry = RemoteEntry.folder(name: "FigLock", bookmark: Data([0, 1, 2, 3]))
    rig.list.add(entry)
    await rig.coordinator.syncAll(force: true)
    #expect(await phase(rig, entry) == .unavailable(.pickAgain))
}

@Test func aLostRemoteNeedsAYesAndOtherRemotesAreNotAsked() async throws {
    let rig = try Rig.make()
    defer { rig.cleanup() }
    let (lost, url) = try rig.addFolder()
    defer { try? FileManager.default.removeItem(at: url) }
    let (steady, steadyURL) = try rig.addFolder()
    defer { try? FileManager.default.removeItem(at: steadyURL) }
    await rig.coordinator.syncAll(force: true)
    #expect(await phase(rig, lost) == .synced)

    // Someone emptied the folder: it synced before and now holds nothing.
    let store = try CoordinatedFolderStore(root: url)
    for entry in try store.list(prefix: "") { try store.delete(path: entry.path) }

    await rig.coordinator.syncAll(force: true)
    #expect(await phase(rig, lost) == .remoteLost)
    #expect(try store.list(prefix: "").isEmpty, "nothing was uploaded without a yes")
    #expect(await phase(rig, steady) == .synced)

    // A background run leaves it alone; the person's yes uploads again.
    await rig.coordinator.syncAll(force: false)
    #expect(try store.list(prefix: "").isEmpty)
    await rig.coordinator.sync(lost, force: true, confirmReupload: true)
    #expect(await phase(rig, lost) == .synced)
    #expect(try store.get(path: "vault.json").count > 0)
}

@Test func aFolderHoldingAnotherVaultIsNeverMerged() async throws {
    let rig = try Rig.make()
    defer { rig.cleanup() }
    let (entry, url) = try rig.addFolder()
    defer { try? FileManager.default.removeItem(at: url) }
    let other = try createVault(password: "x", deviceName: "Other", kdf: minimumKdfSettings(), nowMs: 1_800_000_000_000)
    let remote = try CoordinatedFolderStore(root: url)
    let foreign = try other.vaultJson()
    try remote.putNew(path: "vault.json", bytes: foreign)

    await rig.coordinator.syncAll(force: true)
    #expect(await phase(rig, entry) == .differentVault)
    #expect(try remote.get(path: "vault.json") == foreign)
    #expect(try remote.list(prefix: "").count == 1, "nothing of this vault was written there")
}

@Test func aSyncPullsWhatAnotherDeviceWroteToAFolder() async throws {
    let rig = try Rig.make()
    defer { rig.cleanup() }
    let (entry, url) = try rig.addFolder()
    defer { try? FileManager.default.removeItem(at: url) }
    await rig.coordinator.syncAll(force: true)

    let remote = try CoordinatedFolderStore(root: url)
    try remote.putNew(path: "log/zz/00000001.op", bytes: Data("from another device".utf8))
    let pulled = await rig.coordinator.sync(entry, force: true, confirmReupload: false)
    #expect(pulled >= 1)
    #expect(try rig.seeded.vault.openStore().get(path: "log/zz/00000001.op") == Data("from another device".utf8))
}
