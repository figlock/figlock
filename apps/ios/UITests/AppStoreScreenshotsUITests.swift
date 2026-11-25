import XCTest

/// Captures the App Store screenshots of screens that need taps (FIG-174). It does nothing unless
/// `FIGLOCK_SCREENSHOT_DIR` is set, so the normal UI test run skips it. The demo vault holds made-up
/// items only; `apps/ios/scripts/app-store-screenshots.sh` drives this test and takes the pictures.
@MainActor
final class AppStoreScreenshotsUITests: XCTestCase {
    private var app: XCUIApplication!
    private var directory: String!

    override func setUpWithError() throws {
        continueAfterFailure = false
        guard let directory = ProcessInfo.processInfo.environment["FIGLOCK_SCREENSHOT_DIR"], !directory.isEmpty else {
            throw XCTSkip("Set FIGLOCK_SCREENSHOT_DIR to capture App Store screenshots.")
        }
        self.directory = directory
        app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo"]
        app.launch()
    }

    /// The app hides itself from in-process screenshots on purpose (design/DESIGN.md 8), so the test only navigates:
    /// it writes `<name>.ready` and waits for `<name>.done`, and the script takes the picture with `simctl io`.
    private func capture(_ name: String) throws {
        let base = URL(fileURLWithPath: directory)
        try Data().write(to: base.appendingPathComponent("\(name).ready"))
        let done = base.appendingPathComponent("\(name).done").path
        let deadline = Date().addingTimeInterval(60)
        while !FileManager.default.fileExists(atPath: done) {
            XCTAssertLessThan(Date(), deadline, "the screenshot script did not answer for \(name)")
            Thread.sleep(forTimeInterval: 0.3)
        }
    }

    private func row(_ title: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title)).firstMatch
    }

    func testCaptureItemDetailAndSettings() throws {
        let stripe = row("Stripe API key")
        XCTAssertTrue(stripe.waitForExistence(timeout: 15))
        stripe.tap()
        XCTAssertTrue(app.buttons["Item actions"].waitForExistence(timeout: 10))
        try capture("04-item-detail")
        app.navigationBars.buttons.firstMatch.tap()
        let settings = app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'Settings'")).firstMatch
        XCTAssertTrue(settings.waitForExistence(timeout: 10))
        settings.tap()
        XCTAssertTrue(app.navigationBars.firstMatch.waitForExistence(timeout: 10))
        try capture("05-settings")
    }
}
