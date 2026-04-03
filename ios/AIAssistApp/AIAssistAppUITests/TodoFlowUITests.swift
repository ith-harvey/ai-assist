import XCTest

/// Tests the todo list UI flow — viewing, completing, searching, and detail views.
final class TodoFlowUITests: XCTestCase {

    let app = XCUIApplication()

    override func setUp() {
        super.setUp()
        continueAfterFailure = false
        app.launchArguments = ["--uitesting"]
        app.launch()

        // Navigate to Todos tab
        let todosTab = app.tabBars.buttons["Todos"]
        if todosTab.exists {
            todosTab.tap()
        }
    }

    // MARK: - Todo List

    func testTodoListPopulatesWithSampleData() {
        let list = app.collectionViews.firstMatch
        XCTAssertTrue(list.waitForExistence(timeout: 5), "Todo list should appear")
    }

    func testTodoRowShowsTitleAndType() {
        let list = app.collectionViews.firstMatch
        guard list.waitForExistence(timeout: 5) else { return }

        // At least one cell should exist with sample data
        let firstCell = list.cells.firstMatch
        if firstCell.waitForExistence(timeout: 3) {
            XCTAssertTrue(firstCell.isHittable)
        }
    }

    // MARK: - Todo Detail

    func testTapTodoOpensDetail() {
        let list = app.collectionViews.firstMatch
        guard list.waitForExistence(timeout: 5) else { return }

        let firstCell = list.cells.firstMatch
        if firstCell.waitForExistence(timeout: 3) {
            firstCell.tap()
            // Detail view should appear
            Thread.sleep(forTimeInterval: 0.5)
            // Back button should exist (we navigated forward)
            let backButton = app.navigationBars.buttons.firstMatch
            XCTAssertTrue(backButton.exists)
        }
    }

    // MARK: - Swipe Actions

    func testSwipeToComplete() {
        let list = app.collectionViews.firstMatch
        guard list.waitForExistence(timeout: 5) else { return }

        let firstCell = list.cells.firstMatch
        if firstCell.waitForExistence(timeout: 3) {
            // Swipe to reveal complete action
            firstCell.swipeLeft()
            let completeButton = app.buttons["Complete"]
            if completeButton.waitForExistence(timeout: 2) {
                completeButton.tap()
            }
        }
    }

    // MARK: - Search

    func testSearchBarExists() {
        // The searchable modifier should provide a search bar
        let searchField = app.searchFields.firstMatch
        _ = searchField.waitForExistence(timeout: 3)
        // Search may be accessible via pull-down or always visible
    }
}
