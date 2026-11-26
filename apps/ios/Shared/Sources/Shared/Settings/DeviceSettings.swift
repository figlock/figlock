import Foundation

/// How long FigLock may stay in the background before it locks (`design/prototypes/ios`, "Auto-lock"). There is no
/// "never" option on purpose.
public enum AutoLockOption: Double, CaseIterable, Identifiable, Sendable {
    case immediately = 0
    case oneMinute = 60
    case fiveMinutes = 300
    case fifteenMinutes = 900
    case oneHour = 3600

    public var id: Double { rawValue }
    public var seconds: TimeInterval { rawValue }

    public var title: String {
        switch self {
        case .immediately: "Immediately"
        case .oneMinute: "After 1 minute"
        case .fiveMinutes: "After 5 minutes"
        case .fifteenMinutes: "After 15 minutes"
        case .oneHour: "After 1 hour"
        }
    }

    /// The default for a phone that never chose (`AppLock.defaultAutoLock`).
    public static let standard = AutoLockOption.fiveMinutes

    /// The option for stored seconds; anything that is not one of the options (a hand edited file) is the default,
    /// never a longer time than the person could have picked.
    public static func from(seconds: TimeInterval?) -> AutoLockOption {
        guard let seconds, let option = AutoLockOption(rawValue: seconds) else { return .standard }
        return option
    }
}

/// Which look the app has: the system's, or always light or always dark.
public enum AppearanceOption: String, CaseIterable, Identifiable, Sendable {
    case system
    case light
    case dark

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .system: "System"
        case .light: "Light"
        case .dark: "Dark"
        }
    }

    /// The option for a stored value; anything unknown is the system look.
    public static func from(stored: String?) -> AppearanceOption {
        stored.flatMap(AppearanceOption.init(rawValue:)) ?? .system
    }
}

/// This phone's own settings: the auto-lock time and the names the person gave to the devices in the list. A small
/// file in the device folder; not secret and never synced.
///
/// The names are a label on this iPhone only. The vault itself keeps one name per device, signed by the vault, and
/// changing it for everyone is a change to the device records that needs its own decision (ADR); until then a
/// rename here changes what this iPhone shows.
public struct DeviceSettings: Sendable {
    private struct Saved: Codable {
        var autoLockSeconds: Double?
        var appearance: String?
        var nicknames: [String: String] = [:]
    }

    /// The longest nickname kept, in characters.
    public static let maxNameLength = 40

    private let file: URL

    public init(container: VaultContainer) {
        file = container.deviceFolder.appendingPathComponent("settings.json")
    }

    private func load() -> Saved {
        guard let data = try? Data(contentsOf: file), let saved = try? JSONDecoder().decode(Saved.self, from: data)
        else { return Saved() }
        return saved
    }

    private func save(_ saved: Saved) throws {
        try FileManager.default.createDirectory(
            at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try JSONEncoder().encode(saved).write(
            to: file, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    public var autoLock: AutoLockOption { AutoLockOption.from(seconds: load().autoLockSeconds) }

    public func setAutoLock(_ option: AutoLockOption) throws {
        var saved = load()
        saved.autoLockSeconds = option.seconds
        try save(saved)
    }

    public var appearance: AppearanceOption { AppearanceOption.from(stored: load().appearance) }

    public func setAppearance(_ option: AppearanceOption) throws {
        var saved = load()
        saved.appearance = option.rawValue
        try save(saved)
    }

    public var nicknames: [String: String] { load().nicknames }

    /// The name as it will be kept: trimmed and cut to `maxNameLength`; nil when nothing is left.
    public static func cleaned(_ name: String) -> String? {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : String(trimmed.prefix(maxNameLength))
    }

    /// Give `deviceId` a name on this iPhone, or take it away with nil or an empty name.
    public func setNickname(_ name: String?, for deviceId: String) throws {
        var saved = load()
        if let name, let cleaned = Self.cleaned(name) {
            saved.nicknames[deviceId] = cleaned
        } else {
            saved.nicknames[deviceId] = nil
        }
        try save(saved)
    }

    /// Forget everything, for "remove this vault from this iPhone".
    public func clear() {
        try? FileManager.default.removeItem(at: file)
    }
}
