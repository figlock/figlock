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
    let onboarding: Onboarding
    let identitySync: CredentialIdentitySync
    let drive: DriveConnection
    let sync: SyncModel
    let settings: SettingsContext
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
        let quick = QuickUnlock(container: container, chip: SecureEnclaveChip(accessGroup: Self.keychainGroup))
        let identitySync = CredentialIdentitySync()
        self.identitySync = identitySync
        onboarding = Onboarding(vault: vault, keeper: keeper, quick: quick, identities: identitySync)
        drive = DriveConnection(store: KeychainSecretStore(accessGroup: Self.keychainGroup))
        sync = SyncModel(vault: vault, keeper: keeper, drive: drive)
        BackgroundSync.model = sync
        let deviceSettings = DeviceSettings(container: container)
        lock = AppLock(
            vault: vault,
            keeper: keeper,
            quick: quick,
            deviceName: UIDevice.current.name,
            autoLockAfter: deviceSettings.autoLock.seconds)
        settings = SettingsContext(
            lock: lock, service: SettingsService(vault: vault, keeper: keeper), settings: deviceSettings,
            onboarding: onboarding, drive: drive, sync: sync)
        #if DEBUG
            DebugSeed.freshIfRequested(self)
            DebugSeed.seedIfRequested(self)
            DebugSeed.remotesIfRequested(self)
            DebugSeed.unlockIfRequested(self)
        #endif
    }
}
