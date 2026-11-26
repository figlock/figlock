import XCTest

/// Settings has a Face ID row. The simulator has no Secure Enclave, so it says Face ID is unavailable there (ADR-0046).
@MainActor
final class SettingsFaceIDUITests: XCTestCase {
    func testSettingsOffersFaceIDAndSaysWhenThePhoneCannot() {
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo"]
        app.launch()

        let settings = app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'Settings'")).firstMatch
        XCTAssertTrue(settings.waitForExistence(timeout: 15))
        settings.tap()
        let row = app.buttons["settings-face-id"]
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        XCTAssertTrue(row.label.contains("Unavailable"), row.label)
        row.tap()
        XCTAssertTrue(app.staticTexts["Face ID is unavailable"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.switches["face-id-toggle"].exists)
    }
}
