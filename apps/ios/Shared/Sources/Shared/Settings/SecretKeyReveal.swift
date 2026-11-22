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
        guard await authenticator.authenticate(reason: "Show your Secret Key in FigLock"),
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
        guard await authenticator.authenticate(reason: "Save your Emergency Kit"), started == generation,
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

    public func cleanUp() {
        generation += 1
        hide()
        if let kitFile { try? FileManager.default.removeItem(at: kitFile) }
        kitFile = nil
    }
}
