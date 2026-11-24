import FigLockFFI
import Foundation

/// The clock for one sync run: it always answers with the moment the run started. The engine stamps the sync
/// state when it finishes, and an edit made while the run was listing files may have missed it, so the state
/// must say when the run began for `PendingSync` to keep such an edit marked as not synced. Make one per run.
public final class SyncRunClock: SyncClock {
    private let startedMs = UInt64(Date().timeIntervalSince1970 * 1000)

    public init() {}

    public func nowMs() -> UInt64 { startedMs }
}

/// The small record a device keeps about its syncing (`SyncStateStore`), in the device folder of the container.
/// Not secret and never synced.
public final class FileSyncState: SyncStateStore, @unchecked Sendable {
    private struct Saved: Codable {
        var syncedBefore = false
        var lastSyncMs: UInt64?
        var signInRefused = false
    }

    private let file: URL
    private let lock = NSLock()

    /// The record of one remote. Each remote keeps its own, so one that is lost or new never changes what another
    /// remembers (ADR-0043). `remote` defaults to Google Drive, whose file is the one earlier versions wrote.
    public init(container: VaultContainer, remote: RemoteEntry = .drive) {
        file = container.deviceFolder.appendingPathComponent(remote.stateFileName)
    }

    /// When every remote of this phone had last finished a sync that started at or after this time: the oldest
    /// of their records. An item changed here has reached the other devices only once all of them have it, so
    /// one remote that never synced (or needs a sign-in) keeps it marked, which is true. `nil` when this phone
    /// has no remote, or any of its remotes never finished a sync.
    public static func settledMs(container: VaultContainer, remotes: [RemoteEntry]) -> UInt64? {
        var oldest: UInt64?
        for remote in remotes {
            guard let synced = FileSyncState(container: container, remote: remote).lastSyncMs() else { return nil }
            oldest = min(oldest ?? synced, synced)
        }
        return oldest
    }

    private func load() -> Saved {
        guard let data = try? Data(contentsOf: file), let saved = try? JSONDecoder().decode(Saved.self, from: data)
        else { return Saved() }
        return saved
    }

    private func update(_ change: (inout Saved) -> Void) {
        lock.lock()
        defer { lock.unlock() }
        var saved = load()
        change(&saved)
        guard let data = try? JSONEncoder().encode(saved) else { return }
        try? FileManager.default.createDirectory(
            at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: file, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    private func read() -> Saved {
        lock.lock()
        defer { lock.unlock() }
        return load()
    }

    public func syncedBefore() -> Bool { read().syncedBefore }
    public func lastSyncMs() -> UInt64? { read().lastSyncMs }
    public func signInRefused() -> Bool { read().signInRefused }

    public func markSynced(nowMs: UInt64) {
        update {
            $0.syncedBefore = true
            $0.lastSyncMs = nowMs
        }
    }

    public func markSignInRefused() { update { $0.signInRefused = true } }
    public func clearSignInRefused() { update { $0.signInRefused = false } }
    public func forget() { update { $0 = Saved() } }
}
