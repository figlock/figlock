import Foundation
import Testing

@testable import Shared

@Test func deletedAgeIsCountedInCalendarDays() {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "UTC")!
    let now = Date(timeIntervalSince1970: 1_700_000_000 + 3600 * 12)
    func text(daysAgo: Double) -> String {
        DeletedItemAge.text(deleted: now.addingTimeInterval(-daysAgo * 86_400), now: now, calendar: calendar)
    }
    #expect(text(daysAgo: 0) == "Deleted today")
    #expect(text(daysAgo: 1) == "Deleted yesterday")
    #expect(text(daysAgo: 12) == "Deleted 12 days ago")
    // A clock a little behind another device's never shows a negative age.
    #expect(text(daysAgo: -3) == "Deleted today")
}

@MainActor
@Test func recoverAllRestoresEveryDeletedItemInOneWrite() async throws {
    let seeded = try SeededVault.make([
        ("note", [("title", "First"), ("body", "generated one")]),
        ("note", [("title", "Second"), ("body", "generated two")]),
    ])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()
    let store = try seeded.vault.openStore()
    let editor = VaultEditor(
        browser: browser, store: store, pending: PendingSync(container: seeded.container),
        lastSyncMs: { nil }, now: { SeededVault.tick() })
    let ids = browser.items.map(\.id)
    #expect(ids.count == 2)
    for id in ids { try await editor.delete(id: id) }
    #expect(browser.items.isEmpty)

    let before = try store.list(prefix: "log/").count
    try await editor.restoreAll(ids: browser.deletedItems.map(\.id))
    #expect(Set(browser.items.map(\.id)) == Set(ids))
    #expect(browser.deletedItems.isEmpty)
    #expect(try store.list(prefix: "log/").count == before + 1)

    // Nothing to recover writes nothing.
    try await editor.restoreAll(ids: [])
    #expect(try store.list(prefix: "log/").count == before + 1)
}
