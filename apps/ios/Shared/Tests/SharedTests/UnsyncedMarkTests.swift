import FigLockFFI
import Foundation
import Testing

@testable import Shared

// FIG-222: the unsynced mark follows the remotes this phone uses, not only Google Drive.

@Test func anICloudOnlyPhoneSettlesOnItsFolderRecord() throws {
    let seeded = try SeededVault.make([])
    defer { seeded.cleanup() }
    let folder = RemoteEntry.folder(name: "FigLock", bookmark: Data([1]))
    // No remote at all: nothing has synced.
    #expect(FileSyncState.settledMs(container: seeded.container, remotes: []) == nil)
    // A folder that never finished a sync.
    #expect(FileSyncState.settledMs(container: seeded.container, remotes: [folder]) == nil)
    // The Drive record stays empty on this phone; the folder's record is the one that counts.
    FileSyncState(container: seeded.container, remote: folder).markSynced(nowMs: 2_000)
    #expect(FileSyncState.settledMs(container: seeded.container, remotes: [folder]) == 2_000)
}

@Test func withTwoRemotesTheOlderSyncDecides() throws {
    let seeded = try SeededVault.make([])
    defer { seeded.cleanup() }
    let folder = RemoteEntry.folder(name: "FigLock", bookmark: Data([1]))
    let drive = RemoteEntry.drive
    FileSyncState(container: seeded.container, remote: folder).markSynced(nowMs: 5_000)
    // Drive never synced: an edit has not reached it.
    #expect(FileSyncState.settledMs(container: seeded.container, remotes: [folder, drive]) == nil)
    FileSyncState(container: seeded.container, remote: drive).markSynced(nowMs: 3_000)
    #expect(FileSyncState.settledMs(container: seeded.container, remotes: [folder, drive]) == 3_000)
}

@MainActor
@Test func anEditOnAnICloudOnlyPhoneStopsBeingUnsyncedOnceTheFolderSynced() async throws {
    let seeded = try SeededVault.make([])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()
    let folder = RemoteEntry.folder(name: "FigLock", bookmark: Data([1]))
    let container = seeded.container
    let editor = VaultEditor(
        browser: browser, store: try seeded.vault.openStore(), pending: PendingSync(container: container),
        lastSyncMs: { FileSyncState.settledMs(container: container, remotes: [folder]) },
        now: { SeededVault.tick() })
    var draft = ItemDraft(kind: .note)
    draft.title = "On iCloud"
    let id = try await editor.create(draft)
    #expect(editor.isUnsynced(id))

    FileSyncState(container: container, remote: folder).markSynced(nowMs: UInt64.max / 2)
    editor.refreshUnsynced()
    #expect(!editor.isUnsynced(id))
}
