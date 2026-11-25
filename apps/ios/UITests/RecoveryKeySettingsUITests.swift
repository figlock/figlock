import XCTest

/// FIG-237: Settings > Recovery Key against the made-up demo vault (`-figlock-seed-demo -figlock-unlock-demo`),
/// which starts without a Recovery Key and keeps its Secret Key. Every value here is made up.
@MainActor
final class RecoveryKeySettingsUITests: XCTestCase {
    private static let password = "demo-password-1"
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo", "-figlock-expose-new-key"]
        app.launch()
    }

    private func openRecoveryKey() {
        let settings = app.buttons["Settings"]
        XCTAssertTrue(settings.waitForExistence(timeout: 20))
        settings.tap()
        let row = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Recovery Key'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.tap()
        XCTAssertTrue(app.navigationBars["Recovery Key"].waitForExistence(timeout: 10))
    }

    /// The list behind the sheet has a button of the same name; the sheet's is the last one.
    private func sheetTurnOffButton() -> XCUIElement {
        let buttons = app.buttons.matching(identifier: "Turn off the Recovery Key")
        return buttons.element(boundBy: buttons.count - 1)
    }

    private func type(_ text: String, into field: XCUIElement) {
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        field.typeText(text)
    }

    func testTurningTheRecoveryKeyOnChecksTheCodeAndThePasswordThenTurnsItOff() {
        openRecoveryKey()
        XCTAssertTrue(app.staticTexts["Off"].exists)
        app.buttons["Set up a Recovery Key"].tap()

        // Step 1: the code is shown, and the person has to open the page before going on.
        XCTAssertTrue(app.staticTexts["Step 1 of 2: save it"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["I saved it, continue"].isEnabled)
        app.buttons["Save the Recovery Key page"].tap()
        app.buttons["Close"].firstMatch.tap()
        app.buttons["I saved it, continue"].tap()

        // Step 2: a wrong check or a wrong password changes nothing.
        XCTAssertTrue(app.staticTexts["Step 2 of 2: check it"].waitForExistence(timeout: 10))
        let tail = app.staticTexts["debug-recovery-tail"].label
        let turnOn = app.buttons["Turn on Recovery Key"]
        type("AAAA-AAAA", into: app.textFields["Last two groups of the Recovery Key"])
        type(Self.password, into: app.secureTextFields["Master password"])
        XCTAssertFalse(turnOn.isEnabled, "the code was not typed back")

        let check = app.textFields["Last two groups of the Recovery Key"]
        check.tap()
        check.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 9) + tail)
        let password = app.secureTextFields["Master password"]
        password.tap()
        password.typeText(
            String(repeating: XCUIKeyboardKey.delete.rawValue, count: Self.password.count) + "wrong-password")
        turnOn.tap()
        XCTAssertTrue(app.staticTexts["That master password or Secret Key is not right."].waitForExistence(timeout: 30))

        password.tap()
        password.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 14) + Self.password)
        turnOn.tap()
        XCTAssertTrue(app.staticTexts["Your Recovery Key is on."].waitForExistence(timeout: 30))
        XCTAssertTrue(app.staticTexts["On"].exists)
        XCTAssertTrue(app.buttons["Replace Recovery Key"].exists)

        // Turning it off needs the master password too.
        app.buttons["Turn off the Recovery Key"].tap()
        let offPassword = app.secureTextFields["Master password"]
        type("wrong-password", into: offPassword)
        sheetTurnOffButton().tap()
        XCTAssertTrue(app.staticTexts["That master password or Secret Key is not right."].waitForExistence(timeout: 30))
        // Cancelling clears what was typed; the second try opens the sheet again.
        app.buttons["Cancel"].tap()
        app.buttons["Turn off the Recovery Key"].tap()
        type(Self.password, into: app.secureTextFields["Master password"])
        sheetTurnOffButton().tap()
        XCTAssertTrue(
            app.staticTexts["Your Recovery Key is off. The old code opens nothing from now on."]
                .waitForExistence(timeout: 30))
        XCTAssertTrue(app.staticTexts["Off"].exists)
    }
}
