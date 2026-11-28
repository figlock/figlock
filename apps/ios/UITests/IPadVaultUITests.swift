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

    // FIG-267: the delete confirmation is a popover pointing at the actions button, not a bottom sheet.
    func testTheDeleteConfirmationIsAPopover() {
        row("Fastmail").tap()
        app.buttons["Item actions"].tap()
        app.buttons["Delete"].tap()
        XCTAssertTrue(app.popovers.firstMatch.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Delete"].exists)
    }

    // FIG-267: the generator opens as a popover from the toolbar button, and from the password field of the editor.
    func testTheGeneratorIsAPopover() {
        XCTAssertTrue(app.buttons["Password generator"].waitForExistence(timeout: 15))
        app.buttons["Password generator"].tap()
        XCTAssertTrue(app.popovers.firstMatch.waitForExistence(timeout: 5))
        XCTAssertTrue(app.navigationBars["Generator"].exists)
    }

    // FIG-267: a new item is a form sheet over the vault, and the generator pops over the form.
    func testANewItemIsAFormSheetWithAGeneratorPopover() {
        XCTAssertTrue(app.buttons["Add item"].waitForExistence(timeout: 15))
        app.buttons["Add item"].tap()
        app.buttons["Login"].tap()
        XCTAssertTrue(app.navigationBars["New Login"].waitForExistence(timeout: 5))
        // A form sheet is narrower than the window; a full height page would fill it.
        XCTAssertLessThan(app.navigationBars["New Login"].frame.width, app.windows.firstMatch.frame.width - 100)
        app.buttons["Generate a password"].tap()
        XCTAssertTrue(app.popovers.firstMatch.waitForExistence(timeout: 5))
    }
}

/// Onboarding and unlock are one centred column on an iPad, not stretched across the window (ADR-0048 decision 3).
@MainActor
final class IPadFormColumnUITests: XCTestCase {
    override func setUpWithError() throws {
        try XCTSkipUnless(UIDevice.current.userInterfaceIdiom == .pad, "iPad layout")
        continueAfterFailure = false
        XCUIDevice.shared.orientation = .landscapeLeft
    }

    override func tearDown() {
        XCUIDevice.shared.orientation = .portrait
    }

    func testWelcomeIsACappedColumn() {
        let app = XCUIApplication()
        app.launchArguments = ["-figlock-fresh"]
        app.launch()
        let choice = app.buttons["Create a new vault"]
        XCTAssertTrue(choice.waitForExistence(timeout: 15))
        XCTAssertLessThanOrEqual(choice.frame.width, 540)
        XCTAssertLessThan(choice.frame.width, app.windows.firstMatch.frame.width / 2)
    }
}
