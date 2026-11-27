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

    // FIG-216: a rejected password does not stay in the field.
    // FIG-219: one tap on Unlock is enough while the keyboard is up.
    func testWrongPasswordIsClearedAfterOneTapOnUnlock() {
        let field = app.secureTextFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        field.tap()
        field.typeText("not-the-password")
        XCTAssertTrue(app.keyboards.firstMatch.exists)
        app.buttons["Unlock"].tap()
        XCTAssertTrue(
            app.staticTexts.matching(NSPredicate(format: "label CONTAINS \"isn't right\"")).firstMatch
                .waitForExistence(timeout: 15))
        XCTAssertTrue(app.buttons["Unlock"].exists)
        XCTAssertFalse(app.buttons["Unlock"].isEnabled, "the empty field disables Unlock")
        XCTAssertEqual(app.secureTextFields.firstMatch.value as? String ?? "", "")
    }

    // FIG-220: the attempt that starts the pause clears the field too.
    func testPasswordIsClearedWhenTheAttemptStartsThePause() {
        for attempt in 1...3 {
            let field = app.secureTextFields.firstMatch
            if attempt == 1 { XCTAssertTrue(field.waitForExistence(timeout: 15)) }
            field.tap()
            field.typeText("not-the-password")
            app.buttons["Unlock"].tap()
            if attempt < 3 {
                XCTAssertTrue(
                    app.staticTexts.matching(NSPredicate(format: "label CONTAINS \"isn't right\"")).firstMatch
                        .waitForExistence(timeout: 15))
            }
        }
        XCTAssertTrue(
            app.staticTexts.matching(NSPredicate(format: "label CONTAINS \"Too many tries\"")).firstMatch
                .waitForExistence(timeout: 15))
        XCTAssertEqual(app.secureTextFields.firstMatch.value as? String ?? "", "")
    }
}
