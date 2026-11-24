import FigLockFFI
import Foundation
import Testing

@testable import Shared

/// The AutoFill extension's logic: matching, the QuickType identities and the read-only Face ID open. Every
/// value is generated for these tests.

private func item(
    _ id: String, kind: ItemKind = .login, user: String = "ada", urls: [String], deleted: Bool = false,
    updated: TimeInterval = 0
) -> VaultItem {
    var fields = ["title": "Item \(id)", "username": user]
    for (index, url) in urls.enumerated() { fields["url/a\(index)"] = url }
    return VaultItem(
        id: id, kind: kind, updated: Date(timeIntervalSince1970: updated), fields: fields, secretFields: ["password"],
        isDeleted: deleted)
}

@Test func hostsAreReadFromDomainsAndUrls() {
    #expect(ServiceMatch.host(of: "Example.COM") == "example.com")
    #expect(ServiceMatch.host(of: "https://www.example.com:8443/login?x=1") == "example.com")
    #expect(ServiceMatch.host(of: "example.com/login") == "example.com")
    #expect(ServiceMatch.host(of: "  ") == nil)
}

@Test func aLoginMatchesItsHostAndSubdomainsButNotSiblingsOrLookalikes() {
    let items = [
        item("exact", urls: ["https://example.com"]),
        item("parent", urls: ["example.com"], updated: 5),
        item("sibling", urls: ["https://mail.example.com"]),
        item("lookalike", urls: ["https://notexample.com"]),
        item("evil", urls: ["https://example.com.evil.net"]),
        item("tld", urls: ["https://com"]),
        item("note", kind: .note, urls: ["https://example.com"]),
        item("gone", urls: ["https://example.com"], deleted: true),
    ]
    let ids = ServiceMatch.matches(items, identifiers: ["accounts.example.com"]).map(\.id)
    #expect(ids == ["parent", "exact"])
    // The bare domain also offers the logins saved for its subdomains.
    #expect(
        ServiceMatch.matches(items, identifiers: ["https://example.com/signin"]).map(\.id)
            == ["parent", "exact", "sibling"])
    #expect(ServiceMatch.matches(items, identifiers: ["com"]).isEmpty)
    #expect(ServiceMatch.matches(items, identifiers: []).isEmpty)
}

@Test func exactHostsComeBeforeSubdomainMatches() {
    let items = [
        item("sub", urls: ["https://accounts.example.com"], updated: 9),
        item("exact", urls: ["https://example.com"], updated: 1),
    ]
    #expect(ServiceMatch.matches(items, identifiers: ["example.com"]).map(\.id) == ["exact", "sub"])
}

/// Wait for a write that runs in a task of its own; fails the test if it never arrives. The budget is generous
/// because CI runs the whole suite in parallel and a starved main actor can take tens of seconds to get to it.
private func eventually(_ condition: @Sendable () async -> Bool) async throws {
    for _ in 0..<2400 {
        if await condition() { return }
        try await Task.sleep(for: .milliseconds(50))
    }
    Issue.record("the condition never became true")
}

private actor RecordingStore: CredentialIdentityStoring {
    private(set) var replaced: [[CredentialIdentity]] = []
    private(set) var cleared = 0
    private var succeeds = true
    func setSucceeds(_ value: Bool) { succeeds = value }
    func replace(with identities: [CredentialIdentity]) -> Bool {
        replaced.append(identities)
        return succeeds
    }
    func removeAll() { cleared += 1 }
}

@Test func identitiesCarryDomainAndUsernameNeverASecret() {
    let identities = CredentialIdentity.identities(for: [
        item("a", user: "ada", urls: ["https://www.example.com", "example.com", "https://other.org"]),
        item("nouser", user: "", urls: ["https://example.com"]),
        item("token", kind: .token, urls: ["https://example.com"]),
        item("gone", urls: ["https://example.com"], deleted: true),
    ])
    #expect(
        identities == [
            CredentialIdentity(domain: "example.com", username: "ada", recordIdentifier: "a"),
            CredentialIdentity(domain: "other.org", username: "ada", recordIdentifier: "a"),
        ])
}

@MainActor
@Test func theSystemStoreIsWrittenOnlyWhenTheIdentitiesChange() async throws {
    let store = RecordingStore()
    let sync = CredentialIdentitySync(store: store)
    let items = [item("a", urls: ["https://example.com"])]
    sync.update(from: items)
    sync.update(from: items)
    sync.update(from: items + [item("b", urls: ["https://other.org"])])
    try await eventually { await store.replaced.count == 2 }
    sync.clear()
    try await eventually { await store.cleared == 1 }
    sync.update(from: items)
    try await eventually { await store.replaced.count == 3 }
}

@MainActor
@Test func aFailedWriteIsRetriedOnTheNextUpdate() async throws {
    let store = RecordingStore()
    await store.setSucceeds(false)
    let sync = CredentialIdentitySync(store: store)
    let items = [item("a", urls: ["https://example.com"])]
    sync.update(from: items)
    try await eventually { await store.replaced.count == 1 }
    await store.setSucceeds(true)
    sync.update(from: items)
    try await eventually { await store.replaced.count == 2 }
    try await Task.sleep(for: .milliseconds(100))
    sync.update(from: items)
    try await Task.sleep(for: .milliseconds(100))
    #expect(await store.replaced.count == 2)
}

@MainActor
@Test func removingTheVaultFromThePhoneEmptiesTheSystemStoreAndForgetsWhatWasWritten() async throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let store = RecordingStore()
    let sync = CredentialIdentitySync(store: store)
    let items = [item("a", urls: ["https://example.com"])]
    sync.update(from: items)
    try await eventually { await store.replaced.count == 1 }
    try await Task.sleep(for: .milliseconds(100))
    let onboarding = Onboarding(vault: phone.vault, keeper: phone.keeper, quick: phone.quick, identities: sync)
    try onboarding.removeFromThisPhone()
    try await eventually { await store.cleared == 1 }
    sync.update(from: items)
    try await eventually { await store.replaced.count == 2 }
}

private actor SlowOrderedStore: CredentialIdentityStoring {
    private(set) var events: [String] = []
    func replace(with identities: [CredentialIdentity]) async -> Bool {
        events.append("replace-start")
        try? await Task.sleep(for: .milliseconds(200))
        events.append("replace-end")
        return true
    }
    func removeAll() { events.append("removeAll") }
}

@MainActor
@Test func aClearDuringASlowWriteRunsAfterItAndTheSameVaultIsWrittenAgain() async throws {
    let store = SlowOrderedStore()
    let sync = CredentialIdentitySync(store: store)
    let items = [item("a", urls: ["https://example.com"])]
    sync.update(from: items)
    try await eventually { await store.events == ["replace-start"] }
    sync.clear()
    sync.update(from: items)
    try await eventually { await store.events.count == 5 }
    let events = await store.events
    #expect(events == ["replace-start", "replace-end", "removeAll", "replace-start", "replace-end"])
}

/// A phone with logins in the vault and Face ID turned on, opened from the extension's side.
private struct AutofillPhone {
    let phone: Phone
    let itemIds: [String]

    static func make(logins: [(String, [(String, String)])], faceID: Bool = true) throws -> AutofillPhone {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("figlock-autofill-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let container = VaultContainer(root: root)
        try container.prepare()
        let vault = LocalVault(container: container, access: .readWrite)
        let nowMs: UInt64 = 1_800_000_000_000
        let created = try createVault(
            password: Phone.password, deviceName: "Test iPhone", kdf: minimumKdfSettings(), nowMs: nowMs)
        let store = try vault.openStore()
        try store.putNew(path: "vault.json", bytes: created.vaultJson())
        let keys = created.deviceKeys()
        try store.putNew(path: "devices/\(keys.deviceId()).json", bytes: created.deviceRecord())
        try vault.markVaultPresent()
        let secretKey = try created.secretKeyText()
        guard case .ready(let id, _) = vault.status() else { throw ChipError.failed }
        let phone = Phone(
            container: container, vault: vault, secretKey: secretKey, vaultId: id, deviceKeys: keys)
        try phone.keeper.saveDeviceKeys(keys, vaultId: id)
        try phone.keeper.keep(secretKey, vaultId: id)

        let writer = try created.takeSession()
        var ids: [String] = []
        for (offset, entry) in logins.enumerated() {
            ids.append(
                try writer.addItem(
                    itemType: entry.0, vault: "personal", fields: entry.1.map { FieldInput(name: $0.0, value: $0.1) },
                    nowMs: nowMs + UInt64(offset)))
            if (offset + 1) % 250 == 0 || offset + 1 == logins.count, let file = try writer.commit() {
                try store.putNew(path: file.path, bytes: file.bytes)
            }
        }
        writer.lock()
        if faceID {
            let key = try deriveUnlockKey(
                vaultJson: try store.get(path: "vault.json"), password: Phone.password, secretKey: secretKey)
            try phone.quick.enable(key: key, vaultId: id, deviceId: keys.deviceId())
        }
        return AutofillPhone(phone: phone, itemIds: ids)
    }

    var access: AutofillAccess {
        let readOnly = LocalVault(container: phone.container, access: .readOnly)
        return AutofillAccess(
            vault: readOnly, keeper: phone.keeper, quick: phone.quick, deviceName: "Test iPhone",
            now: { SeededVault.tick() })
    }
}

private func login(_ title: String, user: String, url: String, password: String) -> (String, [(String, String)]) {
    ("login", [("title", title), ("username", user), ("url/a1", url), ("password", password)])
}

@Test func theExtensionOpensWithFaceIDAndFillsTheMatchingLogin() async throws {
    let setup = try AutofillPhone.make(logins: [
        login("Example", user: "ada", url: "https://example.com", password: "generated-pass-1"),
        login("Other", user: "bob", url: "https://other.org", password: "generated-pass-2"),
    ])
    defer { setup.phone.cleanup() }

    let vault = try await setup.access.open(reason: "Fill")
    defer { vault.close() }
    let matches = vault.matches(identifiers: ["login.example.com"])
    #expect(matches.map(\.title) == ["Example"])
    let credential = try vault.credential(itemId: try #require(matches.first).id)
    #expect(credential.username == "ada")
    #expect(credential.password == "generated-pass-1")
    #expect(vault.search("bob").map(\.title) == ["Other"])
    #expect(vault.search("").count == 2)
}

@Test func theExtensionNeverWritesTheVaultFolder() async throws {
    let setup = try AutofillPhone.make(logins: [
        login("Example", user: "ada", url: "https://example.com", password: "generated-pass-1")
    ])
    defer { setup.phone.cleanup() }
    func snapshot() throws -> [String: Date] {
        let folder = setup.phone.container.vaultFolder
        var result: [String: Date] = [:]
        let walker = FileManager.default.enumerator(
            at: folder, includingPropertiesForKeys: [.contentModificationDateKey])
        for case let url as URL in walker ?? NSEnumerator() {
            result[url.path] = try url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate
        }
        return result
    }
    let before = try snapshot()
    let vault = try await setup.access.open(reason: "Fill")
    vault.close()
    #expect(try snapshot() == before)
}

@Test func theExtensionRefusesWithoutFaceID() async throws {
    let setup = try AutofillPhone.make(
        logins: [login("Example", user: "ada", url: "https://example.com", password: "generated-pass-1")],
        faceID: false)
    defer { setup.phone.cleanup() }
    #expect(setup.access.canUnlock() == .needsApp(nil))
    await #expect(throws: AutofillAccess.Failure.needsApp(nil)) { try await setup.access.open(reason: "Fill") }
    #expect(setup.phone.chip.prompts == 0)
}

@Test func theExtensionRefusesAfterAPhoneRestart() async throws {
    let setup = try AutofillPhone.make(logins: [
        login("Example", user: "ada", url: "https://example.com", password: "generated-pass-1")
    ])
    defer { setup.phone.cleanup() }
    var phone = setup.phone
    phone.boot.value = "boot-2"
    let access = setup.access
    guard case .needsApp(let reason)? = access.canUnlock() else {
        Issue.record("a restart should send the person to the app")
        return
    }
    #expect(reason != nil)
}

@Test func aCancelledOrFailedFaceIDPromptOpensNothing() async throws {
    let setup = try AutofillPhone.make(logins: [
        login("Example", user: "ada", url: "https://example.com", password: "generated-pass-1")
    ])
    defer { setup.phone.cleanup() }
    setup.phone.chip.behaviour = .cancelled
    await #expect(throws: AutofillAccess.Failure.cancelled) { try await setup.access.open(reason: "Fill") }
    setup.phone.chip.behaviour = .notRecognised
    await #expect(throws: AutofillAccess.Failure.notRecognised(triesLeft: 4)) {
        try await setup.access.open(reason: "Fill")
    }
}

@Test func anEmptyPhoneHasNoVaultForTheExtension() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("figlock-autofill-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: root) }
    let container = VaultContainer(root: root)
    try container.prepare()
    let secrets = MemorySecrets()
    let access = AutofillAccess(
        vault: LocalVault(container: container, access: .readOnly),
        keeper: SecretKeyKeeper(store: secrets, container: container),
        quick: QuickUnlock(container: container, chip: FakeChip()))
    #expect(access.canUnlock() == .noVault)
}

/// ADR-0041 decision 7: does a full replay fit the extension's memory limit at 10,000 items? The number is
/// printed so it can be recorded in the ADR; the test process also holds the seeding of the vault, so only the growth is bounded here (90 MB, under the roughly 120 MB an extension may use; other tests share the process, so it is noisy); the absolute figure is measured on a device.
@Test func replayingTenThousandItemsInTheExtensionStaysInsideTheMemoryLimit() async throws {
    var logins: [(String, [(String, String)])] = []
    for index in 0..<10_000 {
        logins.append(
            login(
                "Service \(index)", user: "user\(index)@example.com", url: "https://site\(index).example.com",
                password: "generated-pass-\(index)"))
    }
    let setup = try AutofillPhone.make(logins: logins)
    defer { setup.phone.cleanup() }

    let before = Memory.footprintMB()
    let clock = ContinuousClock()
    let started = clock.now
    let vault = try await setup.access.open(reason: "Fill")
    let elapsed = clock.now - started
    let after = Memory.footprintMB()
    #expect(vault.logins.count == 10_000)
    #expect(vault.failedFiles == 0)
    let hit = vault.matches(identifiers: ["site4242.example.com"])
    #expect(hit.count == 1)
    _ = try vault.credential(itemId: try #require(hit.first).id)
    let end = Memory.footprintMB()
    vault.close()
    print("FIG-166 extension replay at 10000 items: \(elapsed), footprint \(before) MB -> \(after) MB -> \(end) MB")
    #expect(max(after, end) - before < 90)
}

enum Memory {
    /// The process's physical footprint, the number iOS compares with the extension limit.
    static func footprintMB() -> Int {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? Int(info.phys_footprint / 1_048_576) : -1
    }
}
