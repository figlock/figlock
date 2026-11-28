import XCTest

/// FIG-288: Settings > Export against the made-up demo vault. Debug builds take `-figlock-allow-device-auth` and
/// `-figlock-deny-device-auth` to stand in for Face ID, which the simulator cannot give.
@MainActor
final class ExportUITests: XCTestCase {
    private var app: XCUIApplication!

    private func launch(auth: String) {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo", auth]
        app.launch()
    }

    private func openExport() {
        let settings = app.buttons["Settings"]
        XCTAssertTrue(settings.waitForExistence(timeout: 20))
        settings.tap()
        let row = app.buttons["settings-export"]
        // The Vault rows can run past the first screen: scroll to the row before tapping it.
        for _ in 0..<3 where !row.exists || !row.isHittable { app.swipeUp() }
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.tap()
        XCTAssertTrue(app.navigationBars["Export"].waitForExistence(timeout: 10))
    }

    func testExportWithoutSecretsOpensTheShareSheetWithoutAsking() {
        launch(auth: "-figlock-deny-device-auth")
        openExport()
        app.buttons["export-button"].tap()
        // The share sheet is up, and no "not confirmed" message is shown.
        XCTAssertTrue(app.otherElements["ActivityListView"].waitForExistence(timeout: 30))
        XCTAssertFalse(app.descendants(matching: .any)["export-message"].exists)
        let close = app.buttons["Close"].firstMatch
        XCTAssertTrue(close.waitForExistence(timeout: 15))
        close.tap()
        XCTAssertTrue(app.buttons["export-button"].waitForExistence(timeout: 10))
    }

    func testExportWithSecretsOpensTheShareSheetAfterFaceID() {
        launch(auth: "-figlock-allow-device-auth")
        openExport()
        let toggle = app.switches["export-include-secrets"].switches.firstMatch
        toggle.tap()
        XCTAssertEqual(toggle.value as? String, "1")
        app.buttons["export-button"].tap()
        XCTAssertTrue(app.otherElements["ActivityListView"].waitForExistence(timeout: 30))
        XCTAssertFalse(app.descendants(matching: .any)["export-message"].exists)
    }

    func testExportWithSecretsIsBlockedWhenFaceIDOrThePasscodeFails() {
        launch(auth: "-figlock-deny-device-auth")
        openExport()
        let toggle = app.switches["export-include-secrets"].switches.firstMatch
        toggle.tap()
        XCTAssertEqual(toggle.value as? String, "1")
        app.buttons["export-button"].tap()
        let message = app.descendants(matching: .any)["export-message"]
        XCTAssertTrue(message.waitForExistence(timeout: 30))
        XCTAssertFalse(app.otherElements["ActivityListView"].exists)
    }
}
