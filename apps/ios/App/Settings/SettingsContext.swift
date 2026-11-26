import FigLockFFI
import Shared
import SwiftUI

/// Which look the app has, held here so a change in Settings redraws the whole app at once.
@MainActor
@Observable
final class AppearanceModel {
    private(set) var option: AppearanceOption
    private let settings: DeviceSettings

    init(settings: DeviceSettings) {
        self.settings = settings
        option = settings.appearance
    }

    /// `nil` follows the system.
    var colorScheme: ColorScheme? {
        switch option {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }

    /// Returns false when the choice could not be saved (the screen keeps the old one).
    @discardableResult
    func choose(_ new: AppearanceOption) -> Bool {
        do { try settings.setAppearance(new) } catch { return false }
        option = new
        return true
    }
}

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
    let appearance: AppearanceModel

    init(
        lock: AppLock, service: SettingsService, settings: DeviceSettings, onboarding: Onboarding,
        drive: DriveConnection, sync: SyncModel, appearance: AppearanceModel
    ) {
        self.lock = lock
        self.service = service
        self.settings = settings
        self.onboarding = onboarding
        self.drive = drive
        self.sync = sync
        self.appearance = appearance
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
