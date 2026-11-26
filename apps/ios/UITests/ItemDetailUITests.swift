import XCTest

/// Runs the debug build against the made-up demo vault (`-figlock-seed-demo -figlock-unlock-demo`).
@MainActor
final class ItemDetailUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo"]
        app.launch()
    }

    private func openItem() {
        let row = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Fastmail'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15))
        row.tap()
    }

    // FIG-201: the item actions menu is enabled on the detail screen and offers Edit and Delete.
    func testItemActionsAreReachableFromDetail() {
        openItem()
        let menu = app.buttons["Item actions"]
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        XCTAssertTrue(menu.isEnabled)
        menu.tap()
        XCTAssertTrue(app.buttons["Edit"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Delete"].exists)
    }

    // FIG-202: searching and then opening an item leaves no keyboard behind.
    func testKeyboardIsGoneAfterOpeningItemFromSearch() {
        let search = app.textFields["vault-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 15))
        search.tap()
        search.typeText("fast")
        XCTAssertTrue(app.keyboards.firstMatch.exists)
        openItem()
        XCTAssertTrue(app.buttons["Item actions"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.keyboards.firstMatch.waitForExistence(timeout: 2))
    }

    // FIG-220: the edit sheet title uses the display name of the kind, not its raw value.
    func testEditTitleUsesKindDisplayName() {
        openItem()
        app.buttons["Item actions"].tap()
        app.buttons["Edit"].tap()
        XCTAssertTrue(app.navigationBars["Edit Login"].waitForExistence(timeout: 5))
    }
}
