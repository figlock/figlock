/// The one rule for a new vault (master) password on iOS: at least 8 characters, as on the desktop app. The CLI only
/// refuses an empty password. The Secret Key carries the entropy the password does not, so the
/// minimum is a typo guard, not a strength claim.
public enum VaultPasswordRule {
    public static let minimumLength = 8

    public static func isLongEnough(_ password: String) -> Bool {
        password.count >= minimumLength
    }
}
