import Foundation

/// Where the app and the AutoFill extension keep the one local vault (ADR-0041 decision 2, ADR-0026).
///
/// Everything sits in the App Group container, which both targets can reach:
///
/// ```
/// <container>/vault/     vault.json, devices/, log/, snap/, inbox/ - ciphertext only, the sync target
/// <container>/device/    this device's own state (device config, quick unlock blob) - never leaves the phone
/// ```
///
/// Both folders are excluded from device backups, so neither the quick unlock blob nor the device identity can
/// move to another phone through a restore, and they get the file protection class
/// `completeUntilFirstUserAuthentication`, so the extension can read while the phone is locked after it has
/// been unlocked once. The vault stays ciphertext at rest regardless.
public struct VaultContainer: Sendable {
    public enum SetupError: Error, Equatable {
        /// The App Group is not available to this process (a missing entitlement or a wrong group id).
        case containerUnavailable
    }

    /// The File protection class the vault and device folders are created with.
    public static let protection = FileProtectionType.completeUntilFirstUserAuthentication

    public let root: URL

    /// A container rooted at `root`: the App Group container in the apps, a temporary folder in tests.
    public init(root: URL) {
        self.root = root
    }

    /// The App Group container of `groupIdentifier`. `locate` is the system lookup; tests replace it, because
    /// the simulator answers for any group id.
    public static func appGroup(
        _ groupIdentifier: String,
        locate: (String) -> URL? = { FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: $0) }
    ) throws -> VaultContainer {
        guard let url = locate(groupIdentifier) else { throw SetupError.containerUnavailable }
        return VaultContainer(root: url)
    }

    public var vaultFolder: URL { root.appendingPathComponent("vault", isDirectory: true) }
    public var deviceFolder: URL { root.appendingPathComponent("device", isDirectory: true) }

    /// Written once a vault exists on this phone. A vault folder that is gone while this is still here is a
    /// folder that went missing, not a phone that has no vault yet.
    var presenceMarker: URL { deviceFolder.appendingPathComponent("vault-present") }

    /// Create the folders if they are not there and apply the backup exclusion and protection class to both.
    /// Safe to repeat; the app calls it on launch. Never recreates a vault folder that went missing: that is
    /// `LocalVault.status()`'s `missing` case, and the person decides what happens next.
    public func prepare() throws {
        for folder in [deviceFolder] + (hasPresenceMarker ? [] : [vaultFolder]) {
            try createProtected(folder)
        }
        if hasPresenceMarker, FileManager.default.fileExists(atPath: vaultFolder.path) {
            try secure(vaultFolder)
        }
    }

    /// Create the vault folder with its protections. Only for a phone that has no vault yet, or after the
    /// person chose to start over or restore into a new folder.
    func createVaultFolder() throws {
        try createProtected(vaultFolder)
    }

    var hasPresenceMarker: Bool {
        FileManager.default.fileExists(atPath: presenceMarker.path)
    }

    func markVaultPresent() throws {
        try createProtected(deviceFolder)
        try Data().write(to: presenceMarker, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    func forgetVaultPresent() throws {
        if hasPresenceMarker { try FileManager.default.removeItem(at: presenceMarker) }
    }

    private func createProtected(_ folder: URL) throws {
        try FileManager.default.createDirectory(
            at: folder, withIntermediateDirectories: true,
            attributes: [.protectionKey: Self.protection])
        try secure(folder)
    }

    /// Exclude `folder` from backups and set its protection class; new files inside inherit both.
    private func secure(_ folder: URL) throws {
        var url = folder
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try url.setResourceValues(values)
        try FileManager.default.setAttributes([.protectionKey: Self.protection], ofItemAtPath: folder.path)
    }
}
