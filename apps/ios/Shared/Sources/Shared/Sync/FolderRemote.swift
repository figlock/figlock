import FigLockFFI
import Foundation

/// Why a folder cannot be used as a remote right now. No path or name is carried.
public enum FolderProblem: Error, Equatable, Sendable {
    /// The folder is gone, or the bookmark no longer leads to one. Never recreated by a sync.
    case missing
    /// The bookmark is stale or unreadable and the person has to pick the folder again.
    case pickAgain
    /// The folder is, contains or sits inside the local vault folder (ADR-0043).
    case insideVault
    /// The folder is, contains or sits inside a folder this phone already syncs with (FIG-225).
    case alreadyRemote
    /// The folder is in iCloud Drive and iCloud Drive is off or signed out on this iPhone.
    case iCloudOff
    /// The folder holds files but no vault. Syncing into it could sit next to another device's folder of the
    /// same name ("FigLock 2"), so it is refused.
    case notEmpty
    /// The folder holds a different vault than this phone.
    case differentVault
    /// The folder has no vault to open or join.
    case noVault
    /// The Files provider refused access to the folder.
    case denied
}

/// A folder the person picked, opened for use: the security scope is held until `close()` or deinit, and the
/// store works through file coordination. Nothing is ever created at the folder itself.
public final class OpenedFolder: @unchecked Sendable {
    public let url: URL
    public let store: CoordinatedFolderStore
    private let scoped: Bool

    fileprivate init(url: URL, scoped: Bool, store: CoordinatedFolderStore) {
        self.url = url
        self.scoped = scoped
        self.store = store
    }

    public func close() {
        if scoped { url.stopAccessingSecurityScopedResource() }
    }

    deinit { close() }
}

/// Everything about a picked folder that is not the store itself: bookmarks, the checks of ADR-0043 and the
/// iCloud hint.
public enum FolderRemote {
    /// A bookmark for a URL the Files picker returned. Taken inside its security scope.
    public static func bookmark(for url: URL) throws -> Data {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        return try url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
    }

    /// Whether iCloud Drive is on and signed in for this iPhone.
    public static var iCloudIsOn: Bool { FileManager.default.ubiquityIdentityToken != nil }

    /// The name a remote shows for a folder the person picked. The root of a Files location (such as On My iPhone)
    /// is a system folder called "File Provider Storage", which means nothing to a person, so it is shown as
    /// "Files folder" instead (FIG-217).
    public static func displayName(for url: URL) -> String {
        let name = url.lastPathComponent
        return name.isEmpty || name == "/" || name == "File Provider Storage" ? "Files folder" : name
    }

    /// The remote for a folder the person just picked: a bookmark taken inside its scope and a name for people.
    public static func entry(for url: URL) throws -> RemoteEntry {
        .folder(name: displayName(for: url), bookmark: try bookmark(for: url))
    }

    /// A folder in iCloud Drive is under `Mobile Documents`, whichever way the picker hands it out.
    public static func isInICloudDrive(_ url: URL) -> Bool {
        url.resolvingSymlinksInPath().path.contains("/Mobile Documents/")
    }

    /// Open a bookmarked folder. Throws `pickAgain` when the bookmark cannot be resolved, `missing` when the
    /// folder is gone, `iCloudOff` for an iCloud folder while iCloud is off, and `insideVault` when it overlaps
    /// the local vault. `refreshed` is a new bookmark when the old one was stale but still led to the folder.
    public static func open(
        bookmark: Data, localVault: URL, iCloudOn: @Sendable () -> Bool = { iCloudIsOn }
    ) throws -> (folder: OpenedFolder, refreshed: Data?) {
        var stale = false
        let url: URL
        do {
            url = try URL(resolvingBookmarkData: bookmark, options: [], relativeTo: nil, bookmarkDataIsStale: &stale)
        } catch {
            // A folder that was deleted can no longer be resolved; any other failure means iOS dropped the access.
            let code = (error as NSError).code
            throw (error as NSError).domain == NSCocoaErrorDomain
                && [CocoaError.fileNoSuchFile.rawValue, CocoaError.fileReadNoSuchFile.rawValue].contains(code)
                ? FolderProblem.missing : FolderProblem.pickAgain
        }
        let folder = try open(url: url, localVault: localVault, iCloudOn: iCloudOn)
        var refreshed: Data?
        if stale { refreshed = try? Self.bookmark(for: url) }
        return (folder, refreshed)
    }

    /// Open a folder the person just picked (the URL carries a security scope) or one resolved from a bookmark.
    public static func open(
        url: URL, localVault: URL, iCloudOn: @Sendable () -> Bool = { iCloudIsOn }
    ) throws -> OpenedFolder {
        let scoped = url.startAccessingSecurityScopedResource()
        do {
            if isInICloudDrive(url), !iCloudOn() { throw FolderProblem.iCloudOff }
            try check(folder: url, localVault: localVault)
            return OpenedFolder(url: url, scoped: scoped, store: try CoordinatedFolderStore(root: url))
        } catch {
            if scoped { url.stopAccessingSecurityScopedResource() }
            throw error
        }
    }

    /// The real-path check of ADR-0043: the folder exists and is not, does not contain and is not inside the
    /// local vault folder. Symbolic links are resolved on both sides.
    public static func check(folder: URL, localVault: URL) throws {
        var isFolder: ObjCBool = false
        guard FileManager.default.fileExists(atPath: folder.path, isDirectory: &isFolder), isFolder.boolValue else {
            throw FolderProblem.missing
        }
        let one = components(of: folder)
        let other = components(of: localVault)
        let shared = zip(one, other).prefix { $0 == $1 }.count
        if shared == one.count || shared == other.count { throw FolderProblem.insideVault }
    }

    private static func components(of url: URL) -> [String] {
        url.resolvingSymlinksInPath().standardizedFileURL.pathComponents
    }

    /// Whether two folders are the same, or one holds the other, by their resolved real paths. Symbolic links are
    /// followed, and a folder whose name only starts like the other's is a different folder.
    public static func overlap(_ one: URL, _ other: URL) -> Bool {
        let first = components(of: one)
        let second = components(of: other)
        let shared = zip(first, second).prefix { $0 == $1 }.count
        return shared == first.count || shared == second.count
    }

    /// FIG-225: refuse a folder that is, contains or sits inside a folder this phone already syncs with. Each
    /// remote's bookmark is resolved to its real folder, so the same folder picked again (which gives different
    /// bookmark bytes) is still found. `ignoring` is the remote being replaced. A bookmark that cannot be resolved
    /// holds no known folder and is skipped.
    public static func checkNotARemote(_ url: URL, among entries: [RemoteEntry], ignoring: String? = nil) throws {
        for entry in entries where entry.kind == .folder && entry.id != ignoring {
            guard let bookmark = entry.bookmark else { continue }
            var stale = false
            guard
                let existing = try? URL(
                    resolvingBookmarkData: bookmark, options: [], relativeTo: nil, bookmarkDataIsStale: &stale)
            else { continue }
            if overlap(url, existing) { throw FolderProblem.alreadyRemote }
        }
    }

    /// What may become a remote of a vault that already exists on this phone: an empty folder, or one that holds
    /// this vault. A folder of other files, or another vault, is refused. A `vault.json` that is still downloading
    /// cannot be compared yet and is let through; the first sync compares it before anything is merged.
    public static func checkCanHold(vaultId: String, in store: CoordinatedFolderStore) throws {
        let anything: Bool
        let top: [BlobEntry]
        do {
            anything = try store.hasAnyContent()
            top = try store.list(prefix: "")
        } catch { throw FolderProblem.denied }
        if !anything { return }
        guard top.contains(where: { $0.path == "vault.json" }) else { throw FolderProblem.notEmpty }

        do {
            let found = try vaultIdentity(vaultJson: try store.get(path: "vault.json"))
            if found.vaultId != vaultId { throw FolderProblem.differentVault }
        } catch StoreFailure.NotYetAvailable {
            return
        } catch let problem as FolderProblem {
            throw problem
        } catch {
            throw FolderProblem.notEmpty
        }
    }
}
