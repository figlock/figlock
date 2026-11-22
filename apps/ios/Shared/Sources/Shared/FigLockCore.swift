import FigLockFFI

/// The one place the apps reach the Rust core. Later tickets (FIG-159 onwards) put the vault storage and the
/// session on top of this; for now it proves the library links and answers.
public enum FigLockCore {
    /// The Argon2id profile a real vault is created with.
    public static var defaultKdf: (memoryKiB: UInt32, iterations: UInt32, parallelism: UInt32) {
        let s = defaultKdfSettings()
        return (s.mKib, s.t, s.p)
    }

    /// True when `text` is a well formed Secret Key (`FL1-...`).
    public static func isValidSecretKey(_ text: String) -> Bool {
        secretKeyIsValid(text: text)
    }
}
