import FigLockFFI
import Foundation
import Testing

@testable import Shared

// FIG-286: importing an export file. Values are generated; nothing here is a real password.

private func chromeCSV(_ count: Int) -> Data {
    var text = "name,url,username,password,note\n"
    for index in 0..<count {
        text += "Site \(index),https://s\(index).example,me,generated-import-pw-\(index),\n"
    }
    return Data(text.utf8)
}

@MainActor
private func editor(_ seeded: SeededVault) async throws -> VaultEditor {
    let browser = await seeded.browser()
    let container = seeded.container
    return VaultEditor(
        browser: browser, store: try seeded.vault.openStore(), pending: PendingSync(container: container),
        now: { SeededVault.tick() })
}

@MainActor
@Test func previewingWritesNothingAndImportingAddsEveryItemAsNotSynced() async throws {
    let seeded = try SeededVault.make([])
    defer { seeded.cleanup() }
    let editor = try await editor(seeded)
    let plan = try prepareImport(source: "chrome", bytes: chromeCSV(250))
    #expect(plan.preview().logins == 250)
    #expect(try seeded.session.items(includeDeleted: false).items.isEmpty)

    // 250 items go in three op files of at most 100.
    let added = try await editor.importItems(plan)
    #expect(added == 250)
    let items = try seeded.session.items(includeDeleted: false).items
    #expect(items.count == 250)
    #expect(items.allSatisfy { editor.isUnsynced($0.id) })
}

@MainActor
@Test func aFileThatIsNotThatExportIsRefusedWithAFixedReason() throws {
    let secret = "generated-import-pw-0"
    #expect(throws: ImportFailure.WrongFormat) {
        try prepareImport(source: "apple", bytes: chromeCSV(1))
    }
    #expect(throws: ImportFailure.Empty) {
        try prepareImport(source: "chrome", bytes: Data("name,url,username,password,note\n".utf8))
    }
    #expect(!"\(ImportFailure.WrongFormat)".contains(secret))
}

@MainActor
@Test func aNotesFileBecomesNotes() throws {
    let plan = try prepareImport(source: "notes", bytes: Data("One\nbody\n---\nTwo\nbody\n".utf8))
    #expect(plan.preview().notes == 2)
}
