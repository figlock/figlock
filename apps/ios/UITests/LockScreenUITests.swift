import XCTest

/// Runs the debug build against the made-up demo vault, left locked (`-figlock-seed-demo`).
@MainActor
final class LockScreenUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo"]
        app.launch()
    }

    // FIG-216: a rejected master password does not stay in the field.
    // FIG-219: one tap on Unlock is enough while the keyboard is up.
    func testWrongMasterPasswordIsClearedAfterOneTapOnUnlock() {
        let field = app.secureTextFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        field.tap()
        field.typeText("not-the-password")
        XCTAssertTrue(app.keyboards.firstMatch.exists)
        app.buttons["Unlock with master password"].tap()
        XCTAssertTrue(
            app.staticTexts.matching(NSPredicate(format: "label CONTAINS \"isn't right\"")).firstMatch
                .waitForExistence(timeout: 15))
        XCTAssertTrue(app.buttons["Unlock with master password"].exists)
        XCTAssertFalse(app.buttons["Unlock with master password"].isEnabled, "the empty field disables Unlock")
        XCTAssertEqual(app.secureTextFields.firstMatch.value as? String ?? "", "")
    }
}
