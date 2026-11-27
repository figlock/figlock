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

    // FIG-254: the two password boxes on step 1 are the same height, though one has a show/hide button.
    func testPasswordBoxesAreTheSameHeight() {
        let create = app.buttons.matching(NSPredicate(format: "label BEGINSWITH \"Create a new vault\"")).firstMatch
        XCTAssertTrue(create.waitForExistence(timeout: 15))
        create.tap()
        let password = app.secureTextFields.element(boundBy: 0)
        XCTAssertTrue(password.waitForExistence(timeout: 15))
        let eye = app.buttons["Show password"]
        XCTAssertTrue(eye.exists)
        XCTAssertEqual(eye.frame.height, 44, accuracy: 1)
        // The box is what lies between its label and the hint under it (4pt and 16pt of spacing): 44pt tall, as
        // tall as the confirm box, not 44pt of button plus the field's own padding.
        let label = app.staticTexts["Password"]
        let hint = app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'Use at least'")).firstMatch
        XCTAssertTrue(label.exists && hint.exists)
        let box = hint.frame.minY - label.frame.maxY - 4 - 16
        XCTAssertLessThanOrEqual(box, 46, "the password box is \(box)pt tall")
    }
}
