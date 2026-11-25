import FigLockFFI
import Shared
import SwiftUI

/// What the Settings screens work with, handed down through the SwiftUI environment so the vault screens do not
/// have to carry it.
@MainActor
final class SettingsContext {
    let lock: AppLock
    let service: SettingsService
    let settings: DeviceSettings
    let onboarding: Onboarding
    let drive: DriveConnection
    let sync: SyncModel

    init(
        lock: AppLock, service: SettingsService, settings: DeviceSettings, onboarding: Onboarding,
        drive: DriveConnection, sync: SyncModel
    ) {
        self.lock = lock
        self.service = service
        self.settings = settings
        self.onboarding = onboarding
        self.drive = drive
        self.sync = sync
    }

    /// The Drive store while this iPhone is signed in to Google, otherwise nil.
    func remote() -> (any BlobStoreFfi)? { drive.isSignedIn ? try? drive.remote() : nil }

    /// Whether the unlocked vault has a Recovery Key.
    var recoveryKeyIsOn: Bool { lock.session.map { service.hasRecoveryKey(session: $0) } ?? false }

    /// Where the kit says the vault is kept.
    var storageLabel: String { sync.rows.isEmpty ? "On this iPhone" : sync.summary }
}

private struct SettingsContextKey: EnvironmentKey {
    static let defaultValue: SettingsContext? = nil
}

extension EnvironmentValues {
    var settingsContext: SettingsContext? {
        get { self[SettingsContextKey.self] }
        set { self[SettingsContextKey.self] = newValue }
    }
}
