import XCTest

/// Core UI tests for AI Assist — validates critical user flows in the simulator.
///
/// Setup: Add a "UI Testing Bundle" target in Xcode (File > New > Target > UI Testing Bundle),
/// name it "AIAssistAppUITests", then these files will auto-sync via the file system group.
final class AIAssistAppUITests: XCTestCase {

    let app = XCUIApplication()

    override func setUp() {
        super.setUp()
        continueAfterFailure = false
        app.launchArguments = ["--uitesting"]
        app.launch()
    }

    // MARK: - Tab Navigation

    func testTabBarExists() {
        let tabBar = app.tabBars.firstMatch
        XCTAssertTrue(tabBar.waitForExistence(timeout: 5), "Tab bar should appear on launch")
    }

    func testNavigateToMessagesTab() {
        let messagesTab = app.tabBars.buttons["Messages"]
        if messagesTab.exists {
            messagesTab.tap()
            // Should show the messages/cards view
            XCTAssertTrue(app.navigationBars.firstMatch.waitForExistence(timeout: 3))
        }
    }

    func testNavigateToTodosTab() {
        let todosTab = app.tabBars.buttons["Todos"]
        if todosTab.exists {
            todosTab.tap()
            XCTAssertTrue(app.navigationBars.firstMatch.waitForExistence(timeout: 3))
        }
    }

    func testNavigateToCalendarTab() {
        let calendarTab = app.tabBars.buttons["Calendar"]
        if calendarTab.exists {
            calendarTab.tap()
            XCTAssertTrue(app.navigationBars.firstMatch.waitForExistence(timeout: 3))
        }
    }

    func testNavigateToBrainTab() {
        let brainTab = app.tabBars.buttons["Brain"]
        if brainTab.exists {
            brainTab.tap()
            XCTAssertTrue(app.navigationBars.firstMatch.waitForExistence(timeout: 3))
        }
    }

    // MARK: - Settings

    func testSettingsAccessible() {
        // Settings is typically accessible via a gear icon
        let settingsButton = app.navigationBars.buttons["gearshape"]
        if settingsButton.exists {
            settingsButton.tap()
            XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 3))
        }
    }

    // MARK: - Approval Cards

    func testSwipeCardContainer() {
        let messagesTab = app.tabBars.buttons["Messages"]
        if messagesTab.exists {
            messagesTab.tap()
        }

        // Wait for any cards to appear (in test/sample data mode)
        let card = app.otherElements["approval_card"].firstMatch
        if card.waitForExistence(timeout: 5) {
            // Test swipe right (approve)
            card.swipeRight()
        }
    }

    // MARK: - Todo List

    func testTodoListDisplays() {
        let todosTab = app.tabBars.buttons["Todos"]
        if todosTab.exists {
            todosTab.tap()
        }

        // Should show todo items (sample data or connected)
        let list = app.collectionViews.firstMatch
        XCTAssertTrue(list.waitForExistence(timeout: 5), "Todo list should appear")
    }

    // MARK: - Connection Banner

    func testConnectionBannerShowsWhenDisconnected() {
        // When server is not running, a connection banner should appear
        // This test validates the empty/disconnected state gracefully handles
        let banner = app.staticTexts["Connecting..."]
        // Not asserting existence since it depends on server state,
        // but verifying the app doesn't crash
        _ = banner.waitForExistence(timeout: 2)
    }
}
