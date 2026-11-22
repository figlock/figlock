import FigLockFFI
import Foundation

@testable import Shared

/// A vault folder with items written by a first session, and a second session that reads them back the way the
/// app does: unlock, then apply the op files. Everything is generated for the tests.
struct SeededVault {
    static let password = "generated-test-password-1"

    let container: VaultContainer
    let vault: LocalVault
    let session: VaultSession
    let log: VaultLog
    let itemIds: [String]

    func cleanup() { try? FileManager.default.removeItem(at: container.root) }

    /// `items` are `(type, fields)` pairs; they are added, committed as one op file and stored under `log/`.
    static func make(_ items: [(String, [(String, String)])]) throws -> SeededVault {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("figlock-vault-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let container = VaultContainer(root: root)
        try container.prepare()
        let vault = LocalVault(container: container, access: .readWrite)
        let nowMs: UInt64 = 1_800_000_000_000
        let created = try createVault(
            password: password, deviceName: "Test iPhone", kdf: minimumKdfSettings(), nowMs: nowMs)
        let store = try vault.openStore()
        try store.putNew(path: "vault.json", bytes: created.vaultJson())
        let keys = created.deviceKeys()
        try store.putNew(path: "devices/\(keys.deviceId()).json", bytes: created.deviceRecord())
        try vault.markVaultPresent()

        let writer = try created.takeSession()
        var ids: [String] = []
        for (offset, item) in items.enumerated() {
            let fields = item.1.map { FieldInput(name: $0.0, value: $0.1) }
            ids.append(
                try writer.addItem(itemType: item.0, vault: "personal", fields: fields, nowMs: nowMs + UInt64(offset)))
            // Real edits are committed one at a time; batches keep each op file well inside the size limit.
            if (offset + 1) % 250 == 0 || offset + 1 == items.count, let file = try writer.commit() {
                try store.putNew(path: file.path, bytes: file.bytes)
            }
        }
        let secretKey = try created.secretKeyText()
        writer.lock()

        let outcome = try unlockVault(
            vaultJson: created.vaultJson(), password: password, secretKey: secretKey, device: keys,
            deviceName: "Test iPhone", deviceRecords: [created.deviceRecord()], nowMs: nowMs + 1_000_000)
        return SeededVault(
            container: container, vault: vault, session: outcome.session, log: VaultLog(store: store),
            itemIds: ids)
    }

    @MainActor
    func browser() async -> VaultBrowser {
        let browser = VaultBrowser(session: session, log: log, now: { SeededVault.tick() })
        await browser.reload()
        return browser
    }

    private static let ticks = NSLock()
    nonisolated(unsafe) private static var elapsed = 0.0

    /// A moment just after the vault was made, so op files are not dated in the future, moving on 10 ms per call
    /// like a real clock does while a large vault loads.
    static func tick() -> Date {
        ticks.lock()
        defer { ticks.unlock() }
        elapsed += 0.01
        return Date(timeIntervalSince1970: 1_800_000_000 + 2_000 + elapsed)
    }
}

/// A sleep a test opens by hand, so timers fire when the test says.
actor SleepGate {
    private var waiting: [CheckedContinuation<Void, Never>] = []
    private(set) var requested: [TimeInterval] = []

    func sleep(_ seconds: TimeInterval) async {
        requested.append(seconds)
        await withCheckedContinuation { waiting.append($0) }
    }

    func fire() {
        let pending = waiting
        waiting = []
        for continuation in pending { continuation.resume() }
    }

    var sleeping: Int { waiting.count }
}

final class FakePasteboard: Pasteboard, @unchecked Sendable {
    private let lock = NSLock()
    private(set) var text: String?
    private(set) var expiry: Date?
    private(set) var count = 0
    private(set) var localOnly = false

    var changeCount: Int { lock.withLock { count } }

    func setLocalOnly(_ text: String, expiry: Date) {
        lock.withLock {
            self.text = text
            self.expiry = expiry
            localOnly = true
            count += 1
        }
    }

    /// Something else (another app, the person) put text on the clipboard.
    func foreignCopy(_ text: String) {
        lock.withLock {
            self.text = text
            expiry = nil
            localOnly = false
            count += 1
        }
    }

    func clear() {
        lock.withLock {
            text = nil
            count += 1
        }
    }
}

struct FakeAuthenticator: Authenticating {
    let allow: Bool
    func authenticate(reason: String) async -> Bool { allow }
}

/// Suspends in `authenticate` until the test calls `release`, to model a Face ID prompt that is still up.
actor GatedAuthenticator: Authenticating {
    private var waiting: [CheckedContinuation<Void, Never>] = []
    var pending: Int { waiting.count }

    func authenticate(reason: String) async -> Bool {
        await withCheckedContinuation { waiting.append($0) }
        return true
    }

    func release() {
        let pending = waiting
        waiting = []
        for continuation in pending { continuation.resume() }
    }
}

/// Wait up to two seconds for `condition`, so a broken timer fails a test instead of hanging it.
@MainActor
func waitFor(_ condition: @MainActor () async -> Bool) async {
    for _ in 0..<2_000 {
        if await condition() { return }
        try? await Task.sleep(for: .milliseconds(1))
    }
}
