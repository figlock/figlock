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
    let appearance: AppearanceModel
    /// Set when the App Group container cannot be reached; the app then shows this instead of working elsewhere.
    let startupError: String?
    /// The browser, writer and copier of the current unlock (ADR-0048 decision 4), built once when the vault
    /// unlocks and dropped when it locks.
    private var unlocked: UnlockedVault?

    /// The vault of the current unlocked session, built on first use. A new unlock has a new session, so it gets a
    /// new one; nil while locked.
    func unlockedVault() -> UnlockedVault? {
        guard let session = lock.session else { return nil }
        if let unlocked, unlocked.session === session { return unlocked }
        unlocked?.copier.clearNow()
        let fresh = UnlockedVault(session: session, vault: vault, sync: sync, identitySync: identitySync)
        unlocked = fresh
        return fresh
    }

    /// The vault locked: forget what it held and take a copied secret off the pasteboard.
    func endUnlockedVault() {
        unlocked?.copier.clearNow()
        unlocked = nil
    }

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
        let driveSecrets = KeychainSecretStore(accessGroup: Self.keychainGroup)
        onboarding = Onboarding(
            vault: vault, keeper: keeper, quick: quick, identities: identitySync, driveGrants: driveSecrets)
        drive = DriveConnection(store: driveSecrets)
        sync = SyncModel(vault: vault, keeper: keeper, drive: drive)
        BackgroundSync.model = sync
        let deviceSettings = DeviceSettings(container: container)
        lock = AppLock(
            vault: vault,
            keeper: keeper,
            quick: quick,
            deviceName: UIDevice.current.name,
            autoLockAfter: deviceSettings.autoLock.seconds)
        appearance = AppearanceModel(settings: deviceSettings)
        settings = SettingsContext(
            lock: lock, service: SettingsService(vault: vault, keeper: keeper), settings: deviceSettings,
            onboarding: onboarding, drive: drive, sync: sync,
            appearance: appearance)
        #if DEBUG
            DebugSeed.freshIfRequested(self)
            DebugSeed.seedIfRequested(self)
            DebugSeed.seedRecoveryIfRequested(self)
            DebugSeed.remotesIfRequested(self)
            DebugSeed.unlockIfRequested(self)
        #endif
    }
}
