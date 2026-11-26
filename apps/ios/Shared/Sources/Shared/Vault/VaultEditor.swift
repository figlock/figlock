import FigLockFFI
import Foundation
import Observation

/// Why a save did not happen. Neither case says anything about the content, so a message built from it cannot
/// leak a secret.
public enum EditFailure: Error, Equatable {
    /// The core refused the change (a value over its limit, too many fields, a field the item type does not have).
    case rejected
    /// The change could not be written to this phone's vault folder. Nothing was changed.
    case storage
    /// The item is no longer in the vault.
    case missing

    public var message: String {
        switch self {
        case .rejected: "FigLock could not save this item. A value may be too long, or the item has too many fields."
        case .storage: "FigLock could not write to its storage on this phone. Nothing was changed. Try again."
        case .missing: "This item is no longer in the vault."
        }
    }
}

/// Creates, edits, deletes and restores items. Every change goes through the core (`VaultSession`), is committed
/// as one op file and written to the local vault folder, where the sync engine picks it up. The item then shows
/// as not synced until a sync has finished (`PendingSync`).
@MainActor
@Observable
public final class VaultEditor {
    /// The vault name items are created in. Vaults beyond the first are not offered on the phone yet.
    public static let vaultName = "personal"

    public private(set) var unsyncedIds: Set<String> = []

    private let browser: VaultBrowser
    private let store: any BlobStoreFfi
    private let pending: PendingSync
    private let lastSyncMs: @Sendable () -> UInt64?
    private let now: @Sendable () -> Date

    public init(
        browser: VaultBrowser, store: any BlobStoreFfi, pending: PendingSync,
        lastSyncMs: @escaping @Sendable () -> UInt64? = { nil }, now: @escaping @Sendable () -> Date = { Date() }
    ) {
        self.browser = browser
        self.store = store
        self.pending = pending
        self.lastSyncMs = lastSyncMs
        self.now = now
        refreshUnsynced()
    }

    /// Whether `id` was changed on this phone and has not been synced yet.
    public func isUnsynced(_ id: String) -> Bool { unsyncedIds.contains(id) }

    /// Read the pending state again, after a sync ran.
    public func refreshUnsynced() {
        unsyncedIds = pending.pendingIds(lastSyncMs: lastSyncMs())
    }

    private var nowMs: UInt64 { UInt64(now().timeIntervalSince1970 * 1000) }

    /// Add the item. Returns its id.
    @discardableResult
    public func create(_ draft: ItemDraft) async throws -> String {
        let fields = draft.newItemFields()
        let id = try await perform { session, stamp in
            [try session.addItem(itemType: draft.kind.wire, vault: Self.vaultName, fields: fields, nowMs: stamp)]
        }
        return id[0]
    }

    /// Write what changed in `draft` against the item it was made from. Nothing is written when nothing changed.
    public func update(_ draft: ItemDraft) async throws {
        guard let original = draft.original else { throw EditFailure.missing }
        let changes = draft.changes()
        guard !changes.isEmpty else { return }
        _ = try await perform { session, stamp in
            for change in changes {
                try session.setField(itemId: original.id, field: change.name, value: change.value, nowMs: stamp)
            }
            return [original.id]
        }
    }

    public func delete(id: String) async throws {
        _ = try await perform { session, stamp in
            try session.deleteItem(itemId: id, nowMs: stamp)
            return [id]
        }
    }

    public func restore(id: String) async throws {
        _ = try await perform { session, stamp in
            try session.restoreItem(itemId: id, nowMs: stamp)
            return [id]
        }
    }

    /// Restore every item in `ids` in one commit, so "Recover all" is one write and one sync.
    public func restoreAll(ids: [String]) async throws {
        guard !ids.isEmpty else { return }
        _ = try await perform { session, stamp in
            for id in ids { try session.restoreItem(itemId: id, nowMs: stamp) }
            return ids
        }
    }

    /// Run the edits, commit them as one op file and store it. A failure at any step leaves the session as it was.
    private func perform(_ work: (VaultSession, UInt64) throws -> [String]) async throws -> [String] {
        let session = browser.session
        let stamp = nowMs
        let ids: [String]
        do {
            ids = try work(session, stamp)
        } catch {
            try? session.discardPending()
            throw EditFailure.rejected
        }
        let file: CommittedFile?
        do {
            file = try session.commit()
        } catch {
            try? session.discardPending()
            throw EditFailure.rejected
        }
        guard let file else { return ids }
        do {
            try store.putNew(path: file.path, bytes: file.bytes)
        } catch {
            // The file is not stored: take the commit back and drop the edits, so the list shows what is stored.
            try? session.rollbackCommit()
            try? session.discardPending()
            throw EditFailure.storage
        }
        try? session.confirmCommit()
        browser.didWrite(path: file.path)
        pending.record(ids, atMs: stamp)
        refreshUnsynced()
        await browser.refresh()
        return ids
    }
}
