import Foundation
import Testing

@testable import Shared

private func item(_ title: String, _ type: String, tags: [String] = []) -> (String, [(String, String)]) {
    (type, [("title", title)] + tags.map { ("tags/\($0)", "1") })
}

@MainActor
@Test func scopesListTheRightItemsAndTheSidebarCountsThem() async throws {
    let seeded = try SeededVault.make([
        item("Fastmail", "login", tags: ["work", "email"]),
        item("Linear", "login", tags: ["work"]),
        item("Stripe key", "token", tags: ["home"]),
        item("Router", "note"),
    ])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()

    #expect(browser.rows(query: "", scope: .all).count == 4)
    #expect(browser.rows(query: "", scope: .kind(.login)).count == 2)
    #expect(browser.rows(query: "", scope: .otherKinds).map(\.title) == ["Stripe key"])
    #expect(Set(browser.rows(query: "", scope: .tag("work")).map(\.title)) == ["Fastmail", "Linear"])
    // A scope and a search narrow together.
    #expect(browser.rows(query: "linear", scope: .tag("work")).map(\.title) == ["Linear"])
    #expect(browser.rows(query: "linear", scope: .tag("email")).isEmpty)
    #expect(browser.tagCounts.map(\.tag) == ["email", "home", "work"])
    #expect(browser.tagCounts.first { $0.tag == "work" }?.count == 2)
    #expect(browser.count(in: .kind(.login)) == 2)
    #expect(browser.count(in: .deleted) == 0)
}

@MainActor
@Test func theNavigationKeepsItsStateAndForgetsAnAnchorWhenTheListChanges() {
    let navigation = VaultNavigation()
    navigation.selectedItemId = "a"
    navigation.scrollAnchor = "z"
    navigation.query = "fast"
    // A window that goes narrow and wide again reads the same model: nothing is reset by a layout change.
    #expect(navigation.selectedItemId == "a")
    #expect(navigation.scrollAnchor == "z")
    // Another list has other rows: the anchor goes, the selection stays (it may still be in the new list).
    navigation.scope = .kind(.login)
    #expect(navigation.scrollAnchor == nil)
    #expect(navigation.selectedItemId == "a")
    #expect(navigation.isSearching)
}

@MainActor
@Test func aSelectionThatIsGoneIsDropped() async throws {
    let seeded = try SeededVault.make([item("Fastmail", "login")])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()
    let navigation = VaultNavigation()
    navigation.selectedItemId = seeded.itemIds[0]
    navigation.clearSelectionIfMissing(in: browser)
    #expect(navigation.selectedItemId == seeded.itemIds[0])
    navigation.selectedItemId = "no-such-item"
    navigation.clearSelectionIfMissing(in: browser)
    #expect(navigation.selectedItemId == nil)
}

@Test func compactLayoutHasNoTrashChip() {
    #expect(VaultScope.deleted.forCompact == .all)
    #expect(VaultScope.tag("work").forCompact == .tag("work"))
    #expect(VaultScope.chips.map(\.title) == ["All", "Logins", "Notes", "Other"])
}
