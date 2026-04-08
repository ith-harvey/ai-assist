import Foundation
import Observation

// MARK: - WebSocket Message Types

/// Messages received from the household tasks WebSocket server.
enum HouseholdWsMessage {
    case tasksSync([HouseholdTask])
    case taskCreated(HouseholdTask)
    case taskUpdated(HouseholdTask)
    case taskDeleted(UUID)
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
        case "ping":
            return .ping
        default:
            return nil
        }
    }
}

/// Actions sent to the household tasks WebSocket server.
enum HouseholdWsAction {
    case create(title: String, priority: HouseholdTaskPriority, assignedTo: String?, dueDate: Date?, recurrence: RecurrenceRule?, description: String?)
    case complete(taskId: UUID)
    case uncomplete(taskId: UUID)
    case update(task: HouseholdTask)
    case delete(taskId: UUID)

    func toData() throws -> Data {
        var payload: [String: Any] = [:]
        let formatter = ISO8601DateFormatter()

        switch self {
        case .create(let title, let priority, let assignedTo, let dueDate, let recurrence, let description):
            payload["action"] = "create"
            payload["title"] = title
            payload["priority"] = priority.rawValue
            if let assignedTo { payload["assigned_to"] = assignedTo }
            if let dueDate { payload["due_date"] = formatter.string(from: dueDate) }
            if let recurrence { payload["recurrence"] = recurrence.id }
            if let description { payload["description"] = description }
        case .complete(let id):
            payload = ["action": "complete", "id": id.uuidString.lowercased()]
        case .uncomplete(let id):
            payload = ["action": "uncomplete", "id": id.uuidString.lowercased()]
        case .update(let task):
            payload["action"] = "update"
            payload["id"] = task.id.uuidString.lowercased()
            payload["title"] = task.title
            payload["status"] = task.status.rawValue
            payload["priority"] = task.priority.rawValue
            if let desc = task.description { payload["description"] = desc }
            if let assignedTo = task.assignedTo { payload["assigned_to"] = assignedTo }
            if let dueDate = task.dueDate { payload["due_date"] = formatter.string(from: dueDate) }
            if let recurrence = task.recurrence { payload["recurrence"] = recurrence.id }
        case .delete(let id):
            payload = ["action": "delete", "id": id.uuidString.lowercased()]
        }
        return try JSONSerialization.data(withJSONObject: payload)
    }
}

// MARK: - WebSocket Client

/// WebSocket client for the household task system.
/// Connects to `/ws/households/{householdId}/tasks` for real-time task updates.
/// Falls back to sample data when backend is unavailable.
@Observable
public final class HouseholdWebSocket: @unchecked Sendable {

    // MARK: - Published State

    public var tasks: [HouseholdTask] = []
    public var members: [HouseholdMember] = []
    public var household: Household?
    public var isConnected: Bool = false

    // MARK: - Configuration

    public private(set) var host: String
    public private(set) var port: Int
    public var householdId: UUID?

    // MARK: - Private

    private var webSocketTask: URLSessionWebSocketTask?
    private let session: URLSession
    private var reconnectAttempt: Int = 0
    private let maxReconnectDelay: TimeInterval = 30.0
    private var isIntentionalDisconnect = false
    private var usingSampleData = false
    private var api: HouseholdAPI

    public init(
        host: String = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost",
        port: Int = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
    ) {
        self.host = host
        self.port = port
        self.session = URLSession(configuration: .default)
        self.api = HouseholdAPI(host: host, port: port)
    }

    // MARK: - Computed

    /// Active tasks sorted by due date then priority.
    public var activeTasks: [HouseholdTask] {
        tasks
            .filter { $0.isActive }
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

    /// Active tasks grouped by priority.
    public var tasksByPriority: [HouseholdTaskPriority: [HouseholdTask]] {
        Dictionary(grouping: activeTasks, by: \.priority)
    }

    /// Active tasks for a specific assignee.
    public func activeTasks(for userId: String) -> [HouseholdTask] {
        activeTasks.filter { $0.assignedTo == userId }
    }

    /// Count of overdue tasks.
    public var overdueCount: Int {
        activeTasks.filter(\.isOverdue).count
    }

    // MARK: - Connection

    /// Connect to the household WebSocket. If no householdId is set, fetches from REST first.
    public func connect() {
        isIntentionalDisconnect = false
        reconnectAttempt = 0

        if let householdId {
            openConnection(householdId: householdId)
        } else {
            // Try to discover the user's household via REST
            Task { @MainActor in
                await discoverAndConnect()
            }
        }
    }

    /// Set the household and connect.
    public func connect(householdId: UUID) {
        self.householdId = householdId
        isIntentionalDisconnect = false
        reconnectAttempt = 0
        openConnection(householdId: householdId)
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
        self.api = HouseholdAPI(host: host, port: port)
        if wasConnected {
            connect()
        }
    }

    /// Fetch household list, pick the first one, load members, then connect WebSocket.
    @MainActor
    private func discoverAndConnect() async {
        do {
            let households = try await api.listHouseholds()
            if let first = households.first {
                self.household = first
                self.householdId = first.id
                let members = try await api.listMembers(householdId: first.id)
                self.members = members
                openConnection(householdId: first.id)
            } else {
                // No household yet — load sample data for preview
                loadSampleData()
            }
        } catch {
            loadSampleData()
        }
    }

    private func openConnection(householdId: UUID) {
        let idStr = householdId.uuidString.lowercased()
        guard let url = URL(string: "ws://\(host):\(port)/ws/households/\(idStr)/tasks") else {
            loadSampleData()
            return
        }
        let task = session.webSocketTask(with: url)
        self.webSocketTask = task
        task.resume()

        DispatchQueue.main.asyncAfter(deadline: .now() + 4.0) { [weak self] in
            guard let self else { return }
            if !self.isConnected && self.tasks.isEmpty {
                self.loadSampleData()
            }
        }

        reconnectAttempt = 0
        usingSampleData = false
        receiveMessage()

        // Also fetch members via REST
        Task { @MainActor in
            if let members = try? await api.listMembers(householdId: householdId) {
                self.members = members
            }
        }
    }

    private func loadSampleData() {
        usingSampleData = true
        isConnected = true
        household = Household.sample
        householdId = Household.sample.id
        tasks = HouseholdTask.samples
        members = HouseholdMember.samples
    }

    // MARK: - Receiving

    private func receiveMessage() {
        webSocketTask?.receive { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let message):
                // Mark connected on first successful receive (handshake complete)
                if !self.isConnected {
                    DispatchQueue.main.async { self.isConnected = true }
                }
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
        case .ping:
            break
        }
    }

    // MARK: - Actions

    public func complete(taskId: UUID) {
        if usingSampleData {
            if let index = tasks.firstIndex(where: { $0.id == taskId }) {
                tasks[index].status = .completed
                tasks[index].completedAt = Date()
                tasks[index].updatedAt = Date()
            }
            return
        }
        send(action: .complete(taskId: taskId))
        if let index = tasks.firstIndex(where: { $0.id == taskId }) {
            tasks[index].status = .completed
            tasks[index].completedAt = Date()
            tasks[index].updatedAt = Date()
        }
    }

    public func uncomplete(taskId: UUID) {
        if usingSampleData {
            if let index = tasks.firstIndex(where: { $0.id == taskId }) {
                tasks[index].status = .pending
                tasks[index].completedAt = nil
                tasks[index].updatedAt = Date()
            }
            return
        }
        send(action: .uncomplete(taskId: taskId))
        if let index = tasks.firstIndex(where: { $0.id == taskId }) {
            tasks[index].status = .pending
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

    public func createTask(title: String, priority: HouseholdTaskPriority = .medium, assignedTo: String? = nil, dueDate: Date? = nil, recurrence: RecurrenceRule? = nil, description: String? = nil) {
        guard let householdId else { return }

        let newTask = HouseholdTask(
            householdId: householdId,
            title: title,
            description: description,
            priority: priority,
            assignedTo: assignedTo,
            dueDate: dueDate,
            recurrence: recurrence
        )

        if usingSampleData {
            tasks.append(newTask)
            return
        }
        send(action: .create(title: title, priority: priority, assignedTo: assignedTo, dueDate: dueDate, recurrence: recurrence, description: description))
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
            if let householdId = self.householdId {
                self.openConnection(householdId: householdId)
            }
        }
    }

    private func reconnectDelay() -> TimeInterval {
        let delay = pow(2.0, Double(reconnectAttempt))
        return min(delay, maxReconnectDelay)
    }
}
