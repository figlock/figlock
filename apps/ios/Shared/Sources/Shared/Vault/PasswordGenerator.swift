import FigLockFFI
import Foundation

/// The settings of the password generator and the calls into the Rust one (`figlock-core` `generate`). The
/// random draws, the character classes and the strength estimate all live in Rust.
public struct GeneratorSettings: Equatable, Sendable {
    public enum Mode: String, CaseIterable, Identifiable, Sendable {
        case password
        case passphrase

        public var id: String { rawValue }
        public var title: String { self == .password ? "Password" : "Passphrase" }
    }

    public enum Separator: String, CaseIterable, Identifiable, Sendable {
        case hyphen
        case dot
        case space
        case none

        public var id: String { rawValue }
        public var title: String {
            switch self {
            case .hyphen: "-"
            case .dot: "."
            case .space: "Space"
            case .none: "None"
            }
        }
        var ffi: PassphraseSeparator {
            switch self {
            case .hyphen: .hyphen
            case .dot: .dot
            case .space: .space
            case .none: .none
            }
        }
    }

    public static let lengthRange = 8...128
    public static let wordsRange = 3...16

    public var mode: Mode = .password
    public var length = 20
    public var lower = true
    public var upper = true
    public var digits = true
    public var symbols = true
    public var avoidLookalikes = false
    public var words = 6
    public var separator: Separator = .hyphen
    public var capitalize = false
    public var addNumber = false

    public init() {}

    /// Whether the settings can produce something: a password needs a length in range and one class on.
    public var isValid: Bool {
        switch mode {
        case .password: Self.lengthRange.contains(length) && (lower || upper || digits || symbols)
        case .passphrase: Self.wordsRange.contains(words)
        }
    }

    public func generate() -> String? {
        guard isValid else { return nil }
        switch mode {
        case .password:
            return try? generatePassword(
                length: UInt32(length), lower: lower, upper: upper, digits: digits, symbols: symbols,
                avoidLookalikes: avoidLookalikes)
        case .passphrase:
            return try? generatePassphrase(
                words: UInt32(words), separator: separator.ffi, capitalize: capitalize, addNumber: addNumber)
        }
    }

    public enum Rating: String, Sendable {
        case weak = "Weak"
        case fair = "Fair"
        case strong = "Strong"
    }

    /// How strong `value` is, as the strength meter shows it.
    public static func rating(of value: String) -> Rating? {
        guard !value.isEmpty else { return nil }
        switch passwordStrength(password: value).level {
        case .weak: return .weak
        case .fair: return .fair
        case .strong: return .strong
        }
    }
}
