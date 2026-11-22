import FigLockFFI
import Foundation
import Testing

@testable import Shared

/// A store that cannot be written, to check that a failed save changes nothing.
private final class FailingStore: BlobStoreFfi, @unchecked Sendable {
    let wrapping: any BlobStoreFfi
    init(wrapping: any BlobStoreFfi) { self.wrapping = wrapping }
    func get(path: String) throws -> Data { try wrapping.get(path: path) }
    func list(prefix: String) throws -> [BlobEntry] { try wrapping.list(prefix: prefix) }
    func putNew(path: String, bytes: Data) throws { throw StoreFailure.Other }
    func putReplace(path: String, bytes: Data) throws { throw StoreFailure.Other }
    func delete(path: String) throws { throw StoreFailure.Other }
}

private struct Rig {
    let seeded: SeededVault
    let browser: VaultBrowser
    let editor: VaultEditor
    let store: any BlobStoreFfi

    @MainActor
    static func make(
        _ items: [(String, [(String, String)])] = [], failing: Bool = false, lastSync: UInt64? = nil
    ) async throws -> Rig {
        let seeded = try SeededVault.make(items)
        let browser = await seeded.browser()
        var store = try seeded.vault.openStore()
        if failing { store = FailingStore(wrapping: store) }
        let editor = VaultEditor(
            browser: browser, store: store, pending: PendingSync(container: seeded.container),
            lastSyncMs: { lastSync }, now: { SeededVault.tick() })
        return Rig(seeded: seeded, browser: browser, editor: editor, store: store)
    }

    func opFiles() throws -> [String] { try store.list(prefix: "log/").map(\.path) }
}

@MainActor
@Test func aNewLoginIsCommittedStoredAndShownAsNotSynced() async throws {
    let rig = try await Rig.make()
    defer { rig.seeded.cleanup() }
    var draft = ItemDraft(kind: .login)
    draft.title = "Fastmail"
    draft.values["username"] = "mira@example.com"
    draft.values["password"] = "generated-pass-1"
    draft.urls = [DraftEntry(id: "web1", value: "https://app.fastmail.com")]
    draft.tagsText = "Work, mail"
    let id = try await rig.editor.create(draft)

    let item = try #require(rig.browser.item(id: id))
    #expect(item.kind == .login && item.title == "Fastmail" && item.subtitle == "mira@example.com")
    #expect(item.websites == ["https://app.fastmail.com"])
    #expect(item.tags == ["mail", "work"])
    #expect(item.secretFields == ["password"])
    #expect(try rig.seeded.session.revealField(itemId: id, field: "password") == "generated-pass-1")
    #expect(try rig.opFiles().count == 1)
    #expect(try !rig.seeded.session.hasPending())
    #expect(rig.editor.isUnsynced(id))
    #expect(rig.browser.items.map(\.id) == [id])

    // Reading the log again does not apply this device's own file a second time.
    await rig.browser.reload()
    #expect(rig.browser.loadFailures.isEmpty)
    #expect(rig.browser.items.count == 1)
}

@MainActor
@Test func everyItemKindCanBeCreated() async throws {
    let rig = try await Rig.make()
    defer { rig.seeded.cleanup() }

    var token = ItemDraft(kind: .token)
    token.title = "GitHub"
    token.values["provider"] = "github"
    token.values["token"] = "fl_demo_generated"
    token.values["env_var"] = "GITHUB_TOKEN"
    token.allowedHosts = [DraftEntry(id: "h1", value: "api.github.com")]
    var note = ItemDraft(kind: .note)
    note.title = "Wifi"
    note.values["body"] = "line one\nline two"
    var env = ItemDraft(kind: .env)
    env.title = "Staging"
    env.env = [DraftEntry(id: "a", name: "API_KEY", value: "generated-env-1")]
    var ssh = ItemDraft(kind: .ssh)
    ssh.title = "Deploy key"
    ssh.values["private_key"] = "generated-private-key"
    ssh.values["public_key"] = "ssh-ed25519 AAAA"
    var doc = ItemDraft(kind: .document)
    doc.title = "Signing"
    doc.values["password"] = "generated-p12-pass"
    doc.custom = [DraftEntry(id: "c1", name: "Team", value: "generated-custom")]

    var ids: [ItemKind: String] = [:]
    for draft in [token, note, env, ssh, doc] { ids[draft.kind] = try await rig.editor.create(draft) }

    #expect(rig.browser.items.count == 5)
    let tokenItem = try #require(rig.browser.item(id: ids[.token] ?? ""))
    #expect(tokenItem.secretFields == ["token"])
    #expect(tokenItem.detailRows.contains { $0.label == "Allowed host" && $0.value == "api.github.com" })
    #expect(try rig.seeded.session.revealField(itemId: ids[.note] ?? "", field: "body") == "line one\nline two")
    let envItem = try #require(rig.browser.item(id: ids[.env] ?? ""))
    #expect(envItem.secretFields == ["env/API_KEY"])
    #expect(envItem.kind == .env)
    #expect(rig.browser.item(id: ids[.ssh] ?? "")?.kind == .ssh)
    let docItem = try #require(rig.browser.item(id: ids[.document] ?? ""))
    #expect(docItem.kind == .document)
    #expect(Set(docItem.secretFields) == ["password", "custom/c1/value"])
    #expect(docItem.fields["custom/c1/name"] == "Team")
    #expect(try rig.opFiles().count == 5)
}

@MainActor
@Test func editingWritesOnlyWhatChangedAndKeepsUntouchedSecrets() async throws {
    let rig = try await Rig.make([
        (
            "login",
            [
                ("title", "Fastmail"), ("username", "mira@example.com"), ("url/a1", "https://old.example.com"),
                ("password", "generated-pass-1"), ("tags/work", "1"),
            ]
        )
    ])
    defer { rig.seeded.cleanup() }
    let original = try #require(rig.browser.items.first)

    var draft = ItemDraft(editing: original)
    #expect(draft.changes().isEmpty)
    draft.values["username"] = "mira@work.example"
    draft.urls[0].value = "https://new.example.com"
    draft.tagsText = "home"
    let names = Set(draft.changes().map(\.name))
    #expect(names == ["username", "url/a1", "tags/home", "tags/work"])
    try await rig.editor.update(draft)

    let edited = try #require(rig.browser.item(id: original.id))
    #expect(edited.fields["username"] == "mira@work.example")
    #expect(edited.websites == ["https://new.example.com"])
    #expect(edited.tags == ["home"])
    // The password was not typed again, so it is unchanged.
    #expect(try rig.seeded.session.revealField(itemId: original.id, field: "password") == "generated-pass-1")
    #expect(rig.editor.isUnsynced(original.id))

    var replace = ItemDraft(editing: edited)
    replace.values["password"] = "generated-pass-2"
    try await rig.editor.update(replace)
    #expect(try rig.seeded.session.revealField(itemId: original.id, field: "password") == "generated-pass-2")

    var remove = ItemDraft(editing: try #require(rig.browser.item(id: original.id)))
    remove.values["username"] = ""
    remove.removedSecrets = ["password"]
    try await rig.editor.update(remove)
    let stripped = try #require(rig.browser.item(id: original.id))
    #expect(stripped.fields["username"] == nil && stripped.secretFields.isEmpty)

    // An edit with nothing changed writes no file.
    let before = try rig.opFiles().count
    try await rig.editor.update(ItemDraft(editing: stripped))
    #expect(try rig.opFiles().count == before)
}

@MainActor
@Test func aDeletedItemLeavesTheListAndComesBackWhenRestored() async throws {
    let rig = try await Rig.make([("note", [("title", "Old note"), ("body", "generated body")])])
    defer { rig.seeded.cleanup() }
    let id = try #require(rig.browser.items.first?.id)

    try await rig.editor.delete(id: id)
    #expect(rig.browser.items.isEmpty)
    #expect(rig.browser.deletedItems.map(\.id) == [id])
    #expect(rig.browser.deletedItems.first?.isDeleted == true)
    #expect(rig.editor.isUnsynced(id))

    try await rig.editor.restore(id: id)
    #expect(rig.browser.items.map(\.id) == [id])
    #expect(rig.browser.deletedItems.isEmpty)
    #expect(try rig.seeded.session.revealField(itemId: id, field: "body") == "generated body")
    await rig.browser.reload()
    #expect(rig.browser.loadFailures.isEmpty)
}

@MainActor
@Test func aDocumentKeepsItsFilesAndGivesThemBackAfterFaceID() async throws {
    let rig = try await Rig.make()
    defer { rig.seeded.cleanup() }
    var draft = ItemDraft(kind: .document)
    draft.title = "App Store key"
    let bytes = Data((0..<2_000).map { UInt8($0 % 251) })
    try draft.attach(DraftFile(id: draft.newFileId(), name: "AuthKey.p8", mime: "application/x-pkcs8", data: bytes))
    let id = try await rig.editor.create(draft)

    let item = try #require(rig.browser.item(id: id))
    let file = try #require(item.files.first)
    #expect(file.name == "AuthKey.p8" && file.mime == "application/x-pkcs8")
    #expect(item.textSecretFields.isEmpty)
    #expect(item.secretFields == [file.dataField])
    // Search finds a file by its name; its contents are not indexed.
    #expect(try rig.seeded.session.search(query: "AuthKey", nowMs: 1_800_000_100_000, limit: 5).count == 1)

    let allowed = FileExtraction(
        session: rig.browser.session, itemId: id, authenticator: FakeAuthenticator(allow: true))
    #expect(await allowed.contents(of: file) == bytes)
    let denied = FileExtraction(
        session: rig.browser.session, itemId: id, authenticator: FakeAuthenticator(allow: false))
    #expect(await denied.contents(of: file) == nil)

    // Dropping the file removes its three fields; adding another keeps the first.
    var edit = ItemDraft(editing: item)
    try edit.attach(
        DraftFile(id: edit.newFileId(), name: "second.pdf", mime: "application/pdf", data: Data([1, 2, 3])))
    try await rig.editor.update(edit)
    #expect(Set(try #require(rig.browser.item(id: id)).files.map(\.name)) == ["AuthKey.p8", "second.pdf"])
    var drop = ItemDraft(editing: try #require(rig.browser.item(id: id)))
    drop.files.removeAll { $0.name == "AuthKey.p8" }
    try await rig.editor.update(drop)
    let left = try #require(rig.browser.item(id: id))
    #expect(left.files.map(\.name) == ["second.pdf"])
    #expect(left.secretFields.count == 1)
}

@Test func theFileLimitsOfADocumentAreChecked() throws {
    var draft = ItemDraft(kind: .document)
    #expect(throws: ItemDraftError.fileTooLarge("big.bin")) {
        try draft.attach(
            DraftFile(
                id: "a", name: "big.bin", mime: "application/octet-stream",
                data: Data(count: ItemDraft.maxFileBytes + 1)))
    }
    for index in 0..<ItemDraft.maxFiles {
        try draft.attach(DraftFile(id: "f\(index)", name: "f\(index)", mime: "text/plain", data: Data([1])))
    }
    #expect(throws: ItemDraftError.tooManyFiles) {
        try draft.attach(DraftFile(id: "x", name: "x", mime: "text/plain", data: Data([1])))
    }
    // The largest allowed file encodes to a value inside the 1 MiB ceiling of a field.
    #expect(Data(count: ItemDraft.maxFileBytes).base64EncodedString().utf8.count == 1024 * 1024)
}

@Test func theFilesPickerReadsANameAMediaTypeAndTheBytes() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let small = directory.appendingPathComponent("notes.txt")
    try Data("generated".utf8).write(to: small)
    let file = try DocumentFiles.read(small, id: "ab12")
    #expect(file.name == "notes.txt" && file.mime == "text/plain" && file.data == Data("generated".utf8))

    let big = directory.appendingPathComponent("big.bin")
    try Data(count: ItemDraft.maxFileBytes + 1).write(to: big)
    #expect(throws: ItemDraftError.fileTooLarge("big.bin")) { try DocumentFiles.read(big, id: "cd34") }
    #expect(throws: ItemDraftError.unreadableFile("missing.txt")) {
        try DocumentFiles.read(directory.appendingPathComponent("missing.txt"), id: "ef56")
    }
}

@MainActor
@Test func aSaveThatCannotBeWrittenChangesNothing() async throws {
    let rig = try await Rig.make(
        [("note", [("title", "Keep"), ("body", "generated body")])], failing: true)
    defer { rig.seeded.cleanup() }
    let original = try #require(rig.browser.items.first)

    var draft = ItemDraft(kind: .login)
    draft.title = "New"
    await #expect(throws: EditFailure.storage) { try await rig.editor.create(draft) }
    var edit = ItemDraft(editing: original)
    edit.values["body"] = "changed"
    await #expect(throws: EditFailure.storage) { try await rig.editor.update(edit) }
    await #expect(throws: EditFailure.storage) { try await rig.editor.delete(id: original.id) }

    #expect(try !rig.seeded.session.hasPending())
    #expect(rig.browser.items == [original])
    #expect(try rig.seeded.session.revealField(itemId: original.id, field: "body") == "generated body")
    #expect(!rig.editor.isUnsynced(original.id))
}

@MainActor
@Test func aValueTheCoreRefusesIsReportedAndLeavesNothingBehind() async throws {
    let rig = try await Rig.make()
    defer { rig.seeded.cleanup() }
    var draft = ItemDraft(kind: .login)
    draft.title = "Too long"
    draft.values["username"] = String(repeating: "u", count: 5_000)
    await #expect(throws: EditFailure.rejected) { try await rig.editor.create(draft) }
    #expect(try !rig.seeded.session.hasPending())
    #expect(rig.browser.items.isEmpty)
    #expect(try rig.opFiles().isEmpty)

    // The session still takes a good edit afterwards.
    draft.values["username"] = "ok"
    _ = try await rig.editor.create(draft)
    #expect(rig.browser.items.count == 1)
}

@MainActor
@Test func anItemStaysNotSyncedUntilASyncFinishesAfterTheEdit() async throws {
    let seeded = try SeededVault.make([])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()
    let pending = PendingSync(container: seeded.container)
    let state = LastSync()
    let editor = VaultEditor(
        browser: browser, store: try seeded.vault.openStore(), pending: pending,
        lastSyncMs: { state.value }, now: { SeededVault.tick() })
    var draft = ItemDraft(kind: .note)
    draft.title = "Pending"
    let id = try await editor.create(draft)
    #expect(editor.isUnsynced(id))

    state.value = 1_700_000_000_000  // a sync from before the edit
    editor.refreshUnsynced()
    #expect(editor.isUnsynced(id))
    state.value = 1_900_000_000_000
    editor.refreshUnsynced()
    #expect(!editor.isUnsynced(id))
    // It is remembered across launches.
    #expect(PendingSync(container: seeded.container).pendingIds(lastSyncMs: nil) == [])
}

private final class LastSync: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: UInt64?
    var value: UInt64? {
        get { lock.withLock { stored } }
        set { lock.withLock { stored = newValue } }
    }
}

@Test func theDraftChecksWhatTheCoreWouldRefuse() {
    var draft = ItemDraft(kind: .env)
    #expect(draft.problem == "Give the item a name.")
    draft.title = "Staging"
    draft.env = [DraftEntry(id: "1", name: "api key", value: "x")]
    #expect(draft.problem?.contains("capital letters") == true)
    draft.env = [DraftEntry(id: "1", name: "API_KEY", value: "x"), DraftEntry(id: "2", name: "API_KEY", value: "y")]
    #expect(draft.problem == "API_KEY appears twice.")
    draft.env = [DraftEntry(id: "1", name: "API_KEY", value: "x")]
    #expect(draft.problem == nil)
    draft.tagsText = "ok, bad tag!"
    #expect(draft.problem == "Tags use letters, digits, - and _ only.")
    draft.tagsText = "Prod #ops, prod"
    #expect(draft.tags == ["prod", "ops"])
    #expect(draft.problem == nil)
}

@Test func wireNamesFollowTheVaultSchema() {
    #expect(ItemKind.document.wire == "doc")
    #expect(ItemKind.allCases.allSatisfy { ItemKind(wire: $0.wire) == $0 })
    for kind in ItemKind.allCases {
        for spec in ItemDraft.fields(for: kind) { #expect(!spec.label.isEmpty && !spec.name.isEmpty) }
    }
}

@Test func theGeneratorFollowsItsSettings() throws {
    var settings = GeneratorSettings()
    let password = try #require(settings.generate())
    #expect(password.count == 20)
    #expect(GeneratorSettings.rating(of: password) == .strong)
    #expect(GeneratorSettings.rating(of: "password1") == .weak)
    #expect(GeneratorSettings.rating(of: "") == nil)

    settings.length = 32
    settings.upper = false
    settings.symbols = false
    settings.avoidLookalikes = true
    let plain = try #require(settings.generate())
    #expect(plain.count == 32)
    #expect(plain.allSatisfy { $0.isLowercase || $0.isNumber })
    #expect(!plain.contains(where: { "l1o0".contains($0) }))

    settings.lower = false
    settings.digits = false
    #expect(!settings.isValid && settings.generate() == nil)
    settings = GeneratorSettings()
    settings.length = 4
    #expect(settings.generate() == nil)

    settings = GeneratorSettings()
    settings.mode = .passphrase
    settings.words = 5
    settings.separator = .dot
    settings.capitalize = true
    let phrase = try #require(settings.generate())
    let words = phrase.split(separator: ".")
    #expect(words.count == 5 && words.allSatisfy { $0.first?.isUppercase == true })
    #expect(phrase != settings.generate())
}
