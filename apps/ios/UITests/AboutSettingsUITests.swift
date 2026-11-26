import XCTest

/// FIG-241: Settings > About lists the Privacy, Terms and Support links and the version (App Store guideline 5.1.1),
/// against the made-up demo vault.
@MainActor
final class AboutSettingsUITests: XCTestCase {
    func testSettingsShowsPrivacyTermsSupportAndVersion() {
        let app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo"]
        app.launch()

        let settings = app.buttons["Settings"]
        XCTAssertTrue(settings.waitForExistence(timeout: 20))
        settings.tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 10))

        for id in ["about-privacy", "about-terms", "about-support"] {
            XCTAssertTrue(reveal(app.descendants(matching: .any)[id], in: app), "missing \(id)")
        }
        XCTAssertTrue(reveal(app.staticTexts["Version"], in: app), "missing Version")
    }

    /// The list is lazy, so a row below the fold only exists once it is scrolled near.
    private func reveal(_ element: XCUIElement, in app: XCUIApplication) -> Bool {
        for _ in 0..<5 {
            if element.exists { return true }
            app.swipeUp()
        }
        return element.exists
    }
}
