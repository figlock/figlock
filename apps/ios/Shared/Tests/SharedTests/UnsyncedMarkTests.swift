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

// FIG-261: a save is pushed at once, and a burst of saves does not become a burst of syncs.

private final class Pauses: @unchecked Sendable {
    private let lock = NSLock()
    private var seen: [Duration] = []
    func add(_ pause: Duration) { lock.withLock { seen.append(pause) } }
    var all: [Duration] { lock.withLock { seen } }
}

@MainActor
private final class Runs {
    var count = 0
    var scheduler: PushScheduler?
}

@MainActor
@Test func theFirstWriteRunsAtOnceAndLaterOnesShareOneRunAfterTheGap() async {
    let runs = Runs()
    let pauses = Pauses()
    let scheduler = PushScheduler(
        minGap: .seconds(2), pause: { pauses.add($0) },
        run: {
            runs.count += 1
            // Writes land while the first run is going: they are one more run, not three.
            if runs.count == 1 { for _ in 0..<3 { runs.scheduler?.noteWrite() } }
        })
    runs.scheduler = scheduler
    #expect(!scheduler.isBusy)
    scheduler.noteWrite()
    #expect(scheduler.isBusy)
    await scheduler.finish()
    #expect(runs.count == 2)
    #expect(pauses.all == [.seconds(2)])
    #expect(!scheduler.isBusy)
}

@MainActor
@Test func nothingRunsWithoutAWriteAndASingleWriteIsASingleRun() async {
    let runs = Runs()
    let scheduler = PushScheduler(pause: { _ in }, run: { runs.count += 1 })
    await scheduler.finish()
    #expect(runs.count == 0)
    scheduler.noteWrite()
    await scheduler.finish()
    #expect(runs.count == 1)
    // Idle again: a later write starts a fresh run.
    scheduler.noteWrite()
    await scheduler.finish()
    #expect(runs.count == 2)
}

@MainActor
@Test func everyEditThatReachesTheFolderNotesAWriteAndNothingElseDoes() async throws {
    let seeded = try SeededVault.make([])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()
    let runs = Runs()
    let editor = VaultEditor(
        browser: browser, store: try seeded.vault.openStore(), pending: PendingSync(container: seeded.container),
        lastSyncMs: { nil }, now: { SeededVault.tick() }, didWrite: { runs.count += 1 })
    // Nothing to restore: nothing written, nothing to push.
    try await editor.restoreAll(ids: [])
    #expect(runs.count == 0)
    var draft = ItemDraft(kind: .login)
    draft.title = "Fastmail"
    draft.values["password"] = "generated-pass-1"
    let id = try await editor.create(draft)
    #expect(runs.count == 1)
    try await editor.delete(id: id)
    #expect(runs.count == 2)
}
