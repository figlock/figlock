import XCTest

/// FIG-246: in the item editor each finished tag is its own chip with a remove button.
@MainActor
final class TagChipsUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-figlock-seed-demo", "-figlock-unlock-demo"]
        app.launch()
    }

    private func openEditor() {
        let row = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Fastmail'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15))
        row.tap()
        app.buttons["Item actions"].tap()
        app.buttons["Edit"].tap()
        XCTAssertTrue(app.navigationBars["Edit Login"].waitForExistence(timeout: 5))
    }

    private func tagsInput() -> XCUIElement {
        let input = app.textFields["tags-input"]
        for _ in 0..<8 where !input.isHittable { app.swipeUp() }
        XCTAssertTrue(input.waitForExistence(timeout: 5))
        return input
    }

    func testTypedTagsBecomeChipsAndCanBeRemoved() {
        openEditor()
        let input = tagsInput()
        input.tap()
        // One tag at a time: the field rewrites its text after each separator, and keys typed while that
        // is in flight can be lost on a slow runner (FIG-284).
        input.typeText("zz-work,")
        XCTAssertTrue(app.otherElements["tag-chip-zz-work"].waitForExistence(timeout: 10))
        input.typeText("mail ")
        XCTAssertTrue(app.otherElements["tag-chip-mail"].waitForExistence(timeout: 10))
        let emptied = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "value == 'Add tag'"), object: input)
        XCTAssertEqual(
            XCTWaiter().wait(for: [emptied], timeout: 5), .completed, "the field is empty again after a separator")
        XCTAssertTrue(app.buttons["Remove tag mail"].exists)

        // A half typed tag turns into a chip when return is pressed.
        input.typeText("ops")
        input.typeText("\n")
        XCTAssertTrue(app.otherElements["tag-chip-ops"].waitForExistence(timeout: 5))
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = "tag-chips"
        shot.lifetime = .keepAlways
        add(shot)

        app.buttons["Remove tag mail"].tap()
        XCTAssertFalse(app.otherElements["tag-chip-mail"].exists)
        XCTAssertTrue(app.otherElements["tag-chip-zz-work"].exists)
    }
}
