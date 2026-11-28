import XCTest

/// FIG-286: Settings > Import against the made-up demo vault. The system file picker cannot be driven, so Debug
/// builds take `-figlock-import-fixture good|bad` to stand in for a chosen file made of generated text.
@MainActor
final class ImportUITests: XCTestCase {
    private var app: XCUIApplication!

    private func launch(fixture: String) {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo", "-figlock-import-fixture", fixture]
        app.launch()
        let settings = app.buttons["Settings"]
        XCTAssertTrue(settings.waitForExistence(timeout: 20))
        settings.tap()
        let row = app.buttons["settings-import"]
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.tap()
        XCTAssertTrue(app.navigationBars["Import"].waitForExistence(timeout: 10))
    }

    func testPreviewShowsWhatWasFoundAndWritesNothingUntilConfirmed() {
        launch(fixture: "good")
        app.buttons["import-choose"].tap()
        let counts = app.descendants(matching: .any)["import-counts"]
        XCTAssertTrue(counts.waitForExistence(timeout: 10))
        XCTAssertTrue(counts.label.contains("2 logins"))
        XCTAssertTrue(app.staticTexts["Imported One"].exists)
        // Cancel goes back to the chooser with nothing added.
        app.buttons["import-cancel"].tap()
        XCTAssertTrue(app.buttons["import-choose"].waitForExistence(timeout: 10))

        app.buttons["import-choose"].tap()
        XCTAssertTrue(app.buttons["import-confirm"].waitForExistence(timeout: 10))
        app.buttons["import-confirm"].tap()
        let message = app.descendants(matching: .any)["import-message"]
        XCTAssertTrue(message.waitForExistence(timeout: 30))
        XCTAssertTrue(message.label.contains("Imported 2 items"))
    }

    func testAFileThatIsNotThatExportShowsAClearError() {
        launch(fixture: "bad")
        app.buttons["import-choose"].tap()
        let message = app.descendants(matching: .any)["import-message"]
        XCTAssertTrue(message.waitForExistence(timeout: 10))
        XCTAssertTrue(message.label.contains("does not look like an export"))
        XCTAssertFalse(app.buttons["import-confirm"].exists)
    }
}
