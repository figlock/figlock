import Foundation

/// The one line under the Vault title that says how syncing stands. It is a pure function of the remotes' phases, so
/// the rules (what wins, what shows for how long) are tested without a screen.
public enum SyncStatusLine: Equatable, Sendable {
    /// A remote has a problem the person has to act on; the first such remote names it.
    case attention
    /// No connection, or iCloud has not delivered the files yet. Stays until it clears.
    case offline
    /// Shown only while a sync runs.
    case syncing
    /// Shown for `syncedVisibleSeconds` after a sync finished, then gone.
    case synced

    /// How long "Synced" stays after a sync finished.
    public static let syncedVisibleSeconds: Double = 3

    /// The line to show, or nil for nothing (and so no space). A vault with no remote has no sync to report.
    public static func resolve(phases: [RemoteState.Phase], isSyncing: Bool, justSynced: Bool) -> SyncStatusLine? {
        guard !phases.isEmpty else { return nil }
        let states = phases.map { RemoteState(phase: $0) }
        if states.contains(where: \.needsAttention) { return .attention }
        if phases.contains(where: { $0 == .offline || $0 == .waitingForDownload }) { return .offline }
        if isSyncing || phases.contains(.syncing) { return .syncing }
        if justSynced { return .synced }
        return nil
    }
}
