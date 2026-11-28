import FigLockFFI
import Foundation
import Testing

@testable import Shared

private struct Counting: Authenticating {
    let allow: Bool
    let calls: Counter
    func authenticate(reason: String) async -> Bool {
        await calls.bump()
        return allow
    }
}

private actor Counter {
    private(set) var count = 0
    func bump() { count += 1 }
}

private func seeded() throws -> SeededVault {
    try SeededVault.make([
        ("login", [("title", "Example"), ("username", "me"), ("password", "generated-export-secret")]),
        ("note", [("title", "Second"), ("body", "generated note")]),
    ])
}

private func folder() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("figlock-export-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

@MainActor
@Test func exportingWithoutSecretsAsksNothingAndLeavesSecretsOut() async throws {
    let vault = try seeded()
    _ = await vault.browser()
    let dir = try folder()
    defer {
        vault.cleanup()
        try? FileManager.default.removeItem(at: dir)
    }
    let calls = Counter()
    let exporter = VaultExporter(authenticator: Counting(allow: false, calls: calls), folder: dir)

    let outcome = await exporter.prepare(session: vault.session, includeSecrets: false, includeDeleted: false)
    guard case .ready(let url) = outcome else {
        Issue.record("expected a file, got \(outcome)")
        return
    }
    #expect(await calls.count == 0)
    let text = try String(contentsOf: url, encoding: .utf8)
    #expect(!text.contains("generated-export-secret"))
    let doc = try #require(try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any])
    #expect(doc["format"] as? String == "figlock-export-1")
    #expect(doc["secrets_included"] as? Bool == false)
    #expect((doc["items"] as? [Any])?.count == 2)
}

@MainActor
@Test func exportingWithSecretsNeedsFaceIDEveryTimeAndAFailedPromptMakesNoFile() async throws {
    let vault = try seeded()
    _ = await vault.browser()
    let dir = try folder()
    defer {
        vault.cleanup()
        try? FileManager.default.removeItem(at: dir)
    }
    let calls = Counter()
    let refused = VaultExporter(authenticator: Counting(allow: false, calls: calls), folder: dir)
    #expect(
        await refused.prepare(session: vault.session, includeSecrets: true, includeDeleted: false)
            == .notConfirmed)
    #expect(refused.file == nil)
    #expect(try FileManager.default.contentsOfDirectory(atPath: dir.path).isEmpty)

    let allowed = VaultExporter(authenticator: Counting(allow: true, calls: calls), folder: dir)
    for _ in 0..<2 {
        let outcome = await allowed.prepare(session: vault.session, includeSecrets: true, includeDeleted: false)
        guard case .ready(let url) = outcome else {
            Issue.record("expected a file, got \(outcome)")
            return
        }
        #expect(try String(contentsOf: url, encoding: .utf8).contains("generated-export-secret"))
    }
    #expect(await calls.count == 3)
}

@MainActor
@Test func theFileIsRemovedWhenTheShareSheetIsDoneAndAfterALockedSessionFails() async throws {
    let vault = try seeded()
    _ = await vault.browser()
    let dir = try folder()
    defer {
        vault.cleanup()
        try? FileManager.default.removeItem(at: dir)
    }
    let exporter = VaultExporter(authenticator: FakeAuthenticator(allow: true), folder: dir)
    guard
        case .ready(let url) = await exporter.prepare(
            session: vault.session, includeSecrets: true, includeDeleted: true)
    else {
        Issue.record("expected a file")
        return
    }
    #expect(FileManager.default.fileExists(atPath: url.path))
    exporter.cleanUp()
    #expect(!FileManager.default.fileExists(atPath: url.path))
    #expect(exporter.file == nil)

    // A locked vault exports nothing and leaves no file behind.
    vault.session.lock()
    #expect(
        await exporter.prepare(session: vault.session, includeSecrets: false, includeDeleted: false) == .failed)
    #expect(try FileManager.default.contentsOfDirectory(atPath: dir.path).isEmpty)
}

@MainActor
@Test func onlyBackgroundingRemovesTheFileAnInactiveSceneKeepsIt() async throws {
    let vault = try seeded()
    _ = await vault.browser()
    let dir = try folder()
    defer {
        vault.cleanup()
        try? FileManager.default.removeItem(at: dir)
    }
    let exporter = VaultExporter(authenticator: FakeAuthenticator(allow: true), folder: dir)
    guard
        case .ready(let url) = await exporter.prepare(
            session: vault.session, includeSecrets: false, includeDeleted: false)
    else {
        Issue.record("expected a file")
        return
    }
    exporter.cleanUpForScenePhase(isBackground: false)
    #expect(FileManager.default.fileExists(atPath: url.path))
    #expect(exporter.file == url)
    exporter.cleanUpForScenePhase(isBackground: true)
    #expect(!FileManager.default.fileExists(atPath: url.path))
    #expect(exporter.file == nil)
}
