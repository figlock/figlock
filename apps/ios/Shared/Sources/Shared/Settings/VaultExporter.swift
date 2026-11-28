import FigLockFFI
import Foundation
import Observation

/// Exporting the vault as JSON from Settings (FIG-288): the document `figlock export` writes, built by the Rust core,
/// written to a temporary file for the share sheet or the Files app. Without secrets it needs nothing. With
/// secrets it needs Face ID or the passcode every time, and a refused prompt makes no file. The file is removed by
/// `cleanUp` as soon as the share sheet closes, and when the screen goes away.
@MainActor
@Observable
public final class VaultExporter {
    public enum Outcome: Equatable {
        case ready(URL)
        /// Face ID or the passcode was not confirmed: nothing was made.
        case notConfirmed
        case failed
    }

    /// The export file waiting to be shared, until it is cleaned up.
    public private(set) var file: URL?

    private let authenticator: any Authenticating
    private let folder: URL
    private var generation = 0
    private var authenticating = false

    public init(
        authenticator: any Authenticating = DeviceAuthenticator(),
        folder: URL = FileManager.default.temporaryDirectory
    ) {
        self.authenticator = authenticator
        self.folder = folder
    }

    public func prepare(session: VaultSession, includeSecrets: Bool, includeDeleted: Bool) async -> Outcome {
        cleanUp()
        let started = generation
        if includeSecrets {
            authenticating = true
            let confirmed = await authenticator.authenticate(reason: "Export your FigLock vault with its secrets")
            authenticating = false
            guard confirmed, started == generation, !Task.isCancelled else { return .notConfirmed }
        }
        guard
            let text = try? session.exportJson(includeSecrets: includeSecrets, includeDeleted: includeDeleted),
            started == generation
        else { return .failed }
        let url = folder.appendingPathComponent("figlock-export.json")
        do {
            try Data(text.utf8).write(
                to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        } catch {
            return .failed
        }
        file = url
        return .ready(url)
    }

    /// The app left the foreground. Face ID's own sheet takes the scene to `inactive`, which must not throw away
    /// the export it is asking for (same cause as FIG-233); a real `background` does.
    public func cleanUpForScenePhase(isBackground: Bool) {
        if authenticating && !isBackground { return }
        cleanUp()
    }

    public func cleanUp() {
        generation += 1
        if let file { try? FileManager.default.removeItem(at: file) }
        file = nil
    }
}
