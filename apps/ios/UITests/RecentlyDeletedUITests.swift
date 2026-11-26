import XCTest

/// FIG-252: Recently deleted is a screen pushed from the vault list, with Restore on each item and a menu with
/// Recover all. Runs against the made-up demo vault.
@MainActor
final class RecentlyDeletedUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo"]
        app.launch()
    }

    private func row(_ title: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title)).firstMatch
    }

    private func delete(_ title: String) {
        let item = row(title)
        XCTAssertTrue(item.waitForExistence(timeout: 15))
        item.tap()
        app.buttons["Item actions"].tap()
        app.buttons["Delete"].tap()
        app.buttons["Delete"].firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Vault"].waitForExistence(timeout: 10))
    }

    private func openRecentlyDeleted() {
        app.buttons["More"].tap()
        app.buttons["Recently deleted"].tap()
        XCTAssertTrue(app.navigationBars["Recently deleted"].waitForExistence(timeout: 10))
    }

    func testRestoreAndRecoverAll() {
        delete("Fastmail")
        delete("GitHub token")
        openRecentlyDeleted()

        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS 'Deleted today'")).firstMatch.exists)
        app.buttons["Restore Fastmail"].tap()
        XCTAssertFalse(app.buttons["Restore Fastmail"].waitForExistence(timeout: 3))
        XCTAssertTrue(app.buttons["Restore GitHub token"].exists)

        app.buttons["Recently deleted actions"].tap()
        app.buttons["Recover all"].tap()
        XCTAssertTrue(app.staticTexts["Nothing deleted"].waitForExistence(timeout: 10))

        app.navigationBars["Recently deleted"].buttons["Vault"].tap()
        XCTAssertTrue(row("Fastmail").waitForExistence(timeout: 10))
        XCTAssertTrue(row("GitHub token").exists)
    }
}
