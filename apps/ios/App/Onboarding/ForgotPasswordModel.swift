import FigLockFFI
import Observation
import Shared
import SwiftUI

/// The "I forgot my password or Secret Key" flow on the lock screen. One model for its four steps; nothing is
/// stored until the new Emergency Kit was confirmed, so closing the sheet at any step changes nothing.
@MainActor
@Observable
final class ForgotPasswordModel {
    enum Step { case key, password, kit, verify }

    var step: Step = .key
    var busy = false
    var errorText: String?

    var recoveryKey = ""
    var password = ""
    var confirmation = ""
    var verification = ""
    var kitURL: URL?
    var kitOpened = false

    private var recovering: Onboarding.Recovering?
    private let service: Onboarding
    private let lock: AppLock
    private let deviceName: String

    init(service: Onboarding, lock: AppLock, deviceName: String) {
        self.service = service
        self.lock = lock
        self.deviceName = deviceName
    }

    var keyLooksValid: Bool { recoveryKeyIsValid(code: recoveryKey) }
    var passwordsMatch: Bool { password == confirmation }
    var canContinuePassword: Bool { VaultPasswordRule.isLongEnough(password) && passwordsMatch && !busy }
    var verificationMatches: Bool { recovering?.verify(typed: verification) ?? false }

    func clearError() { errorText = nil }

    func findVault() async {
        guard keyLooksValid else {
            errorText = "That is not a valid Recovery Key. It starts with FLR1 and has 14 groups of four characters."
            return
        }
        await run {
            recovering = try await service.startRecovery(code: recoveryKey, deviceName: deviceName)
            step = .password
        }
    }

    func choosePassword() async {
        guard canContinuePassword, let recovering else { return }
        await run {
            try await service.prepareRecovery(recovering, newPassword: password)
            prepareKit()
            step = .kit
        }
    }

    private func prepareKit() {
        guard kitURL == nil, let pdf = try? recovering?.emergencyKit(date: Self.today) else { return }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("FigLock Emergency Kit.pdf")
        try? pdf.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        kitURL = url
    }

    private static var today: String { String(ISO8601DateFormatter().string(from: Date()).prefix(10)) }

    /// Check the typed end of the new Secret Key, store the new vault and unlock with it. Returns whether the
    /// sheet can close.
    func finish() async -> Bool {
        guard let recovering else { return false }
        guard verificationMatches else {
            errorText =
                "That doesn't match the last two groups of your new Secret Key. Check your saved Emergency Kit and try again."
            return false
        }
        var done = false
        await run {
            let newKey = try service.commitRecovery(recovering)
            let newPassword = password
            self.recovering = nil
            removeKit()
            clearSecrets()
            let outcome = await lock.unlock(password: newPassword, secretKey: newKey)
            lock.vaultDidChange()
            done = outcome == .unlocked
        }
        return done
    }

    #if DEBUG
        /// For the UI test only (`-figlock-expose-new-key`): the last two groups of the new fixture Secret Key.
        var debugKitGroups: String? {
            guard ProcessInfo.processInfo.arguments.contains("-figlock-expose-new-key") else { return nil }
            return recovering?.newSecretKeyText().map { $0.split(separator: "-").suffix(2).joined(separator: "-") }
        }
    #endif

    /// Drop everything held in memory and the kit file (the person backed out).
    func cancel() {
        recovering = nil
        removeKit()
        clearSecrets()
    }

    private func removeKit() {
        if let kitURL { try? FileManager.default.removeItem(at: kitURL) }
        kitURL = nil
    }

    private func clearSecrets() {
        recoveryKey = ""
        password = ""
        confirmation = ""
        verification = ""
    }

    private func run(_ step: () async throws -> Void) async {
        busy = true
        errorText = nil
        defer { busy = false }
        do {
            try await step()
        } catch let failure as Onboarding.Failure {
            errorText = Self.text(failure)
        } catch {
            errorText = "Something went wrong. Try again."
        }
    }

    static func text(_ failure: Onboarding.Failure) -> String {
        switch failure {
        case .badRecoveryKey:
            "That is not a valid Recovery Key. Check the code, a letter or number may be mistyped."
        case .recoveryFailed:
            "This Recovery Key does not open the vault on this iPhone. Check the code, and that it is the key of this vault."
        case .noRecoveryKey:
            "This vault has no Recovery Key, so it cannot be opened this way. Use your Emergency Kit and password, or a device that is already set up."
        default: "Something went wrong. Try again."
        }
    }
}
