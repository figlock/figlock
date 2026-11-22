import Shared
import SwiftUI

/// What the app is built from: the App Group container, the shared keychain group and the lock.
@MainActor
final class AppEnvironment {
    static let appGroup = "group.com.figlock.ios"
    /// The keychain group is `<team id prefix>com.figlock.ios.shared`. The prefix comes from the signing team and
    /// is not known to the code, so `nil` means the first group of the app's entitlement, which is that one.
    static let keychainGroup: String? = nil

    let vault: LocalVault
    let lock: AppLock
    let keeper: SecretKeyKeeper
    /// Set when the App Group container cannot be reached; the app then shows this instead of working elsewhere.
    let startupError: String?

    init() {
        var failure: String?
        var container: VaultContainer
        do {
            container = try VaultContainer.appGroup(Self.appGroup)
            try container.prepare()
        } catch {
            container = VaultContainer(
                root: FileManager.default.temporaryDirectory.appendingPathComponent("unavailable"))
            failure = "FigLock cannot reach its storage. Restart the app, or reinstall it if this stays."
        }
        startupError = failure
        vault = LocalVault(container: container, access: failure == nil ? .readWrite : .readOnly)
        keeper = SecretKeyKeeper(store: KeychainSecretStore(accessGroup: Self.keychainGroup), container: container)
        lock = AppLock(
            vault: vault,
            keeper: keeper,
            quick: QuickUnlock(container: container, chip: SecureEnclaveChip(accessGroup: Self.keychainGroup)),
            deviceName: UIDevice.current.name)
        #if DEBUG
            DebugSeed.seedIfRequested(self)
            DebugSeed.unlockIfRequested(self)
        #endif
    }
}
