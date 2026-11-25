import Foundation

/// One place this phone mirrors its vault to (ADR-0043): Google Drive, or a folder the person picked in the Files
/// picker, typically in iCloud Drive. The local vault in the App Group container stays the working copy.
public struct RemoteEntry: Codable, Equatable, Sendable, Identifiable {
    public enum Kind: String, Codable, Sendable {
        case drive
        case folder
    }

    /// `drive`, or `folder-<uuid>`. Names the sync state file, so it never changes for an entry.
    public let id: String
    public let kind: Kind
    /// What the rows show: "Google Drive", or the folder's name.
    public var name: String
    /// The security-scoped bookmark of a folder, reopened on every launch. Nil for Drive.
    public var bookmark: Data?

    public static let driveID = "drive"

    public static var drive: RemoteEntry { RemoteEntry(id: driveID, kind: .drive, name: "Google Drive", bookmark: nil) }

    public static func folder(name: String, bookmark: Data) -> RemoteEntry {
        RemoteEntry(id: "folder-\(UUID().uuidString.lowercased())", kind: .folder, name: name, bookmark: bookmark)
    }

    /// The sync record of this remote in the device folder. Drive keeps the file name the single-remote versions
    /// of the app used, so an existing phone carries its state over.
    var stateFileName: String { kind == .drive ? "sync-state.json" : "sync-state-\(id).json" }
}

/// The remotes of this phone, kept in the device folder (`remotes.json`). Not secret and never synced; the
/// bookmarks only mean something to this app on this phone.
public final class RemoteList: @unchecked Sendable {
    private struct Saved: Codable {
        var remotes: [RemoteEntry] = []
    }

    private let file: URL
    private let lock = NSLock()

    public init(container: VaultContainer) {
        file = container.deviceFolder.appendingPathComponent("remotes.json")
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

    public func entries() -> [RemoteEntry] {
        lock.lock()
        defer { lock.unlock() }
        return load().remotes
    }

    public func entry(id: String) -> RemoteEntry? { entries().first { $0.id == id } }

    /// Add `entry`, or replace the one with the same id.
    public func add(_ entry: RemoteEntry) {
        update { saved in
            if let index = saved.remotes.firstIndex(where: { $0.id == entry.id }) {
                saved.remotes[index] = entry
            } else {
                saved.remotes.append(entry)
            }
        }
    }

    /// Forget `id` and its sync record. A remote that is added again later starts as a new one.
    public func remove(id: String) {
        let removed = entry(id: id)
        update { $0.remotes.removeAll { $0.id == id } }
        if let removed {
            try? FileManager.default.removeItem(
                at: file.deletingLastPathComponent().appendingPathComponent(removed.stateFileName))
        }
    }

    public func removeAll() {
        for entry in entries() { remove(id: entry.id) }
        try? FileManager.default.removeItem(at: file)
    }

    /// Whether this phone has ever saved a list. A phone from an earlier version has none yet.
    public var hasSavedList: Bool { FileManager.default.fileExists(atPath: file.path) }

    /// Start-up: a phone signed in to Google with no list yet (an earlier version, or a vault just opened from
    /// Drive) gets its Drive entry. Once a list exists it is left alone, so a Drive remote the person turned off
    /// (after deleting the Drive copy) stays off while the Google sign-in stays.
    public func adoptSignedInDrive(signedIn: Bool) {
        if signedIn { if !hasSavedList { add(.drive) } } else { reconcileDrive(signedIn: false) }
    }

    /// Keep Drive's entry in line with the sign-in: present while signed in, gone after sign out. A phone from an
    /// earlier version, signed in but with no list yet, gets its Drive entry here.
    public func reconcileDrive(signedIn: Bool) {
        if signedIn {
            if entry(id: RemoteEntry.driveID) == nil { add(.drive) }
        } else {
            remove(id: RemoteEntry.driveID)
            // A different account may sign in next: an empty Drive is then a new remote, not a lost one.
            try? FileManager.default.removeItem(
                at: file.deletingLastPathComponent().appendingPathComponent(RemoteEntry.drive.stateFileName))
        }
    }
}
