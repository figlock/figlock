import Foundation
import Observation

/// Showing the Secret Key and saving the Emergency Kit from Settings: both need Face ID or the passcode first. The
/// key is shown for 10 seconds and then masked again (`DESIGN.md` 8), like any other secret.
@MainActor
@Observable
public final class SecretKeyReveal {
    public static let visibleFor = SecretReveal.visibleFor

    /// The Secret Key while it is shown, otherwise nil.
    public private(set) var shown: String?
    /// The kit PDF made for sharing, until it is cleaned up.
    public private(set) var kitFile: URL?

    private let service: SettingsService
    private let authenticator: any Authenticating
    private let sleep: @Sendable (TimeInterval) async -> Void
    private let kitFolder: URL
    private var timer: Task<Void, Never>?
    private var generation = 0
    private var authenticating = false

    public init(
        service: SettingsService, authenticator: any Authenticating = DeviceAuthenticator(),
        kitFolder: URL = FileManager.default.temporaryDirectory,
        sleep: @escaping @Sendable (TimeInterval) async -> Void = { seconds in
            try? await Task.sleep(for: .seconds(seconds))
        }
    ) {
        self.service = service
        self.authenticator = authenticator
        self.kitFolder = kitFolder
        self.sleep = sleep
    }

    /// Whether this phone keeps a Secret Key at all.
    public var isKept: Bool { service.keptSecretKey() != nil }

    /// Show the key after the person proves it is them. Returns whether it is shown now.
    @discardableResult
    public func show() async -> Bool {
        let started = generation
        guard await authenticate(reason: "Show your Secret Key in FigLock"),
            started == generation, !Task.isCancelled,
            let key = service.keptSecretKey()
        else { return false }
        shown = key
        timer?.cancel()
        let wait = sleep
        timer = Task { [weak self] in
            await wait(Self.visibleFor)
            guard !Task.isCancelled else { return }
            self?.hide()
        }
        return true
    }

    public func hide() {
        timer?.cancel()
        timer = nil
        shown = nil
    }

    /// Make the kit PDF for sharing, after the person proves it is them. The file holds the Secret Key, so it is
    /// removed by `cleanUp` as soon as the share sheet is done, and when this screen goes away.
    public func prepareKit(storage: String) async -> URL? {
        let started = generation
        guard await authenticate(reason: "Save your Emergency Kit"), started == generation,
            !Task.isCancelled, let pdf = try? service.emergencyKit(storage: storage)
        else { return nil }
        let url = kitFolder.appendingPathComponent("FigLock Emergency Kit.pdf")
        do {
            try pdf.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        } catch {
            return nil
        }
        kitFile = url
        return url
    }

    private func authenticate(reason: String) async -> Bool {
        authenticating = true
        defer { authenticating = false }
        return await authenticator.authenticate(reason: reason)
    }

    /// The app left the foreground. Face ID's own sheet also takes the app out of `.active` (the scene goes
    /// `inactive` while it is up), and that must not throw away the reveal it is asking for: it would succeed and
    /// then show nothing (FIG-233, same cause as FIG-223). Only a real `background` cancels one waiting on Face ID.
    public func cleanUpForScenePhase(isBackground: Bool) {
        if authenticating && !isBackground { return }
        cleanUp()
    }

    public func cleanUp() {
        generation += 1
        hide()
        if let kitFile { try? FileManager.default.removeItem(at: kitFile) }
        kitFile = nil
    }
}
