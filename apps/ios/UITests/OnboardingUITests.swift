import XCTest

/// Runs the debug build from an empty phone (`-figlock-fresh`) through the screens of creating a vault.
@MainActor
final class OnboardingUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-figlock-fresh"]
        app.launch()
    }

    private func reachSecretKeyStep() {
        let create = app.buttons.matching(NSPredicate(format: "label BEGINSWITH \"Create a new vault\"")).firstMatch
        XCTAssertTrue(create.waitForExistence(timeout: 15))
        create.tap()
        let password = app.secureTextFields.element(boundBy: 0)
        XCTAssertTrue(password.waitForExistence(timeout: 15))
        password.tap()
        password.typeText("generated-test-password-1")
        let confirm = app.secureTextFields.element(boundBy: 1)
        confirm.tap()
        confirm.typeText("generated-test-password-1")
        app.buttons["Continue"].tap()
        XCTAssertTrue(app.staticTexts["Your Secret Key"].waitForExistence(timeout: 30))
    }

    // FIG-231: the Secret Key can be copied on step 2, and the screen says so.
    func testSecretKeyCanBeCopiedOnStepTwo() {
        reachSecretKeyStep()
        let copy = app.buttons["Copy Secret Key"]
        XCTAssertTrue(copy.exists)
        copy.tap()
        XCTAssertTrue(
            app.staticTexts.matching(NSPredicate(format: "label CONTAINS \"Copied\"")).firstMatch
                .waitForExistence(timeout: 5))
    }
}
