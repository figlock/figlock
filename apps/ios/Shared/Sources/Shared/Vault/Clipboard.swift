import Foundation
import UIKit
import UniformTypeIdentifiers

/// The system pasteboard, as far as copying a secret needs it.
public protocol Pasteboard: AnyObject {
    var changeCount: Int { get }
    /// Replace the contents with `text`, kept on this device only and expiring at `expiry` (the system clears it).
    func setLocalOnly(_ text: String, expiry: Date)
    func clear()
}

/// `UIPasteboard.general` with the options a secret needs: not offered to other devices through Universal
/// Clipboard (`localOnly`) and removed by the system at `expiry`, even if the app is no longer running.
public final class SystemPasteboard: Pasteboard {
    public init() {}

    public var changeCount: Int { UIPasteboard.general.changeCount }

    public func setLocalOnly(_ text: String, expiry: Date) {
        UIPasteboard.general.setItems(
            [[UTType.plainText.identifier: text]],
            options: [.localOnly: true, .expirationDate: expiry])
    }

    public func clear() { UIPasteboard.general.items = [] }
}

/// Copies a value and clears it again 30 seconds later, unless something else was copied in between.
@MainActor
public final class ClipboardCopier {
    public static let clearAfter: TimeInterval = 30

    private let pasteboard: any Pasteboard
    private let sleep: @Sendable (TimeInterval) async -> Void
    private let now: @Sendable () -> Date
    private var clearTask: Task<Void, Never>?
    private var mark = 0
    /// Which field is on the clipboard now, for the "Copied" caption. Cleared with the clipboard.
    public private(set) var copiedKey: String?

    public init(
        pasteboard: any Pasteboard = SystemPasteboard(),
        now: @escaping @Sendable () -> Date = { Date() },
        sleep: @escaping @Sendable (TimeInterval) async -> Void = { seconds in
            try? await Task.sleep(for: .seconds(seconds))
        }
    ) {
        self.pasteboard = pasteboard
        self.now = now
        self.sleep = sleep
    }

    /// Put `text` on the clipboard. `key` names the field for the caption.
    public func copy(_ text: String, key: String) {
        clearTask?.cancel()
        pasteboard.setLocalOnly(text, expiry: now().addingTimeInterval(Self.clearAfter))
        copiedKey = key
        mark = pasteboard.changeCount
        let wait = sleep
        clearTask = Task { [weak self] in
            await wait(Self.clearAfter)
            guard !Task.isCancelled, let self else { return }
            self.clearNow()
        }
    }

    /// Clear now, for locking the app.
    public func clearNow() {
        clearTask?.cancel()
        // Someone else copied something since: leave their clipboard alone.
        if copiedKey != nil, pasteboard.changeCount == mark { pasteboard.clear() }
        copiedKey = nil
    }
}
