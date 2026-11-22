import FigLockFFI
import Foundation
import Observation

/// The list and search of the vault screens. Items are read once from the session and kept; a search asks the
/// Rust index (typo tolerant, domain aware) and maps the hits back to items, so no matching runs in Swift.
@MainActor
@Observable
public final class VaultBrowser {
    public enum Filter: String, CaseIterable, Identifiable, Sendable {
        case all
        case login
        case token
        case other

        public var id: String { rawValue }

        public var title: String {
            switch self {
            case .all: "All"
            case .login: "Logins"
            case .token: "API tokens"
            case .other: "Other"
            }
        }

        func matches(_ kind: ItemKind) -> Bool {
            switch self {
            case .all: true
            case .login: kind == .login
            case .token: kind == .token
            case .other: kind != .login && kind != .token
            }
        }
    }

    /// The most hits a search returns; the list shows the best first and a person narrows the query from there.
    public static let searchLimit: UInt32 = 200

    public private(set) var items: [VaultItem] = []
    /// Ids of items that failed to decrypt: shown as damaged, never dropped.
    public private(set) var damagedCount = 0
    public private(set) var loadFailures: [String] = []
    public var query = ""
    public var filter: Filter = .all

    public let session: VaultSession
    private let log: VaultLog?
    private var appliedSeq: [String: UInt64] = [:]
    private var byId: [String: VaultItem] = [:]
    private let now: @Sendable () -> Date
    private var revision = 0
    @ObservationIgnored private var cachedHits: (query: String, revision: Int, ids: [String])?

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
            let listing = try? session.items(includeDeleted: false)
            // Building the index is the slow part of the first search; do it now.
            _ = try? session.search(query: "", nowMs: UInt64(now().timeIntervalSince1970 * 1000), limit: 1)
            return Loaded(applied: applied, failures: failures, listing: listing)
        }.value
        appliedSeq = result.applied
        revision += 1
        if let failures = result.failures { loadFailures = failures }
        guard let listing = result.listing else {
            items = []
            byId = [:]
            return
        }
        let loaded = listing.items.map(VaultItem.init)
        byId = Dictionary(uniqueKeysWithValues: loaded.map { ($0.id, $0) })
        items = loaded.sorted { $0.updated > $1.updated }
        damagedCount = listing.corruptIds.count
    }

    private struct Loaded: @unchecked Sendable {
        let applied: [String: UInt64]
        let failures: [String]?
        let listing: ItemListing?
    }

    public func item(id: String) -> VaultItem? { byId[id] }

    /// What the list shows now: the search hits in score order, or every item newest first, through the filter.
    public var visible: [VaultItem] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        let base: [VaultItem]
        if trimmed.isEmpty {
            base = items
        } else {
            if let cached = cachedHits, cached.query == trimmed, cached.revision == revision {
                base = cached.ids.compactMap { byId[$0] }
            } else {
                let hits =
                    (try? session.search(
                        query: trimmed, nowMs: UInt64(now().timeIntervalSince1970 * 1000), limit: Self.searchLimit))
                    ?? []
                cachedHits = (trimmed, revision, hits.map(\.id))
                base = hits.compactMap { byId[$0.id] }
            }
        }
        return filter == .all ? base : base.filter { filter.matches($0.kind) }
    }

    public var isSearching: Bool { !query.trimmingCharacters(in: .whitespaces).isEmpty }
}
