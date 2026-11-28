import FigLockFFI
import Foundation
import Observation

/// The list and search of the vault screens. Items are read once from the session and kept; a search asks the
/// Rust index (typo tolerant, domain aware) and maps the hits back to items, so no matching runs in Swift.
@MainActor
@Observable
public final class VaultBrowser {
    /// The most hits a search returns; the list shows the best first and a person narrows the query from there.
    public static let searchLimit: UInt32 = 200

    public private(set) var items: [VaultItem] = []
    /// Deleted items, most recently changed first, for the "Recently deleted" screen.
    public private(set) var deletedItems: [VaultItem] = []
    /// Ids of items that failed to decrypt: shown as damaged, never dropped.
    public private(set) var damagedCount = 0
    public private(set) var loadFailures: [String] = []
    /// When set, the system's QuickType suggestions follow the list after every load.
    @ObservationIgnored public var identitySync: CredentialIdentitySync?

    public let session: VaultSession
    private let log: VaultLog?
    private var appliedSeq: [String: UInt64] = [:]
    private var byId: [String: VaultItem] = [:]
    private let now: @Sendable () -> Date
    private var revision = 0
    private struct CachedHits {
        let query: String
        let revision: Int
        let ids: [String]
    }

    @ObservationIgnored private var cachedHits: CachedHits?

    public init(session: VaultSession, log: VaultLog?, now: @escaping @Sendable () -> Date = { Date() }) {
        self.session = session
        self.log = log
        self.now = now
    }

    /// Read new op files, then the item list, then build the search index, all off the main thread, so a large
    /// vault does not freeze the screen and the first search is as fast as the next. Cheap to repeat: files
    /// already applied are not read again.
    public func reload() async {
        let session = self.session
        let log = self.log
        let now = self.now
        let applied = appliedSeq
        let result = await Task.detached(priority: .userInitiated) { () -> Loaded in
            var applied = applied
            var failures: [String]?
            if let log, let report = try? log.apply(to: session, applied: &applied, now: now) {
                failures = report.failed
            }
            let listing = try? session.items(includeDeleted: true)
            // Building the index is the slow part of the first search; do it now.
            _ = try? session.search(query: "", nowMs: UInt64(now().timeIntervalSince1970 * 1000), limit: 1)
            return Loaded(applied: applied, failures: failures, listing: listing)
        }.value
        appliedSeq = result.applied
        if let failures = result.failures { loadFailures = failures }
        show(result.listing)
    }

    /// Read the item list again from the session without touching the op files: for after this device's own
    /// edit, whose file is already in the session.
    public func refresh() async {
        let session = self.session
        let now = self.now
        let listing = await Task.detached(priority: .userInitiated) { () -> Listed in
            let listing = try? session.items(includeDeleted: true)
            _ = try? session.search(query: "", nowMs: UInt64(now().timeIntervalSince1970 * 1000), limit: 1)
            return Listed(listing: listing)
        }.value
        show(listing.listing)
    }

    private struct Listed: @unchecked Sendable {
        let listing: ItemListing?
    }

    /// This device wrote `path` (`log/<device>/<seq>.op`) and has it in the session already, so a later `reload`
    /// must not apply it a second time.
    public func didWrite(path: String) {
        guard let (device, seq) = VaultLog.parse(path) else { return }
        appliedSeq[device] = max(appliedSeq[device] ?? 0, seq)
    }

    private func show(_ listing: ItemListing?) {
        revision += 1
        guard let listing else {
            items = []
            deletedItems = []
            byId = [:]
            return
        }
        let loaded = listing.items.map(VaultItem.init)
        byId = Dictionary(uniqueKeysWithValues: loaded.map { ($0.id, $0) })
        items = loaded.filter { !$0.isDeleted }.sorted { $0.updated > $1.updated }
        deletedItems = loaded.filter(\.isDeleted).sorted { $0.updated > $1.updated }
        damagedCount = listing.corruptIds.count
        identitySync?.update(from: items)
    }

    private struct Loaded: @unchecked Sendable {
        let applied: [String: UInt64]
        let failures: [String]?
        let listing: ItemListing?
    }

    public func item(id: String) -> VaultItem? { byId[id] }

    /// What a window's list shows: the search hits in score order, or every live item newest first, through the
    /// scope. The query and scope belong to the window (`VaultNavigation`), so two windows search independently.
    public func rows(query: String, scope: VaultScope) -> [VaultItem] {
        if scope == .deleted { return deletedItems }
        let base = search(query: query) ?? items
        return scope == .all ? base : base.filter(scope.matches)
    }

    /// How many live items carry each tag, by name, for the sidebar.
    public var tagCounts: [(tag: String, count: Int)] {
        var counts: [String: Int] = [:]
        for item in items { for tag in item.tags { counts[tag, default: 0] += 1 } }
        return counts.map { (tag: $0.key, count: $0.value) }.sorted {
            $0.tag.localizedCaseInsensitiveCompare($1.tag) == .orderedAscending
        }
    }

    /// How many items a scope lists, for the sidebar.
    public func count(in scope: VaultScope) -> Int {
        scope == .deleted ? deletedItems.count : items.filter(scope.matches).count
    }

    private func search(query: String) -> [VaultItem]? {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        if let cached = cachedHits, cached.query == trimmed, cached.revision == revision {
            return cached.ids.compactMap { byId[$0] }
        }
        let hits =
            (try? session.search(
                query: trimmed, nowMs: UInt64(now().timeIntervalSince1970 * 1000), limit: Self.searchLimit))
            ?? []
        cachedHits = CachedHits(query: trimmed, revision: revision, ids: hits.map(\.id))
        return hits.compactMap { byId[$0.id] }
    }
}
