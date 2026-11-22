import FigLockFFI

/// The one place the apps reach the Rust core. Later tickets (FIG-159 onwards) put the vault storage and the
/// session on top of this; for now it proves the library links and answers.
public enum FigLockCore {
    /// Argon2id cost parameters.
    public struct Kdf: Equatable, Sendable {
        public let memoryKiB: UInt32
        public let iterations: UInt32
        public let parallelism: UInt32
    }

    /// The Argon2id profile a real vault is created with.
    public static var defaultKdf: Kdf {
        let settings = defaultKdfSettings()
        return Kdf(memoryKiB: settings.mKib, iterations: settings.t, parallelism: settings.p)
    }

    /// True when `text` is a well formed Secret Key (`FL1-...`).
    public static func isValidSecretKey(_ text: String) -> Bool {
        secretKeyIsValid(text: text)
    }
}
