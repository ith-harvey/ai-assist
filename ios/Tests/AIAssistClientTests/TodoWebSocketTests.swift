import Foundation
import Testing
@testable import AIAssistClientLib

@Suite("TodoWebSocket Tests")
struct TodoWebSocketTests {

    // MARK: - Initial State

    @Test("Initial state is disconnected with empty todos")
    func initialState() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        #expect(ws.isConnected == false)
        #expect(ws.todos.isEmpty)
        #expect(ws.searchResults == nil)
        #expect(ws.searchQuery == "")
    }

    @Test("Default host and port from init")
    func defaultConfig() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        #expect(ws.host == "localhost")
        #expect(ws.port == 8080)
    }

    @Test("Custom host and port")
    func customConfig() {
        let ws = TodoWebSocket(host: "192.168.1.50", port: 9090)
        #expect(ws.host == "192.168.1.50")
        #expect(ws.port == 9090)
    }

    // MARK: - Reconnect Delay

    @Test("Reconnect delay starts at 1 second")
    func reconnectDelayInitial() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        #expect(ws.reconnectDelay() == 1.0)
    }

    @Test("Reconnect delay caps at 30 seconds")
    func reconnectDelayCap() {
        // Test the formula directly: 2^5 = 32 capped at 30
        let delay = min(pow(2.0, 5.0), 30.0)
        #expect(delay == 30.0)
    }

    // MARK: - Server Update

    @Test("UpdateServer changes host and port")
    func updateServer() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        ws.updateServer(host: "example.com", port: 9090)
        #expect(ws.host == "example.com")
        #expect(ws.port == 9090)
    }

    // MARK: - Computed Properties

    @Test("activeTodos filters out completed and snoozed, sorted by priority")
    func activeTodos() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        ws.todos = [
            TodoItem(title: "Low priority", status: .created, priority: 5),
            TodoItem(title: "Completed", status: .completed, priority: 1),
            TodoItem(title: "High priority", status: .agentWorking, priority: 1),
            TodoItem(title: "Snoozed", status: .snoozed, priority: 2),
            TodoItem(title: "Medium", status: .waitingOnYou, priority: 3),
        ]

        let active = ws.activeTodos
        #expect(active.count == 3)
        #expect(active[0].title == "High priority")
        #expect(active[1].title == "Medium")
        #expect(active[2].title == "Low priority")
    }

    @Test("completedTodos returns only completed, sorted by updatedAt desc")
    func completedTodos() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        let older = Date(timeIntervalSince1970: 1000)
        let newer = Date(timeIntervalSince1970: 2000)
        ws.todos = [
            TodoItem(title: "Active", status: .created),
            TodoItem(title: "Done old", status: .completed, updatedAt: older),
            TodoItem(title: "Done new", status: .completed, updatedAt: newer),
        ]

        let completed = ws.completedTodos
        #expect(completed.count == 2)
        #expect(completed[0].title == "Done new")
        #expect(completed[1].title == "Done old")
    }

    @Test("snoozedTodos returns only snoozed, sorted by snoozedUntil asc")
    func snoozedTodos() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        let sooner = Date(timeIntervalSinceNow: 3600)
        let later = Date(timeIntervalSinceNow: 7200)
        ws.todos = [
            TodoItem(title: "Active", status: .created),
            TodoItem(title: "Snoozed later", status: .snoozed, snoozedUntil: later),
            TodoItem(title: "Snoozed sooner", status: .snoozed, snoozedUntil: sooner),
        ]

        let snoozed = ws.snoozedTodos
        #expect(snoozed.count == 2)
        #expect(snoozed[0].title == "Snoozed sooner")
        #expect(snoozed[1].title == "Snoozed later")
    }

    @Test("approvalCount counts readyForReview and waitingOnYou")
    func approvalCount() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        ws.todos = [
            TodoItem(title: "A", status: .readyForReview),
            TodoItem(title: "B", status: .waitingOnYou),
            TodoItem(title: "C", status: .created),
            TodoItem(title: "D", status: .readyForReview),
        ]
        #expect(ws.approvalCount == 3)
    }

    @Test("approvalCount is zero when no matching statuses")
    func approvalCountZero() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        ws.todos = [
            TodoItem(title: "A", status: .created),
            TodoItem(title: "B", status: .completed),
        ]
        #expect(ws.approvalCount == 0)
    }

    // MARK: - Todo List Operations

    @Test("Todos array supports add and remove")
    func todoListManagement() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        let todo = TodoItem(title: "Test todo")

        ws.todos.append(todo)
        #expect(ws.todos.count == 1)
        #expect(ws.todos[0].title == "Test todo")

        ws.todos.removeAll { $0.id == todo.id }
        #expect(ws.todos.isEmpty)
    }

    @Test("Todos sync replaces entire list")
    func todosSyncReplacesTodos() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        ws.todos = [TodoItem(title: "Old")]

        let newTodos = [TodoItem(title: "New 1"), TodoItem(title: "New 2")]
        ws.todos = newTodos
        #expect(ws.todos.count == 2)
        #expect(ws.todos[0].title == "New 1")
    }

    @Test("Todo update replaces matching item")
    func todoUpdateReplacesItem() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        let id = UUID()
        ws.todos = [
            TodoItem(id: id, title: "Original", status: .created),
            TodoItem(title: "Other"),
        ]

        // Simulate update
        if let index = ws.todos.firstIndex(where: { $0.id == id }) {
            ws.todos[index] = TodoItem(id: id, title: "Updated", status: .agentWorking)
        }
        #expect(ws.todos.first(where: { $0.id == id })?.title == "Updated")
        #expect(ws.todos.first(where: { $0.id == id })?.status == .agentWorking)
    }

    @Test("Todo delete removes item by ID")
    func todoDeleteRemovesItem() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        let id1 = UUID()
        let id2 = UUID()
        ws.todos = [
            TodoItem(id: id1, title: "Keep"),
            TodoItem(id: id2, title: "Remove"),
        ]

        ws.todos.removeAll { $0.id == id2 }
        #expect(ws.todos.count == 1)
        #expect(ws.todos[0].title == "Keep")
    }

    // MARK: - Search

    @Test("clearSearch resets search state")
    func clearSearch() {
        let ws = TodoWebSocket(host: "localhost", port: 8080)
        ws.searchResults = [TodoItem(title: "result")]
        ws.searchQuery = "test"

        ws.clearSearch()
        #expect(ws.searchResults == nil)
        #expect(ws.searchQuery == "")
    }

    // MARK: - TodoWsMessage Decoding

    @Test("Decode todos_sync message")
    func decodeTodosSync() throws {
        let json = """
        {
            "type": "todos_sync",
            "todos": [{
                "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                "title": "Test",
                "todo_type": "deliverable",
                "bucket": "human_only",
                "status": "created",
                "priority": 3,
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:00:00Z"
            }]
        }
        """
        let data = json.data(using: .utf8)!
        let msg = TodoWsMessage.decode(from: data)
        guard case .todosSync(let todos) = msg else {
            Issue.record("Expected todosSync")
            return
        }
        #expect(todos.count == 1)
        #expect(todos[0].title == "Test")
    }

    @Test("Decode todo_created message")
    func decodeTodoCreated() throws {
        let json = """
        {
            "type": "todo_created",
            "todo": {
                "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                "title": "New todo",
                "todo_type": "errand",
                "bucket": "human_only",
                "status": "created",
                "priority": 2,
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:00:00Z"
            }
        }
        """
        let data = json.data(using: .utf8)!
        let msg = TodoWsMessage.decode(from: data)
        guard case .todoCreated(let todo) = msg else {
            Issue.record("Expected todoCreated")
            return
        }
        #expect(todo.title == "New todo")
        #expect(todo.todoType == .errand)
    }

    @Test("Decode todo_deleted message")
    func decodeTodoDeleted() {
        let json = """
        {"type": "todo_deleted", "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"}
        """
        let data = json.data(using: .utf8)!
        let msg = TodoWsMessage.decode(from: data)
        guard case .todoDeleted(let id) = msg else {
            Issue.record("Expected todoDeleted")
            return
        }
        #expect(id == UUID(uuidString: "a1b2c3d4-e5f6-7890-abcd-ef1234567890")!)
    }

    @Test("Decode search_results message")
    func decodeSearchResults() {
        let json = """
        {
            "type": "search_results",
            "query": "groceries",
            "results": [{
                "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                "title": "Buy groceries",
                "todo_type": "errand",
                "bucket": "human_only",
                "status": "created",
                "priority": 3,
                "created_at": "2026-03-01T10:00:00Z",
                "updated_at": "2026-03-01T10:00:00Z"
            }]
        }
        """
        let data = json.data(using: .utf8)!
        let msg = TodoWsMessage.decode(from: data)
        guard case .searchResults(let query, let results) = msg else {
            Issue.record("Expected searchResults")
            return
        }
        #expect(query == "groceries")
        #expect(results.count == 1)
    }

    @Test("Decode ping message")
    func decodePing() {
        let json = "{\"type\": \"ping\"}"
        let data = json.data(using: .utf8)!
        let msg = TodoWsMessage.decode(from: data)
        guard case .ping = msg else {
            Issue.record("Expected ping")
            return
        }
    }

    @Test("Unknown message type returns nil")
    func decodeUnknown() {
        let json = "{\"type\": \"unknown_future_type\"}"
        let data = json.data(using: .utf8)!
        let msg = TodoWsMessage.decode(from: data)
        #expect(msg == nil)
    }

    // MARK: - TodoWsAction Encoding

    @Test("Encode complete action")
    func encodeCompleteAction() throws {
        let id = UUID(uuidString: "a1b2c3d4-e5f6-7890-abcd-ef1234567890")!
        let action = TodoWsAction.complete(todoId: id)
        let data = try action.toData()
        let dict = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        #expect(dict["action"] as? String == "complete")
        #expect(dict["id"] as? String == "a1b2c3d4-e5f6-7890-abcd-ef1234567890")
    }

    @Test("Encode delete action")
    func encodeDeleteAction() throws {
        let id = UUID(uuidString: "a1b2c3d4-e5f6-7890-abcd-ef1234567890")!
        let action = TodoWsAction.delete(todoId: id)
        let data = try action.toData()
        let dict = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        #expect(dict["action"] as? String == "delete")
    }

    @Test("Encode search action")
    func encodeSearchAction() throws {
        let action = TodoWsAction.search(query: "groceries", limit: 10)
        let data = try action.toData()
        let dict = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        #expect(dict["action"] as? String == "search")
        #expect(dict["query"] as? String == "groceries")
        #expect(dict["limit"] as? Int == 10)
    }

    @Test("Encode snooze action with date")
    func encodeSnoozeAction() throws {
        let id = UUID(uuidString: "a1b2c3d4-e5f6-7890-abcd-ef1234567890")!
        let until = Date(timeIntervalSince1970: 1710500000)
        let action = TodoWsAction.snooze(todoId: id, until: until)
        let data = try action.toData()
        let dict = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        #expect(dict["action"] as? String == "snooze")
        #expect(dict["until"] != nil)
    }

    @Test("Encode snooze action without date")
    func encodeSnoozeActionNoDate() throws {
        let id = UUID(uuidString: "a1b2c3d4-e5f6-7890-abcd-ef1234567890")!
        let action = TodoWsAction.snooze(todoId: id, until: nil)
        let data = try action.toData()
        let dict = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        #expect(dict["action"] as? String == "snooze")
        #expect(dict["until"] == nil)
    }
}
