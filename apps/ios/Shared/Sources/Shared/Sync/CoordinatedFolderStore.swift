import FigLockFFI
import Foundation

/// A picked folder (iCloud Drive or any Files location) as a `BlobStoreFfi`, for the folder remote of ADR-0043.
///
/// Every read and write goes through `NSFileCoordinator`, so the iCloud daemon and other processes never see a
/// half written file. A file that iCloud lists but has not downloaded (the evicted file of a File Provider
/// volume, or the hidden `.name.icloud` stub of older systems) is reported as `available: false`, and reading it
/// asks the OS to download it and throws `NotYetAvailable`: it is "not downloaded yet", never missing and never a
/// gap. The sync engine waits a bounded time for `vault.json` and retries on the next run.
///
/// The root folder is never created here; a missing one throws, so a sync cannot make a `FigLock` folder next to
/// another device's of the same name. Paths follow the rules every store has (`figlock-store` `path.rs`).
public final class CoordinatedFolderStore: BlobStoreFfi, @unchecked Sendable {
    public enum SetupError: Error, Equatable { case notAFolder }

    private static let maxBlob = 64 * 1024 * 1024
    private static let stubSuffix = ".icloud"

    private let root: URL
    private let rootComponents: [String]

    public init(root: URL) throws {
        var isFolder: ObjCBool = false
        guard FileManager.default.fileExists(atPath: root.path, isDirectory: &isFolder), isFolder.boolValue else {
            throw SetupError.notAFolder
        }
        self.root = root
        rootComponents = root.resolvingSymlinksInPath().standardizedFileURL.pathComponents
    }

    // MARK: Paths

    private static func validSegment(_ segment: Substring) -> Bool {
        !segment.isEmpty && segment.utf8.count <= 200 && !segment.hasPrefix(".")
            && segment.utf8.allSatisfy {
                ($0 >= 97 && $0 <= 122) || ($0 >= 48 && $0 <= 57) || $0 == 46 || $0 == 95 || $0 == 45
            }
    }

    private static func validate(path: String) throws {
        let segments = path.split(separator: "/", omittingEmptySubsequences: false)
        guard !path.isEmpty, path.utf8.count <= 512, segments.count <= 4, segments.allSatisfy(validSegment) else {
            throw StoreFailure.InvalidPath
        }
    }

    private static func validate(prefix: String) throws {
        guard prefix.utf8.count <= 512 else { throw StoreFailure.InvalidPath }
        if prefix.isEmpty { return }
        let segments = prefix.split(separator: "/", omittingEmptySubsequences: false)
        guard let last = segments.last, segments.count <= 5,
            segments.dropLast().allSatisfy(validSegment), last.isEmpty || validSegment(last)
        else { throw StoreFailure.InvalidPath }
    }

    private func url(for path: String) -> URL {
        path.split(separator: "/").reduce(root) { $0.appendingPathComponent(String($1)) }
    }

    private func stub(for url: URL) -> URL {
        url.deletingLastPathComponent().appendingPathComponent(".\(url.lastPathComponent)\(Self.stubSuffix)")
    }

    private func requireRoot() throws {
        var isFolder: ObjCBool = false
        guard FileManager.default.fileExists(atPath: root.path, isDirectory: &isFolder), isFolder.boolValue else {
            throw StoreFailure.Other
        }
    }

    // MARK: Coordination

    private static func map(_ error: Error) -> StoreFailure {
        if let failure = error as? StoreFailure { return failure }
        let code = (error as? CocoaError)?.code ?? CocoaError.Code(rawValue: (error as NSError).code)
        switch code {
        case .fileNoSuchFile, .fileReadNoSuchFile: return .NotFound
        case .fileWriteFileExists: return .Exists
        default: return .Other
        }
    }

    private func coordinate<T>(
        reading url: URL, options: NSFileCoordinator.ReadingOptions = [], _ body: (URL) throws -> T
    ) throws -> T {
        var coordinationError: NSError?
        var outcome: Result<T, Error>?
        NSFileCoordinator(filePresenter: nil).coordinate(
            readingItemAt: url, options: options, error: &coordinationError,
            byAccessor: { (accessed: URL) in outcome = Result { try body(accessed) } })
        if let coordinationError { throw Self.map(coordinationError) }
        guard let outcome else { throw StoreFailure.Other }
        do { return try outcome.get() } catch { throw Self.map(error) }
    }

    private func coordinate<T>(
        writing url: URL, options: NSFileCoordinator.WritingOptions = [], _ body: (URL) throws -> T
    ) throws -> T {
        var coordinationError: NSError?
        var outcome: Result<T, Error>?
        NSFileCoordinator(filePresenter: nil).coordinate(
            writingItemAt: url, options: options, error: &coordinationError,
            byAccessor: { (accessed: URL) in outcome = Result { try body(accessed) } })
        if let coordinationError { throw Self.map(coordinationError) }
        guard let outcome else { throw StoreFailure.Other }
        do { return try outcome.get() } catch { throw Self.map(error) }
    }

    // MARK: Downloaded or not

    /// Whether the bytes of the file at `url` are on this iPhone. A file outside iCloud has no download status and
    /// counts as downloaded.
    private func isDownloaded(_ url: URL) -> Bool {
        let status = try? url.resourceValues(forKeys: [.ubiquitousItemDownloadingStatusKey])
            .ubiquitousItemDownloadingStatus
        guard let status else { return true }
        return status == .current || status == .downloaded
    }

    private func requestDownload(_ url: URL) {
        try? FileManager.default.startDownloadingUbiquitousItem(at: url)
    }

    // MARK: BlobStoreFfi

    public func get(path: String) throws -> Data {
        try Self.validate(path: path)
        try requireRoot()
        let file = url(for: path)
        let exists = FileManager.default.fileExists(atPath: file.path)
        guard exists else {
            if FileManager.default.fileExists(atPath: stub(for: file).path) {
                requestDownload(file)
                throw StoreFailure.NotYetAvailable
            }
            throw StoreFailure.NotFound
        }
        guard isDownloaded(file) else {
            requestDownload(file)
            throw StoreFailure.NotYetAvailable
        }
        return try coordinate(reading: file) { try Data(contentsOf: $0) }
    }

    public func putNew(path: String, bytes: Data) throws {
        try Self.validate(path: path)
        guard bytes.count <= Self.maxBlob else { throw StoreFailure.TooLarge }
        try requireRoot()
        let file = url(for: path)
        let taken = FileManager.default.fileExists(atPath: file.path)
        let evicted = FileManager.default.fileExists(atPath: stub(for: file).path)
        if taken || evicted { throw StoreFailure.Exists }
        try coordinate(writing: file) { target in
            try FileManager.default.createDirectory(
                at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
            let temporary = self.temporary(beside: target)
            do {
                try bytes.write(to: temporary)
                // `moveItem` never overwrites: a file that appeared meanwhile is `Exists`, not replaced.
                try FileManager.default.moveItem(at: temporary, to: target)
            } catch {
                try? FileManager.default.removeItem(at: temporary)
                throw error
            }
        }
    }

    public func putReplace(path: String, bytes: Data) throws {
        try Self.validate(path: path)
        guard bytes.count <= Self.maxBlob else { throw StoreFailure.TooLarge }
        try requireRoot()
        let file = url(for: path)
        try coordinate(writing: file, options: .forReplacing) { target in
            try FileManager.default.createDirectory(
                at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
            let temporary = self.temporary(beside: target)
            do {
                try bytes.write(to: temporary)
                if FileManager.default.fileExists(atPath: target.path) {
                    _ = try FileManager.default.replaceItemAt(target, withItemAt: temporary)
                } else {
                    try FileManager.default.moveItem(at: temporary, to: target)
                }
            } catch {
                try? FileManager.default.removeItem(at: temporary)
                throw error
            }
        }
    }

    public func delete(path: String) throws {
        try Self.validate(path: path)
        try requireRoot()
        let file = url(for: path)
        for target in [file, stub(for: file)] where FileManager.default.fileExists(atPath: target.path) {
            do {
                try coordinate(writing: target, options: .forDeleting) { try FileManager.default.removeItem(at: $0) }
            } catch StoreFailure.NotFound {
                continue
            }
        }
    }

    public func list(prefix: String) throws -> [BlobEntry] {
        try Self.validate(prefix: prefix)
        try requireRoot()
        let directoryPart = prefix.lastIndex(of: "/").map { String(prefix[...$0]) } ?? ""
        let base = url(for: directoryPart)
        guard FileManager.default.fileExists(atPath: base.path) else { return [] }
        let keys: [URLResourceKey] = [
            .isRegularFileKey, .fileSizeKey, .contentModificationDateKey, .ubiquitousItemDownloadingStatusKey,
        ]
        return try coordinate(reading: base) { directory in
            guard
                let walker = FileManager.default.enumerator(
                    at: directory, includingPropertiesForKeys: keys, options: [.skipsPackageDescendants])
            else { return [] }
            var found: [String: BlobEntry] = [:]
            for case let item as URL in walker {
                guard let entry = self.entry(for: item, keys: keys), entry.path.hasPrefix(prefix) else { continue }
                // A real file wins over a stub of the same name.
                if found[entry.path] == nil || entry.available { found[entry.path] = entry }
            }
            return found.values.sorted { $0.path < $1.path }
        }
    }

    // MARK: Listing helpers

    private func relativePath(of item: URL) -> String? {
        let parts = item.resolvingSymlinksInPath().standardizedFileURL.pathComponents
        guard parts.count > rootComponents.count, Array(parts.prefix(rootComponents.count)) == rootComponents else {
            return nil
        }
        return parts.dropFirst(rootComponents.count).joined(separator: "/")
    }

    private func entry(for item: URL, keys: [URLResourceKey]) -> BlobEntry? {
        guard let values = try? item.resourceValues(forKeys: Set(keys)), values.isRegularFile == true else {
            return nil
        }
        var name = item.lastPathComponent
        var evicted = false
        if name.hasPrefix("."), name.hasSuffix(Self.stubSuffix), name.count > Self.stubSuffix.count + 1 {
            // The hidden placeholder of an evicted file on older systems: `.name.icloud` stands for `name`.
            name = String(name.dropFirst().dropLast(Self.stubSuffix.count))
            evicted = true
        }
        guard !name.hasPrefix("."), let located = relativePath(of: item) else { return nil }
        // The stub's own name is not the path: swap its last component for the real file's name.
        let relative = (located.split(separator: "/").dropLast() + [Substring(name)]).joined(separator: "/")
        guard (try? Self.validate(path: relative)) != nil else { return nil }
        let status = values.ubiquitousItemDownloadingStatus
        let downloaded = status == nil || status == .current || status == .downloaded
        return BlobEntry(
            path: relative, size: UInt64(max(values.fileSize ?? 0, 0)),
            mtimeMs: values.contentModificationDate.map { UInt64(max($0.timeIntervalSince1970 * 1000, 0)) },
            available: !evicted && downloaded)
    }

    private func temporary(beside target: URL) -> URL {
        target.deletingLastPathComponent().appendingPathComponent(".tmp-\(UUID().uuidString.lowercased())")
    }
}
