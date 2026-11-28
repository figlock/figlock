import XCTest

/// FIG-292: Settings > Password health lists weak and reused passwords from the made-up demo vault, opens an item
/// from the list, and with the breach service unreachable says so and keeps the other results. The service is
/// switched off with a launch argument so the test does not depend on the network.
@MainActor
final class PasswordHealthUITests: XCTestCase {
    func testHealthScreenListsProblemsOpensAnItemAndSurvivesAnOfflineBreachCheck() {
        let app = XCUIApplication()
        app.launchArguments = [
            "-figlock-seed-demo", "-figlock-unlock-demo", "-figlock-seed-health", "-figlock-breach-offline",
        ]
        app.launch()

        let settings = app.buttons["Settings"].firstMatch
        XCTAssertTrue(settings.waitForExistence(timeout: 20))
        settings.tap()
        let row = app.descendants(matching: .any)["settings-password-health"]
        XCTAssertTrue(row.waitForExistence(timeout: 10), "missing the Password health row")
        row.tap()

        XCTAssertTrue(app.navigationBars["Password health"].waitForExistence(timeout: 10))
        let weak = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Old forum'")).firstMatch
        XCTAssertTrue(weak.waitForExistence(timeout: 10), "the weak login is not listed")
        XCTAssertTrue(weak.label.contains("Weak"))
        let shared = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Shared one'")).firstMatch
        XCTAssertTrue(shared.exists)
        XCTAssertTrue(shared.label.contains("Reused with 1 other item"))
        XCTAssertFalse(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Fastmail'")).firstMatch.exists)

        // The breach service cannot be reached: a clear notice, and the local results stay.
        app.buttons["health-check-breaches"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["health-unreachable"].waitForExistence(timeout: 10))
        XCTAssertTrue(weak.exists)

        // A row opens the item.
        weak.tap()
        XCTAssertTrue(app.navigationBars["Old forum"].waitForExistence(timeout: 10))
    }
}
