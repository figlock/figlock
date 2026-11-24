import FigLockFFI
import Shared
import SwiftUI

/// The state of the onboarding screens (`design/prototypes/ios`: welcome, create vault, pairing, open from Drive)
/// over the `Onboarding` service. All the storage, key and pairing work is in `Shared`; this holds what the
/// screens show and what the person typed.
@MainActor
@Observable
final class OnboardingModel {
    enum Route: Hashable {
        case create, secretKey, kit, verify
        case pairCode, pairWait, pairApproved
        case drive
    }

    /// Where the vault is read from while onboarding: Google Drive, or a folder picked in the Files picker (iCloud
    /// Drive). Pairing and opening work the same over both (ADR-0043).
    enum Source: String, CaseIterable, Identifiable {
        case drive = "Google Drive"
        case icloud = "iCloud Drive"
        var id: Self { self }
    }

    var path: [Route] = []
    var busy = false
    var errorText: String?

    // Create
    var password = ""
    var confirmation = ""
    var secretKeyShown = false
    var kitURL: URL?
    var kitOpened = false
    var verification = ""
    var faceID = true

    // Pairing
    var code = ""
    var fingerprint = ""

    // Drive
    var source: Source = .drive {
        didSet {
            guard source != oldValue else { return }
            foundVault = nil
            errorText = nil
        }
    }
    private(set) var folder: OpenedFolder?
    private var folderEntry: RemoteEntry?
    var foundVault: Onboarding.RemoteVault?
    var typedSecretKey = ""

    private var pending: Onboarding.PendingVault?
    private var pairing: Onboarding.Pairing?
    private var pollTask: Task<Void, Never>?

    let service: Onboarding
    let lock: AppLock
    let drive: DriveConnection
    private let deviceName: String
    let copier: ClipboardCopier

    init(
        service: Onboarding, lock: AppLock, drive: DriveConnection, deviceName: String,
        copier: ClipboardCopier = ClipboardCopier()
    ) {
        self.copier = copier
        self.service = service
        self.lock = lock
        self.drive = drive
        self.deviceName = deviceName
    }

    var faceIDAvailable: Bool { service.quickAvailable }

    // MARK: The remote

    /// Whether the chosen source is ready: signed in to Google, or a folder picked.
    var hasRemote: Bool { source == .drive ? drive.isSignedIn : folder != nil }

    private func currentRemote() throws -> (store: any BlobStoreFfi, entry: RemoteEntry) {
        switch source {
        case .drive: return (try drive.remote(), .drive)
        case .icloud:
            guard let folder, let folderEntry else { throw FolderProblem.pickAgain }
            return (folder.store, folderEntry)
        }
    }

    /// The person picked a folder. Nothing is created in it; it only has to be a folder FigLock may use.
    func chooseFolder(_ result: Result<URL, Error>) async {
        guard case .success(let url) = result else { return }
        await run {
            closeFolder()
            foundVault = nil
            let opened = try FolderRemote.open(url: url, localVault: service.vaultFolder)
            do {
                folderEntry = try FolderRemote.entry(for: url)
            } catch {
                opened.close()
                throw FolderProblem.pickAgain
            }
            folder = opened
        }
    }

    func closeFolder() {
        folder?.close()
        folder = nil
        folderEntry = nil
    }

    // MARK: Create

    var passwordsMatch: Bool { password == confirmation }
    var canContinuePassword: Bool { VaultPasswordRule.isLongEnough(password) && passwordsMatch && !busy }

    func startCreation() async {
        guard canContinuePassword else { return }
        await run {
            pending = try await service.startCreation(password: password, deviceName: deviceName)
            secretKeyShown = false
            path.append(.secretKey)
        }
    }

    func secretKeyText() -> String { (try? pending?.secretKeyText()) ?? "" }

    /// The key as plain text, local-only and expiring (ADR-0041 decision 6); never the Emergency Kit PDF (FIG-218).
    func copySecretKey() { copier.copy(secretKeyText(), key: Self.secretKeyCopyKey) }
    static let secretKeyCopyKey = "secretKey"

    func prepareKit() {
        guard kitURL == nil, let pending else { return }
        let date = ISO8601DateFormatter().string(from: Date()).prefix(10)
        guard let pdf = try? pending.emergencyKit(vaultName: "My vault", date: String(date)) else { return }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("FigLock Emergency Kit.pdf")
        try? pdf.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        kitURL = url
    }

    var verificationMatches: Bool { pending?.verify(typed: verification) ?? false }

    func finishCreation() async {
        guard let pending else { return }
        guard verificationMatches else {
            errorText =
                "That doesn't match the last two groups of your Secret Key. Check your saved Emergency Kit and try again."
            return
        }
        await run {
            _ = try await service.commitCreation(pending, enableFaceID: faceID)
            removeKit()
            let password = self.password
            self.pending = nil
            await openVault(password: password, secretKey: nil)
        }
    }

    // MARK: Pairing

    var codeLooksValid: Bool { inviteCodeIsValid(code: code) }

    func signInToDrive() async {
        await run { try await drive.signIn() }
    }

    func beginPairing() async {
        guard codeLooksValid else {
            errorText =
                "That code is not the right length. It has 20 letters and numbers, like ABCD-EFGH-JKMN-PQRS-TVWX."
            return
        }
        await run {
            let (remote, entry) = try currentRemote()
            let started = try await service.beginPairing(
                code: code, remote: remote, deviceName: deviceName, entry: entry)
            pairing = started
            fingerprint = started.fingerprint
            path.append(.pairWait)
            startPolling()
        }
    }

    private func startPolling() {
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self, let pairing = self.pairing else { return }
                if (try? pairing.answered()) == true {
                    self.path = [.pairApproved]
                    return
                }
                try? await Task.sleep(for: .seconds(3))
            }
        }
    }

    func cancelPairing() {
        pollTask?.cancel()
        pairing = nil
        closeFolder()
        path = []
    }

    func finishPairing() async {
        guard let pairing else { return }
        await run {
            let password = self.password
            try await pairing.finish(password: password)
            if faceID { await service.turnOnFaceID(password: password) }
            self.pairing = nil
            await openVault(password: password, secretKey: nil)
        }
    }

    // MARK: Open from Drive

    func lookForVault() async {
        await run {
            foundVault = try service.inspect(remote: try currentRemote().store)
            if foundVault == nil { throw Onboarding.Failure.noVaultInStorage }
        }
    }

    var secretKeyLooksValid: Bool { FigLockCore.isValidSecretKey(typedSecretKey) }

    func openFromRemote() async {
        guard secretKeyLooksValid else {
            errorText = "That Secret Key is not well formed. It starts with FL1- and is on your Emergency Kit."
            return
        }
        await run {
            let (store, entry) = try currentRemote()
            try await service.openFromRemote(store, entry: entry)
            let password = self.password
            let key = typedSecretKey
            await openVault(password: password, secretKey: key)
            if faceID, lock.isUnlocked { await service.turnOnFaceID(password: password, secretKey: key) }
        }
    }

    // MARK: Shared

    private func openVault(password: String, secretKey: String?) async {
        let outcome = await lock.unlock(password: password, secretKey: secretKey)
        lock.vaultDidChange()
        switch outcome {
        case .unlocked: clearSecrets()
        case .wrongCredentials: errorText = "That master password or Secret Key isn't right. Check them and try again."
        default: errorText = "FigLock could not open the vault. Try again."
        }
    }

    private func removeKit() {
        if let kitURL { try? FileManager.default.removeItem(at: kitURL) }
        kitURL = nil
    }

    private func clearSecrets() {
        closeFolder()
        password = ""
        confirmation = ""
        verification = ""
        typedSecretKey = ""
        code = ""
    }

    func clearError() { errorText = nil }

    /// Run a step with the busy flag and one place that words every failure.
    private func run(_ step: () async throws -> Void) async {
        busy = true
        errorText = nil
        defer { busy = false }
        do {
            try await step()
        } catch let failure as Onboarding.Failure {
            errorText = Self.text(failure, source: source)
        } catch let failure as SignInError {
            errorText = Self.text(failure)
        } catch let problem as FolderProblem {
            errorText = SyncSettingsView.text(problem)
        } catch {
            errorText = "Something went wrong. Try again."
        }
    }

    static func text(_ failure: Onboarding.Failure, source: Source = .drive) -> String {
        let place = source == .drive ? "that Google Drive" : "that folder"
        switch failure {
        case .noVaultInStorage: return "FigLock found no vault in \(place)."
        case .offline: return "FigLock could not reach \(place). Check your connection and try again."
        case .waitingForDownload:
            return "iCloud has not downloaded the vault in \(place) yet. Wait a moment and try again."
        case .foreignVault:
            return "\(place.prefix(1).uppercased() + place.dropFirst()) holds a different vault than this iPhone."
        default: return text(failure)
        }
    }

    static func text(_ failure: Onboarding.Failure) -> String {
        switch failure {
        case .invalidCode: "That code is not valid, has expired, or belongs to a different vault."
        case .alreadyAnswered: "Someone already answered that code. Ask for a new one on your other device."
        case .joinFailed: "Could not join. The master password may be wrong, or the answer is not valid."
        case .signIn: "Google Drive refused the sign-in. Sign in again."
        case .damaged: "The vault file is damaged."
        case .alreadyHasVault: "This iPhone already has a vault."
        default: "Something went wrong. Try again."
        }
    }

}

extension OnboardingModel {
    static func text(_ failure: SignInError) -> String {
        switch failure {
        case .notConfigured: "Google sign-in is not set up in this build yet."
        case .cancelled: "Sign-in was cancelled."
        case .refused: "Google did not accept the sign-in. Try again."
        case .accessDenied:
            "Google was not given permission. Sign in again and allow FigLock to keep its vault in your Drive."
        case .clientRejected:
            "Google does not recognise this build of FigLock as a sign-in client. It needs an iOS OAuth client made for this app."
        case .codeRejected: "Google's sign-in code expired or was already used. Try again."
        case .noRefreshToken:
            "Google did not let FigLock stay signed in. Remove FigLock under your Google account's third-party access, then try again."
        case .broaderScope: "Google offered more access than FigLock asked for, so FigLock refused it. Try again."
        case .unavailable: "FigLock could not reach Google. Check your connection and try again."
        }
    }
}
