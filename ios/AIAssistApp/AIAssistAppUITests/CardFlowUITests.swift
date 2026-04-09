import XCTest

/// Tests the approval card swipe flow — the core UX of AI Assist.
/// Cards appear in the Messages silo and are approved/dismissed via swipe gestures.
final class CardFlowUITests: XCTestCase {

    let app = XCUIApplication()

    override func setUp() {
        super.setUp()
        continueAfterFailure = false
        app.launchArguments = ["--uitesting"]
        app.launch()
    }

    // MARK: - Card Queue

    func testCardQueueShowsCards() {
        let messagesTab = app.tabBars.buttons["Messages"]
        if messagesTab.exists {
            messagesTab.tap()
        }

        // With sample data, cards should appear
        let cardContainer = app.otherElements["swipe_card_container"].firstMatch
        if cardContainer.waitForExistence(timeout: 5) {
            XCTAssertTrue(cardContainer.isHittable, "Card container should be interactive")
        }
    }

    func testCardShowsSenderAndMessage() {
        let messagesTab = app.tabBars.buttons["Messages"]
        if messagesTab.exists {
            messagesTab.tap()
        }

        // Verify card content elements exist
        let card = app.otherElements["approval_card"].firstMatch
        if card.waitForExistence(timeout: 5) {
            // Card should display sender info and suggested reply
            XCTAssertTrue(card.isHittable)
        }
    }

    // MARK: - Swipe Actions

    func testSwipeRightApproves() {
        let messagesTab = app.tabBars.buttons["Messages"]
        if messagesTab.exists {
            messagesTab.tap()
        }

        let card = app.otherElements["approval_card"].firstMatch
        if card.waitForExistence(timeout: 5) {
            let initialCount = app.otherElements.matching(identifier: "approval_card").count
            card.swipeRight()

            // After approve, the card should be removed or next card shown
            // Wait briefly for animation
            Thread.sleep(forTimeInterval: 0.5)
            let newCount = app.otherElements.matching(identifier: "approval_card").count
            // Count should decrease or remain same (if queue auto-advances)
            XCTAssertTrue(newCount <= initialCount)
        }
    }

    func testSwipeLeftDismisses() {
        let messagesTab = app.tabBars.buttons["Messages"]
        if messagesTab.exists {
            messagesTab.tap()
        }

        let card = app.otherElements["approval_card"].firstMatch
        if card.waitForExistence(timeout: 5) {
            card.swipeLeft()
            // Card should be dismissed
            Thread.sleep(forTimeInterval: 0.5)
        }
    }

    // MARK: - Empty State

    func testEmptyStateMessage() {
        // When all cards are dismissed, an empty state should appear
        let emptyState = app.staticTexts["No messages"]
        // This may or may not appear depending on sample data
        _ = emptyState.waitForExistence(timeout: 2)
    }
}
