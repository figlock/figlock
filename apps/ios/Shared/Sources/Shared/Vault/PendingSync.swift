import Foundation

/// Which items this phone changed that have not reached the other devices yet. Each commit records its items with
/// the time it was written; an item stays pending until a sync finishes after that time, so nothing has to tell
/// this type when a sync ran: it reads the sync state the engine already keeps (`FileSyncState`).
///
/// Holds item ids and times only (no titles, no secrets), in the device folder, never synced.
public final class PendingSync: @unchecked Sendable {
    private let file: URL
    private let lock = NSLock()

    public init(container: VaultContainer) {
        file = container.deviceFolder.appendingPathComponent("pending-sync.json")
    }

    private func load() -> [String: UInt64] {
        guard let data = try? Data(contentsOf: file),
            let saved = try? JSONDecoder().decode([String: UInt64].self, from: data)
        else { return [:] }
        return saved
    }

    private func save(_ entries: [String: UInt64]) {
        guard let data = try? JSONEncoder().encode(entries) else { return }
        try? FileManager.default.createDirectory(
            at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: file, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    /// Remember that `itemIds` were changed at `atMs` (milliseconds since the Unix epoch).
    public func record(_ itemIds: [String], atMs: UInt64) {
        lock.lock()
        defer { lock.unlock() }
        var entries = load()
        for id in itemIds { entries[id] = atMs }
        save(entries)
    }

    /// The items changed after the last sync (or all of them, when this phone never synced). Entries a sync has
    /// covered are dropped from the file.
    public func pendingIds(lastSyncMs: UInt64?) -> Set<String> {
        lock.lock()
        defer { lock.unlock() }
        let entries = load()
        let open = entries.filter { lastSyncMs == nil || $0.value > lastSyncMs! }
        if open.count != entries.count { save(open) }
        return Set(open.keys)
    }
}
