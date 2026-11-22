import FigLockFFI
import Foundation
import LocalAuthentication
import Observation

/// The check that stands between a stored secret and the screen: Face ID, falling back to the passcode.
public protocol Authenticating: Sendable {
    func authenticate(reason: String) async -> Bool
}

public struct DeviceAuthenticator: Authenticating {
    public init() {}

    public func authenticate(reason: String) async -> Bool {
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else { return false }
        return (try? await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)) ?? false
    }
}

/// Showing secrets of one item: masked by default, shown for 10 seconds after Face ID, then masked again
/// (`DESIGN.md` 8). Only this type asks the session for a secret value, and it forgets it when it re-masks.
@MainActor
@Observable
public final class SecretReveal {
    public static let visibleFor: TimeInterval = 10

    public private(set) var shown: [String: String] = [:]
    public private(set) var failed: Set<String> = []

    private let session: VaultSession
    private let itemId: String
    private let authenticator: any Authenticating
    private let sleep: @Sendable (TimeInterval) async -> Void
    private var timers: [String: Task<Void, Never>] = [:]
    private var generation = 0

    public init(
        session: VaultSession, itemId: String, authenticator: any Authenticating = DeviceAuthenticator(),
        sleep: @escaping @Sendable (TimeInterval) async -> Void = { seconds in
            try? await Task.sleep(for: .seconds(seconds))
        }
    ) {
        self.session = session
        self.itemId = itemId
        self.authenticator = authenticator
        self.sleep = sleep
    }

    public func isShown(_ field: String) -> Bool { shown[field] != nil }

    /// Show `field` after the person proves it is them. Returns whether it is shown now.
    @discardableResult
    public func show(_ field: String, label: String) async -> Bool {
        failed.remove(field)
        let started = generation
        let allowed = await authenticator.authenticate(reason: "Show the \(label) in FigLock")
        guard allowed, started == generation, !Task.isCancelled else { return false }
        guard let value = try? session.revealField(itemId: itemId, field: field) else {
            failed.insert(field)
            return false
        }
        shown[field] = value
        timers[field]?.cancel()
        let wait = sleep
        timers[field] = Task { [weak self] in
            await wait(Self.visibleFor)
            guard !Task.isCancelled else { return }
            self?.hide(field)
        }
        return true
    }

    public func hide(_ field: String) {
        generation += 1
        timers[field]?.cancel()
        timers[field] = nil
        shown[field] = nil
    }

    public func hideAll() {
        generation += 1
        for field in Array(shown.keys) { hide(field) }
    }

    /// The value for copying, read straight from the session without showing it. Copy needs no extra check: the
    /// vault is already unlocked, and the clipboard value is local-only and cleared after 30 seconds.
    public func valueForCopy(_ field: String) -> String? {
        try? session.revealField(itemId: itemId, field: field)
    }
}
