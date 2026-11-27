import XCTest

/// FIG-229: "Forgot password?" on the lock screen, against the throwaway Recovery Key fixture
/// (`-figlock-seed-recovery`). Every value here is made up.
@MainActor
final class ForgotPasswordUITests: XCTestCase {
    private static let oldPassword = "fixture-old-password-1"
    private static let newPassword = "generated-new-password-2"
    private static let recoveryKey =
        "FLR1-JJFG-G154-HB6Z-KSE5-PKAY-KN4R-F5F2-8DX4-9E41-5QA2-KVZX-GQAR-ZPM5-2PD0"
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-figlock-fresh", "-figlock-seed-recovery", "-figlock-expose-new-key"]
        app.launch()
    }

    private func openForgot() {
        let link = app.buttons["Forgot password?"]
        XCTAssertTrue(link.waitForExistence(timeout: 20))
        link.tap()
        XCTAssertTrue(app.staticTexts["Enter your Recovery Key"].waitForExistence(timeout: 10))
    }

    private func enterKeyAndContinue(_ key: String) {
        let field = app.textFields["Recovery Key"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        field.typeText(key)
        app.buttons["Continue"].tap()
    }

    private func typeNewPassword() {
        XCTAssertTrue(app.staticTexts["Choose a new password"].waitForExistence(timeout: 30))
        // The lock screen's own field is still there behind the sheet, so take the sheet's two.
        let fields = Array(app.secureTextFields.allElementsBoundByIndex.suffix(2))
        XCTAssertEqual(fields.count, 2)
        for field in fields {
            XCTAssertTrue(field.waitForExistence(timeout: 10))
            field.tap()
            let focused = NSPredicate(format: "hasKeyboardFocus == true")
            if XCTWaiter.wait(for: [expectation(for: focused, evaluatedWith: field)], timeout: 3) != .completed {
                field.tap()
            }
            field.typeText(Self.newPassword)
        }
        app.buttons["Continue"].tap()
        XCTAssertTrue(app.staticTexts["Save your new Emergency Kit"].waitForExistence(timeout: 30))
    }

    func testAWrongRecoveryKeyIsRefusedAndNothingChanges() {
        openForgot()
        // Well formed, but the key of another vault.
        enterKeyAndContinue(Self.recoveryKey.replacingOccurrences(of: "9E41", with: "9E42"))
        XCTAssertTrue(
            app.staticTexts.matching(
                NSPredicate(format: "label CONTAINS \"does not open\" OR label CONTAINS \"not a valid\"")
            )
            .firstMatch.waitForExistence(timeout: 30))
        XCTAssertTrue(app.staticTexts["Enter your Recovery Key"].exists, "still on step 1")
        app.buttons["Cancel"].tap()
        // The old password still opens the vault: unchanged.
        let field = app.secureTextFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        field.typeText(Self.oldPassword)
        app.buttons["Unlock"].tap()
        XCTAssertTrue(app.buttons["Forgot password?"].waitForNonExistence(timeout: 60))
    }

    func testALostRecoveryKeyCanRemoveTheVaultAfterConfirmingAndLandsOnSetup() {
        openForgot()
        let remove = app.buttons["Remove this vault from this iPhone"]
        XCTAssertTrue(remove.waitForExistence(timeout: 10))
        remove.tap()
        // Cancelling changes nothing.
        let alert = app.alerts["Remove this vault from this iPhone?"]
        XCTAssertTrue(alert.waitForExistence(timeout: 10))
        alert.buttons["Cancel"].tap()
        XCTAssertTrue(app.staticTexts["Enter your Recovery Key"].exists)
        remove.tap()
        XCTAssertTrue(alert.waitForExistence(timeout: 10))
        alert.buttons["Remove vault"].tap()
        let create = app.buttons.matching(NSPredicate(format: "label BEGINSWITH \"Create a new vault\"")).firstMatch
        XCTAssertTrue(create.waitForExistence(timeout: 30), "the vault setup screen shows")
        XCTAssertFalse(app.buttons["Forgot password?"].exists)
    }

    func testRecoveringSetsANewPasswordAndOldOneStopsWorking() {
        openForgot()
        enterKeyAndContinue(Self.recoveryKey)
        typeNewPassword()
        app.buttons["Save Emergency Kit"].tap()
        // Close the share sheet; the kit was opened, so the next step is offered.
        let close = app.buttons["Close"].firstMatch
        XCTAssertTrue(close.waitForExistence(timeout: 15))
        close.tap()
        // Wait for the share sheet to finish dismissing, otherwise the tap lands on the closing sheet.
        let saved = app.buttons["I saved it, continue"]
        XCTAssertTrue(saved.waitForExistence(timeout: 15))
        let hittable = NSPredicate(format: "isHittable == true")
        XCTAssertEqual(XCTWaiter.wait(for: [expectation(for: hittable, evaluatedWith: saved)], timeout: 15), .completed)
        saved.tap()
        XCTAssertTrue(app.staticTexts["Check that you saved it"].waitForExistence(timeout: 30))
        // A wrong check does not change the vault.
        let groups = app.staticTexts["debug-new-key-groups"].label
        let field = app.textFields.firstMatch
        field.tap()
        field.typeText("AAAA-AAAA")
        app.buttons["Change password and Secret Key"].tap()
        XCTAssertTrue(
            app.staticTexts.matching(NSPredicate(format: "label CONTAINS \"doesn't match\"")).firstMatch
                .waitForExistence(timeout: 5))
        field.clearAndTypeText(groups)
        app.buttons["Change password and Secret Key"].tap()
        // The vault opens with the new credentials: the sheet closes and the vault list shows.
        XCTAssertTrue(app.buttons["Forgot password?"].waitForNonExistence(timeout: 60))
    }
}

extension XCUIElement {
    fileprivate func clearAndTypeText(_ text: String) {
        tap()
        let current = (value as? String) ?? ""
        typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: current.count) + text)
    }
}
