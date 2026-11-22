import FigLockFFI
import Foundation

/// The wall clock for the sync engine.
public final class SystemClock: SyncClock {
    public init() {}

    public func nowMs() -> UInt64 {
        UInt64(Date().timeIntervalSince1970 * 1000)
    }
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

    public init(container: VaultContainer) {
        file = container.deviceFolder.appendingPathComponent("sync-state.json")
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
