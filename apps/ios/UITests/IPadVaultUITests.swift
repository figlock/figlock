import XCTest

/// The iPad vault screen (FIG-266, ADR-0048): sidebar, list and detail on the made-up demo vault. These need a regular
/// width window, so they skip on an iPhone. `just ios-uitest` runs on an iPhone; run these with an iPad simulator.
@MainActor
final class IPadVaultUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        try XCTSkipUnless(UIDevice.current.userInterfaceIdiom == .pad, "iPad layout")
        continueAfterFailure = false
        XCUIDevice.shared.orientation = .landscapeLeft
        app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo"]
        app.launch()
    }

    override func tearDown() {
        XCUIDevice.shared.orientation = .portrait
    }

    /// An item row of the list column (the row is one accessibility element, "title, kind, subtitle").
    private func row(_ title: String) -> XCUIElement {
        app.otherElements.matching(NSPredicate(format: "label BEGINSWITH %@", "\(title),")).firstMatch
    }

    /// A sidebar row ("Logins, 2").
    private func sidebar(_ title: String) -> XCUIElement {
        app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@", "\(title),")).firstMatch
    }

    private func attach(_ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }

    func testNothingSelectedShowsAnEmptyDetailThenAnItemFillsIt() {
        XCTAssertTrue(app.staticTexts["No item selected"].waitForExistence(timeout: 15))
        row("Fastmail").tap()
        XCTAssertTrue(app.buttons["Item actions"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["No item selected"].exists)
        attach("ipad-landscape-selected")
    }

    func testTheSidebarFiltersTheListAndSearchNarrowsIt() {
        XCTAssertTrue(row("Fastmail").waitForExistence(timeout: 15))
        XCTAssertTrue(row("Stripe API key").exists)
        sidebar("Logins").tap()
        XCTAssertTrue(row("Fastmail").waitForExistence(timeout: 5))
        XCTAssertFalse(row("Stripe API key").exists)
        let search = app.searchFields.firstMatch
        search.tap()
        search.typeText("linear")
        XCTAssertTrue(row("Linear").waitForExistence(timeout: 5))
        XCTAssertFalse(row("Fastmail").exists)
    }

    func testTheSelectionSurvivesARotation() {
        row("Fastmail").tap()
        XCTAssertTrue(app.buttons["Item actions"].waitForExistence(timeout: 10))
        XCUIDevice.shared.orientation = .portrait
        XCTAssertTrue(app.buttons["Item actions"].waitForExistence(timeout: 10))
        attach("ipad-portrait-selected")
        XCUIDevice.shared.orientation = .landscapeLeft
        XCTAssertTrue(app.buttons["Item actions"].waitForExistence(timeout: 10))
    }

    func testSettingsOpensFromTheFootOfTheSidebar() {
        XCTAssertTrue(app.buttons["sidebar-settings"].waitForExistence(timeout: 15))
        app.buttons["sidebar-settings"].tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
    }

    func testDeletingTheSelectedItemEmptiesTheDetail() {
        row("Fastmail").tap()
        app.buttons["Item actions"].tap()
        app.buttons["Delete"].tap()
        app.buttons["Delete"].tap()
        XCTAssertTrue(app.staticTexts["No item selected"].waitForExistence(timeout: 10))
    }
}
