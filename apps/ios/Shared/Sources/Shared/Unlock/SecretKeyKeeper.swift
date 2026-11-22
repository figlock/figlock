import FigLockFFI
import Foundation

/// The Secret Key and this device's keys, in the keychain, with the person's choice about keeping the Secret Key
/// (ADR-0038).
///
/// Stopping to keep the Secret Key is a remembered, per-vault choice in the device folder (not secret). While it
/// is set nothing puts the key back: not a full unlock, not an update, not a rotation. Keeping it again is one
/// action and clears the choice.
public struct SecretKeyKeeper: Sendable {
    private let store: any SecretStore
    private let optOutFile: URL

    public init(store: any SecretStore, container: VaultContainer) {
        self.store = store
        self.optOutFile = container.deviceFolder.appendingPathComponent("secret-key-opt-out.json")
    }

    private func secretKeyAccount(_ vaultId: String) -> String { "secret-key.\(vaultId)" }
    private func deviceKeysAccount(_ vaultId: String) -> String { "device-keys.\(vaultId)" }

    // MARK: Secret Key

    public func isOptedOut(vaultId: String) -> Bool {
        optedOutVaults().contains(vaultId)
    }

    /// The kept Secret Key text, or nil when this phone keeps none.
    public func secretKey(vaultId: String) throws -> String? {
        guard let data = try store.read(secretKeyAccount(vaultId)) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    /// True when the unlock screen has to ask for the Secret Key: the person stopped keeping it, or none is kept
    /// yet (a vault just opened here, or another device rotated the key). Never because Face ID was turned on or
    /// off (ADR-0038 decision 3).
    public func needsTyping(vaultId: String) throws -> Bool {
        try secretKey(vaultId: vaultId) == nil
    }

    /// Keep `secretKey` after a full unlock. Does nothing while the person has opted out. Returns whether it kept it.
    @discardableResult
    public func keep(_ secretKey: String, vaultId: String) throws -> Bool {
        guard !isOptedOut(vaultId: vaultId) else { return false }
        try store.write(secretKeyAccount(vaultId), Data(secretKey.utf8))
        return true
    }

    /// "Stop keeping it on this iPhone": deletes the key and remembers the choice.
    public func stopKeeping(vaultId: String) throws {
        try setOptedOut(true, vaultId: vaultId)
        try store.delete(secretKeyAccount(vaultId))
    }

    /// "Keep it on this iPhone": clears the choice and keeps the key. The caller has checked the master password
    /// and Secret Key against the vault first.
    public func keepAgain(_ secretKey: String, vaultId: String) throws {
        try store.write(secretKeyAccount(vaultId), Data(secretKey.utf8))
        try setOptedOut(false, vaultId: vaultId)
    }

    /// A rotation on another device made the kept key stale: drop it so the screen asks for the new one. The
    /// opt-out choice is not touched.
    public func discardStale(vaultId: String) throws {
        try store.delete(secretKeyAccount(vaultId))
    }

    // MARK: Device keys

    public func deviceKeys(vaultId: String) throws -> DeviceKeysHandle? {
        guard let data = try store.read(deviceKeysAccount(vaultId)) else { return nil }
        return try? DeviceKeysHandle.fromBytes(bytes: data)
    }

    public func saveDeviceKeys(_ keys: DeviceKeysHandle, vaultId: String) throws {
        try store.write(deviceKeysAccount(vaultId), keys.toBytes())
    }

    /// Remove everything kept for the vault, for "remove this vault from this iPhone".
    public func removeAll(vaultId: String) throws {
        try store.delete(secretKeyAccount(vaultId))
        try store.delete(deviceKeysAccount(vaultId))
        try setOptedOut(false, vaultId: vaultId)
    }

    // MARK: Opt-out file

    private struct OptOut: Codable { var vaults: [String] }

    private func optedOutVaults() -> Set<String> {
        guard let data = try? Data(contentsOf: optOutFile),
            let decoded = try? JSONDecoder().decode(OptOut.self, from: data)
        else { return [] }
        return Set(decoded.vaults)
    }

    private func setOptedOut(_ enabled: Bool, vaultId: String) throws {
        var vaults = optedOutVaults()
        if enabled { vaults.insert(vaultId) } else { vaults.remove(vaultId) }
        let data = try JSONEncoder().encode(OptOut(vaults: vaults.sorted()))
        try FileManager.default.createDirectory(
            at: optOutFile.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: optOutFile, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }
}
