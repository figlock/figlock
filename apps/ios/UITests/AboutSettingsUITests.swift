import XCTest

/// FIG-253: Settings has one "About FigLock" row. It opens the About screen with the version and the figlock.com
/// pages (App Store guideline 5.1.1), which open inside the app. Runs against the made-up demo vault.
@MainActor
final class AboutSettingsUITests: XCTestCase {
    func testAboutScreenListsPagesAndOpensOneInApp() {
        let app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo"]
        app.launch()

        let settings = app.buttons["Settings"]
        XCTAssertTrue(settings.waitForExistence(timeout: 20))
        settings.tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 10))

        let about = app.descendants(matching: .any)["about-figlock"]
        XCTAssertTrue(reveal(about, in: app), "missing About FigLock row")
        about.tap()

        XCTAssertTrue(app.navigationBars["About FigLock"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'Version '")).firstMatch.exists)
        for id in ["about-protects", "about-privacy", "about-terms", "about-support"] {
            XCTAssertTrue(app.descendants(matching: .any)[id].exists, "missing \(id)")
        }

        // The page opens in the in-app browser and actually renders (a blank view had no text), then Done returns to About.
        app.descendants(matching: .any)["about-privacy"].tap()
        let heading = app.webViews.staticTexts["Privacy"].firstMatch
        XCTAssertTrue(heading.waitForExistence(timeout: 30), "the privacy page did not render in the in-app browser")
        app.buttons["about-browser-done"].tap()
        XCTAssertTrue(app.navigationBars["About FigLock"].waitForExistence(timeout: 10))
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
