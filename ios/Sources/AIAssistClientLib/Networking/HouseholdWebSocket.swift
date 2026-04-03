import Foundation
import Observation

// MARK: - WebSocket Message Types

/// Messages received from the household tasks WebSocket server.
enum HouseholdWsMessage {
    case tasksSync([HouseholdTask])
    case taskCreated(HouseholdTask)
    case taskUpdated(HouseholdTask)
    case taskDeleted(UUID)
    case membersSync([HouseholdMember])
    case ping

    static func decode(from data: Data) -> HouseholdWsMessage? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = json["type"] as? String else { return nil }

        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601

        switch type {
        case "tasks_sync":
            guard let tasksData = try? JSONSerialization.data(withJSONObject: json["tasks"] ?? []),
                  let tasks = try? decoder.decode([HouseholdTask].self, from: tasksData) else { return nil }
            return .tasksSync(tasks)
        case "task_created":
            guard let taskData = try? JSONSerialization.data(withJSONObject: json["task"] ?? [:]),
                  let task = try? decoder.decode(HouseholdTask.self, from: taskData) else { return nil }
            return .taskCreated(task)
        case "task_updated":
            guard let taskData = try? JSONSerialization.data(withJSONObject: json["task"] ?? [:]),
                  let task = try? decoder.decode(HouseholdTask.self, from: taskData) else { return nil }
            return .taskUpdated(task)
        case "task_deleted":
            guard let idString = json["id"] as? String,
                  let id = UUID(uuidString: idString) else { return nil }
            return .taskDeleted(id)
        case "members_sync":
            guard let membersData = try? JSONSerialization.data(withJSONObject: json["members"] ?? []),
                  let members = try? decoder.decode([HouseholdMember].self, from: membersData) else { return nil }
            return .membersSync(members)
        case "ping":
            return .ping
        default:
            return nil
        }
    }
}

/// Actions sent to the household tasks WebSocket server.
enum HouseholdWsAction {
    case create(title: String, category: HouseholdTaskCategory, assigneeId: UUID?, dueDate: Date?, recurrence: RecurrenceRule?, notes: String?)
    case complete(taskId: UUID)
    case uncomplete(taskId: UUID)
    case update(task: HouseholdTask)
    case delete(taskId: UUID)

    func toData() throws -> Data {
        var payload: [String: Any] = [:]
        let formatter = ISO8601DateFormatter()

        switch self {
        case .create(let title, let category, let assigneeId, let dueDate, let recurrence, let notes):
            payload["action"] = "create"
            payload["title"] = title
            payload["category"] = category.rawValue
            if let assigneeId { payload["assignee_id"] = assigneeId.uuidString.lowercased() }
            if let dueDate { payload["due_date"] = formatter.string(from: dueDate) }
            if let recurrence { payload["recurrence"] = recurrence.rawValue }
            if let notes { payload["notes"] = notes }
        case .complete(let id):
            payload = ["action": "complete", "id": id.uuidString.lowercased()]
        case .uncomplete(let id):
            payload = ["action": "uncomplete", "id": id.uuidString.lowercased()]
        case .update(let task):
            payload["action"] = "update"
            payload["id"] = task.id.uuidString.lowercased()
            payload["title"] = task.title
            payload["category"] = task.category.rawValue
            payload["is_completed"] = task.isCompleted
            if let notes = task.notes { payload["notes"] = notes }
            if let assigneeId = task.assigneeId { payload["assignee_id"] = assigneeId.uuidString.lowercased() }
            if let dueDate = task.dueDate { payload["due_date"] = formatter.string(from: dueDate) }
            if let recurrence = task.recurrence { payload["recurrence"] = recurrence.rawValue }
        case .delete(let id):
            payload = ["action": "delete", "id": id.uuidString.lowercased()]
        }
        return try JSONSerialization.data(withJSONObject: payload)
    }
}

// MARK: - WebSocket Client

/// WebSocket client for the household task system.
/// Connects to `/ws/household`, syncs tasks and members, sends actions.
/// Falls back to sample data when backend is unavailable.
@Observable
public final class HouseholdWebSocket: @unchecked Sendable {

    // MARK: - Published State

    public var tasks: [HouseholdTask] = []
    public var members: [HouseholdMember] = []
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
    private var usingSampleData = false

    public init(
        host: String = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost",
        port: Int = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
    ) {
        self.host = host
        self.port = port
        self.session = URLSession(configuration: .default)
    }

    // MARK: - Computed

    /// Active (not completed) tasks sorted by due date then creation.
    public var activeTasks: [HouseholdTask] {
        tasks
            .filter { !$0.isCompleted }
            .sorted {
                let d0 = $0.dueDate ?? .distantFuture
                let d1 = $1.dueDate ?? .distantFuture
                return d0 < d1
            }
    }

    /// Completed tasks sorted by completion date (most recent first).
    public var completedTasks: [HouseholdTask] {
        tasks
            .filter { $0.isCompleted }
            .sorted { ($0.completedAt ?? $0.updatedAt) > ($1.completedAt ?? $1.updatedAt) }
    }

    /// Active tasks grouped by category.
    public var tasksByCategory: [HouseholdTaskCategory: [HouseholdTask]] {
        Dictionary(grouping: activeTasks, by: \.category)
    }

    /// Active tasks for a specific assignee.
    public func activeTasks(for memberId: UUID) -> [HouseholdTask] {
        activeTasks.filter { $0.assigneeId == memberId }
    }

    /// Count of overdue tasks.
    public var overdueCount: Int {
        activeTasks.filter(\.isOverdue).count
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
        guard let url = URL(string: "ws://\(host):\(port)/ws/household") else {
            loadSampleData()
            return
        }
        let task = session.webSocketTask(with: url)
        self.webSocketTask = task
        task.resume()

        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0) { [weak self] in
            guard let self else { return }
            if !self.isConnected && self.tasks.isEmpty {
                self.loadSampleData()
            }
        }

        isConnected = true
        reconnectAttempt = 0
        usingSampleData = false
        receiveMessage()
    }

    private func loadSampleData() {
        usingSampleData = true
        isConnected = true
        tasks = HouseholdTask.samples
        members = HouseholdMember.samples
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

        guard let wsMessage = HouseholdWsMessage.decode(from: data) else { return }

        DispatchQueue.main.async { [weak self] in
            self?.applyMessage(wsMessage)
        }
    }

    private func applyMessage(_ message: HouseholdWsMessage) {
        switch message {
        case .tasksSync(let synced):
            tasks = synced
        case .taskCreated(let task):
            tasks.append(task)
        case .taskUpdated(let task):
            if let index = tasks.firstIndex(where: { $0.id == task.id }) {
                tasks[index] = task
            }
        case .taskDeleted(let id):
            tasks.removeAll { $0.id == id }
        case .membersSync(let synced):
            members = synced
        case .ping:
            break
        }
    }

    // MARK: - Actions

    public func complete(taskId: UUID) {
        if usingSampleData {
            if let index = tasks.firstIndex(where: { $0.id == taskId }) {
                tasks[index].isCompleted = true
                tasks[index].completedAt = Date()
                tasks[index].updatedAt = Date()
            }
            return
        }
        send(action: .complete(taskId: taskId))
        if let index = tasks.firstIndex(where: { $0.id == taskId }) {
            tasks[index].isCompleted = true
            tasks[index].completedAt = Date()
            tasks[index].updatedAt = Date()
        }
    }

    public func uncomplete(taskId: UUID) {
        if usingSampleData {
            if let index = tasks.firstIndex(where: { $0.id == taskId }) {
                tasks[index].isCompleted = false
                tasks[index].completedAt = nil
                tasks[index].updatedAt = Date()
            }
            return
        }
        send(action: .uncomplete(taskId: taskId))
        if let index = tasks.firstIndex(where: { $0.id == taskId }) {
            tasks[index].isCompleted = false
            tasks[index].completedAt = nil
            tasks[index].updatedAt = Date()
        }
    }

    public func delete(taskId: UUID) {
        if usingSampleData {
            tasks.removeAll { $0.id == taskId }
            return
        }
        send(action: .delete(taskId: taskId))
        tasks.removeAll { $0.id == taskId }
    }

    public func createTask(title: String, category: HouseholdTaskCategory, assigneeId: UUID? = nil, dueDate: Date? = nil, recurrence: RecurrenceRule? = nil, notes: String? = nil) {
        let newTask = HouseholdTask(
            title: title,
            notes: notes,
            category: category,
            assigneeId: assigneeId,
            assigneeName: members.first(where: { $0.id == assigneeId })?.name,
            dueDate: dueDate,
            recurrence: recurrence
        )

        if usingSampleData {
            tasks.append(newTask)
            return
        }
        send(action: .create(title: title, category: category, assigneeId: assigneeId, dueDate: dueDate, recurrence: recurrence, notes: notes))
        tasks.append(newTask)
    }

    public func updateTask(_ task: HouseholdTask) {
        if usingSampleData {
            if let index = tasks.firstIndex(where: { $0.id == task.id }) {
                tasks[index] = task
            }
            return
        }
        send(action: .update(task: task))
        if let index = tasks.firstIndex(where: { $0.id == task.id }) {
            tasks[index] = task
        }
    }

    private func send(action: HouseholdWsAction) {
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

        if !usingSampleData && tasks.isEmpty {
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

    private func reconnectDelay() -> TimeInterval {
        let delay = pow(2.0, Double(reconnectAttempt))
        return min(delay, maxReconnectDelay)
    }
}
