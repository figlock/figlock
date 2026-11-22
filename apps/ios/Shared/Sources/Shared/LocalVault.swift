import FigLockFFI
import Foundation

/// The local vault folder as the app and the extension use it: one ciphertext folder, written atomically by the
/// app and only read by the extension (ADR-0041 decisions 2 and 7). The storage itself is the Rust folder store
/// of `figlock-ffi`, so path rules, atomic writes and the contract tests are the ones every platform has.
public struct LocalVault: Sendable {
    public enum Access: Sendable {
        /// The app: reads, writes and creates the folder.
        case readWrite
        /// The extension: reads only and never creates anything.
        case readOnly
    }

    public enum Status: Equatable, Sendable {
        /// This phone has no vault yet: show onboarding.
        case noVault
        /// A vault is here and its `vault.json` is shaped like one. It is not verified until unlock.
        case ready(vaultId: String, revision: UInt64)
        /// `vault.json` exists but is not a vault file (truncated or overwritten). Nothing is changed; the
        /// remote copy is never used as a silent replacement.
        case damaged
        /// A vault was here and the folder or its `vault.json` is gone: show the recovery screen (ADR-0026).
        case missing
        /// The folder is there but cannot be read right now.
        case unreadable
    }

    public enum StoreError: Error, Equatable, Sendable {
        /// The vault folder is gone: the recovery screen's case. Opening never recreates it.
        case missing
        /// A write was asked of a read-only handle.
        case readOnly
    }

    public let container: VaultContainer
    public let access: Access

    public init(container: VaultContainer, access: Access) {
        self.container = container
        self.access = access
    }

    /// Where this phone stands, read without changing anything.
    public func status() -> Status {
        let manager = FileManager.default
        var isFolder: ObjCBool = false
        let folderExists = manager.fileExists(atPath: container.vaultFolder.path, isDirectory: &isFolder)
        let vaultJson = container.vaultFolder.appendingPathComponent("vault.json")
        guard folderExists, isFolder.boolValue else {
            return container.hasPresenceMarker ? .missing : .noVault
        }
        guard manager.fileExists(atPath: vaultJson.path) else {
            return container.hasPresenceMarker ? .missing : .noVault
        }
        guard let bytes = try? Data(contentsOf: vaultJson) else { return .unreadable }
        guard let identity = try? vaultIdentity(vaultJson: bytes) else { return .damaged }
        return .ready(vaultId: identity.vaultId, revision: identity.revision)
    }

    /// The store over the vault folder, for reading and writing files or for the sync engine.
    ///
    /// A folder that is gone while a vault was here throws `missing` instead of being created empty. A read-only
    /// handle never creates anything and refuses every write.
    public func openStore() throws -> any BlobStoreFfi {
        let folder = container.vaultFolder
        var isFolder: ObjCBool = false
        let exists = FileManager.default.fileExists(atPath: folder.path, isDirectory: &isFolder)
        if !(exists && isFolder.boolValue) {
            if container.hasPresenceMarker || access == .readOnly { throw StoreError.missing }
            try container.createVaultFolder()
        }
        let folderStore = try FolderStore.open(path: folder.path)
        return access == .readOnly ? ReadOnlyStore(wrapping: folderStore) : folderStore
    }

    /// Record that this phone has a vault, so a vanished folder is reported as missing from now on. The app calls
    /// it once the first `vault.json` is written (create, join or open from a remote).
    public func markVaultPresent() throws {
        guard access == .readWrite else { throw StoreError.readOnly }
        try container.markVaultPresent()
    }

    /// Forget that this phone has a vault, after the person chose to remove it or to start over. The folder's
    /// files are not touched here.
    public func forgetVault() throws {
        guard access == .readWrite else { throw StoreError.readOnly }
        try container.forgetVaultPresent()
    }
}

/// A store that reads and refuses to write, for the AutoFill extension.
final class ReadOnlyStore: BlobStoreFfi {
    private let wrapping: any BlobStoreFfi

    init(wrapping: any BlobStoreFfi) {
        self.wrapping = wrapping
    }

    func get(path: String) throws -> Data { try wrapping.get(path: path) }
    func list(prefix: String) throws -> [BlobEntry] { try wrapping.list(prefix: prefix) }
    func putNew(path: String, bytes: Data) throws { throw LocalVault.StoreError.readOnly }
    func putReplace(path: String, bytes: Data) throws { throw LocalVault.StoreError.readOnly }
    func delete(path: String) throws { throw LocalVault.StoreError.readOnly }
}
