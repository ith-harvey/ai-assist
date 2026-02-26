import Foundation
import Observation

// MARK: - WebSocket Message Types

/// Messages received from the todo WebSocket server.
enum TodoWsMessage {
    case todosSync([TodoItem])
    case todoCreated(TodoItem)
    case todoUpdated(TodoItem)
    case todoDeleted(UUID)
    case ping

    static func decode(from data: Data) -> TodoWsMessage? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = json["type"] as? String else { return nil }

        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601

        switch type {
        case "todos_sync":
            guard let todosData = try? JSONSerialization.data(withJSONObject: json["todos"] ?? []),
                  let todos = try? decoder.decode([TodoItem].self, from: todosData) else { return nil }
            return .todosSync(todos)
        case "todo_created":
            guard let todoData = try? JSONSerialization.data(withJSONObject: json["todo"] ?? [:]),
                  let todo = try? decoder.decode(TodoItem.self, from: todoData) else { return nil }
            return .todoCreated(todo)
        case "todo_updated":
            guard let todoData = try? JSONSerialization.data(withJSONObject: json["todo"] ?? [:]),
                  let todo = try? decoder.decode(TodoItem.self, from: todoData) else { return nil }
            return .todoUpdated(todo)
        case "todo_deleted":
            guard let idString = json["id"] as? String,
                  let id = UUID(uuidString: idString) else { return nil }
            return .todoDeleted(id)
        case "ping":
            return .ping
        default:
            return nil
        }
    }
}

/// Actions sent to the todo WebSocket server.
enum TodoWsAction: Encodable {
    case complete(todoId: UUID)
    case delete(todoId: UUID)
    case snooze(todoId: UUID, until: Date?)

    func toData() throws -> Data {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        encoder.dateEncodingStrategy = .iso8601

        let payload: [String: Any]
        switch self {
        case .complete(let id):
            payload = ["action": "complete", "id": id.uuidString.lowercased()]
        case .delete(let id):
            payload = ["action": "delete", "id": id.uuidString.lowercased()]
        case .snooze(let id, let until):
            var p: [String: Any] = ["action": "snooze", "id": id.uuidString.lowercased()]
            if let until {
                let formatter = ISO8601DateFormatter()
                p["until"] = formatter.string(from: until)
            }
            payload = p
        }
        return try JSONSerialization.data(withJSONObject: payload)
    }
}

// MARK: - WebSocket Client

/// WebSocket client for the to-do system.
/// Mirrors `CardWebSocket` pattern — connects to `/ws/todos`, syncs state, sends actions.
/// Uses hardcoded sample data until backend is ready.
@Observable
public final class TodoWebSocket: @unchecked Sendable {

    // MARK: - Published State

    public var todos: [TodoItem] = []
    public var isConnected: Bool = false

    // MARK: - Configuration

    public private(set) var host: String
    public private(set) var port: Int

    // MARK: - Private

    private var webSocketTask: URLSessionWebSocketTask?
    private let session: URLSession
    private var reconnectAttempt: Int = 0
    private let maxReconnectDelay: TimeInterval = 30.0
    private var isIntentionalDisconnect = false
    /// True when using hardcoded data (backend not available).
    private var usingSampleData = false

    public init(host: String = "192.168.0.5", port: Int = 8080) {
        self.host = host
        self.port = port
        self.session = URLSession(configuration: .default)
    }

    // MARK: - Computed

    /// Active (non-completed, non-snoozed) todos sorted by priority.
    public var activeTodos: [TodoItem] {
        todos
            .filter { $0.status.isActive }
            .sorted { $0.priority < $1.priority }
    }

    /// Completed todos.
    public var completedTodos: [TodoItem] {
        todos
            .filter { $0.status == .completed }
            .sorted { $0.updatedAt > $1.updatedAt }
    }

    /// Snoozed todos.
    public var snoozedTodos: [TodoItem] {
        todos
            .filter { $0.status == .snoozed }
            .sorted { ($0.snoozedUntil ?? .distantFuture) < ($1.snoozedUntil ?? .distantFuture) }
    }

    /// Count of items that need attention (ready for review + waiting on you).
    public var approvalCount: Int {
        todos.filter { $0.status == .readyForReview || $0.status == .waitingOnYou }.count
    }

    // MARK: - Connection

    public func connect() {
        isIntentionalDisconnect = false
        reconnectAttempt = 0
        openConnection()
    }

    public func disconnect() {
        isIntentionalDisconnect = true
        webSocketTask?.cancel(with: .normalClosure, reason: nil)
        webSocketTask = nil
        isConnected = false
    }

    public func updateServer(host: String, port: Int) {
        let wasConnected = isConnected
        disconnect()
        self.host = host
        self.port = port
        if wasConnected {
            connect()
        }
    }

    private func openConnection() {
        guard let url = URL(string: "ws://\(host):\(port)/ws/todos") else {
            loadSampleData()
            return
        }
        let task = session.webSocketTask(with: url)
        self.webSocketTask = task
        task.resume()

        // Give the connection a moment, then check if it actually connected.
        // If backend isn't ready, fall back to sample data.
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0) { [weak self] in
            guard let self else { return }
            if !self.isConnected && self.todos.isEmpty {
                self.loadSampleData()
            }
        }

        isConnected = true
        reconnectAttempt = 0
        usingSampleData = false
        receiveMessage()
    }

    /// Load hardcoded sample data when backend isn't available.
    private func loadSampleData() {
        usingSampleData = true
        isConnected = true
        todos = TodoItem.samples
    }

    // MARK: - Receiving

    private func receiveMessage() {
        webSocketTask?.receive { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let message):
                self.handleMessage(message)
                self.receiveMessage()
            case .failure:
                self.handleDisconnect()
            }
        }
    }

    private func handleMessage(_ message: URLSessionWebSocketTask.Message) {
        let data: Data
        switch message {
        case .string(let text):
            guard let textData = text.data(using: .utf8) else { return }
            data = textData
        case .data(let raw):
            data = raw
        @unknown default:
            return
        }

        guard let wsMessage = TodoWsMessage.decode(from: data) else { return }

        DispatchQueue.main.async { [weak self] in
            self?.applyMessage(wsMessage)
        }
    }

    private func applyMessage(_ message: TodoWsMessage) {
        switch message {
        case .todosSync(let synced):
            todos = synced
        case .todoCreated(let todo):
            todos.append(todo)
        case .todoUpdated(let todo):
            if let index = todos.firstIndex(where: { $0.id == todo.id }) {
                todos[index] = todo
            }
        case .todoDeleted(let id):
            todos.removeAll { $0.id == id }
        case .ping:
            break
        }
    }

    // MARK: - Actions

    public func complete(todoId: UUID) {
        if usingSampleData {
            if let index = todos.firstIndex(where: { $0.id == todoId }) {
                todos[index].status = .completed
                todos[index].updatedAt = Date()
            }
            return
        }
        send(action: .complete(todoId: todoId))
        if let index = todos.firstIndex(where: { $0.id == todoId }) {
            todos[index].status = .completed
            todos[index].updatedAt = Date()
        }
    }

    public func delete(todoId: UUID) {
        if usingSampleData {
            todos.removeAll { $0.id == todoId }
            return
        }
        send(action: .delete(todoId: todoId))
        todos.removeAll { $0.id == todoId }
    }

    public func snooze(todoId: UUID, until: Date? = nil) {
        let snoozeTo = until ?? Calendar.current.date(byAdding: .hour, value: 4, to: Date())
        if usingSampleData {
            if let index = todos.firstIndex(where: { $0.id == todoId }) {
                todos[index].status = .snoozed
                todos[index].snoozedUntil = snoozeTo
                todos[index].updatedAt = Date()
            }
            return
        }
        send(action: .snooze(todoId: todoId, until: snoozeTo))
        if let index = todos.firstIndex(where: { $0.id == todoId }) {
            todos[index].status = .snoozed
            todos[index].snoozedUntil = snoozeTo
            todos[index].updatedAt = Date()
        }
    }

    private func send(action: TodoWsAction) {
        guard let data = try? action.toData(),
              let text = String(data: data, encoding: .utf8) else { return }
        webSocketTask?.send(.string(text)) { _ in }
    }

    // MARK: - Reconnection

    private func handleDisconnect() {
        DispatchQueue.main.async { [weak self] in
            self?.isConnected = false
        }

        guard !isIntentionalDisconnect else { return }

        // If we were using real data, fall back to sample
        if !usingSampleData && todos.isEmpty {
            DispatchQueue.main.async { [weak self] in
                self?.loadSampleData()
            }
            return
        }

        let delay = reconnectDelay()
        reconnectAttempt += 1

        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self, !self.isIntentionalDisconnect else { return }
            self.openConnection()
        }
    }

    /// Exponential backoff: 1s, 2s, 4s, 8s, ... capped at `maxReconnectDelay`.
    public func reconnectDelay() -> TimeInterval {
        let delay = pow(2.0, Double(reconnectAttempt))
        return min(delay, maxReconnectDelay)
    }
}
