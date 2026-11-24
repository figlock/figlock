import Shared
import SwiftUI

/// The words and look of each sync state (ADR-0026 table, ADR-0043). One place, so the vault list chip, the Sync
/// screen and the remote's own screen never disagree.
extension RemoteState {
    enum Tone { case good, busy, warn, off }

    var title: String {
        switch phase {
        case .idle: lastSyncMs == nil ? "Not synced yet" : "Synced"
        case .syncing: "Syncing"
        case .synced: "Synced"
        case .unavailable(let problem): problem.title
        default: problemTitle
        }
    }

    private var problemTitle: String {
        switch phase {
        case .offline: "Offline"
        case .signInNeeded: "Sign in again"
        case .waitingForDownload: "Waiting for iCloud"
        case .differentVault: "Different vault"
        case .remoteLost: "Vault not found"
        case .damaged: "Vault file damaged"
        default: "Sync failed"
        }
    }

    func detail(for entry: RemoteEntry, now: Date = Date()) -> String {
        let place = entry.kind == .drive ? "Google Drive" : entry.name
        switch phase {
        case .idle where lastSyncMs == nil: return "Not tried yet."
        case .idle, .synced: return lastSyncMs.map { "Last synced \(Self.when($0, now: now))." } ?? "Synced."
        case .syncing: return "Checking for changes."
        case .unavailable(let problem): return problem.detail(place: place)
        default: return problemDetail(place: place)
        }
    }

    private func problemDetail(place: String) -> String {
        switch phase {
        case .offline: "No connection. Changes save on this iPhone and go up when you are back online."
        case .signInNeeded: "Google needs you to sign in again. Sync with Google Drive is paused."
        case .waitingForDownload:
            "iCloud has not finished downloading files in \(place). FigLock asks for them again shortly."
        case .differentVault: "\(place) holds a different vault than this iPhone. Nothing was merged."
        case .remoteLost: "The copy of your vault in \(place) is gone. Upload this iPhone's copy there again?"
        case .damaged: "The vault file in \(place) cannot be read. Nothing was changed."
        default: "Something went wrong. FigLock tries again shortly."
        }
    }

    var symbol: String {
        switch phase {
        case .idle: "circle.dashed"
        case .syncing: "arrow.triangle.2.circlepath"
        case .synced: "checkmark.circle.fill"
        case .offline: "wifi.slash"
        case .waitingForDownload: "icloud.and.arrow.down"
        default: "exclamationmark.triangle.fill"
        }
    }

    var tone: Tone {
        switch phase {
        case .synced: .good
        case .syncing: .busy
        case .idle, .offline, .waitingForDownload: .off
        default: .warn
        }
    }

    static func when(_ millis: UInt64, now: Date) -> String {
        let date = Date(timeIntervalSince1970: Double(millis) / 1000)
        if now.timeIntervalSince(date) < 60 { return "just now" }
        return date.formatted(.relative(presentation: .named, unitsStyle: .wide))
    }
}

extension RemoteState.Tone {
    var color: Color {
        switch self {
        case .good: FLColor.success
        case .busy: FLColor.textLink
        case .warn: FLColor.warning
        case .off: FLColor.textSecondary
        }
    }

    var background: Color {
        switch self {
        case .good: FLColor.successSubtle
        case .busy: FLColor.primarySubtle
        case .warn: FLColor.warningSubtle
        case .off: FLColor.surfaceSunken
        }
    }
}

extension FolderProblem {
    var title: String {
        switch self {
        case .missing: "Folder not found"
        case .pickAgain: "Pick the folder again"
        case .insideVault: "Folder not allowed"
        case .iCloudOff: "iCloud Drive is off"
        case .denied: "No access"
        default: "Folder not usable"
        }
    }

    func detail(place: String) -> String {
        switch self {
        case .missing:
            "\(place) was moved or deleted. FigLock does not create it again. Pick it again or stop syncing with it."
        case .pickAgain: "iOS no longer lets FigLock open \(place). Pick the folder again."
        case .insideVault: "\(place) overlaps the vault on this iPhone, so it cannot be a sync folder."
        case .iCloudOff: "Turn on iCloud Drive in Settings, under your name, then iCloud. Then sync again."
        case .denied: "The Files app did not let FigLock read \(place)."
        default: "FigLock cannot use \(place) right now."
        }
    }
}

extension RemoteEntry {
    /// The row icon: Drive is a globe, a picked folder (iCloud Drive or any other) a folder.
    var symbol: String { kind == .drive ? "globe" : "folder" }
}

/// What the vault list chip shows.
struct SyncChip {
    let title: String
    let state: RemoteState
    let detail: String
}

extension SyncModel {
    /// The vault list chip: the worst thing first, otherwise the common state.
    var chip: SyncChip? {
        guard !rows.isEmpty else { return nil }
        if let problem = rows.first(where: { $0.state.needsAttention }) {
            return SyncChip(
                title: problem.state.title, state: problem.state,
                detail: problem.state.detail(for: problem.entry))
        }
        if let busy = rows.first(where: { $0.state.phase == .syncing }) {
            return SyncChip(title: busy.state.title, state: busy.state, detail: busy.state.detail(for: busy.entry))
        }
        if let waiting = rows.first(where: { [.offline, .waitingForDownload].contains($0.state.phase) }) {
            return SyncChip(title: "Not synced", state: waiting.state, detail: waiting.state.detail(for: waiting.entry))
        }
        let newest = rows.max { ($0.state.lastSyncMs ?? 0) < ($1.state.lastSyncMs ?? 0) } ?? rows[0]
        var state = newest.state
        state.phase = newest.state.lastSyncMs == nil ? .idle : .synced
        return SyncChip(title: state.title, state: state, detail: state.detail(for: newest.entry))
    }
}
