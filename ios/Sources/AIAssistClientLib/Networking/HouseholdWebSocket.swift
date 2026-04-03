import Foundation
import Observation

// MARK: - WebSocket Message Types

/// Messages received from the household tasks WebSocket server.
/// Endpoint: `/ws/households/{id}/tasks`
enum HouseholdWsMessage {
    case tasksSync(householdId: UUID, tasks: [HouseholdTask])
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
            let householdIdStr = json["household_id"] as? String ?? ""
            let householdId = UUID(uuidString: householdIdStr) ?? UUID()
            return .tasksSync(householdId: householdId, tasks: tasks)
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

// MARK: - WebSocket Client

/// Manages household data via REST APIs and real-time task updates via WebSocket.
///
/// - REST: `/api/households`, `/api/households/{id}/members`, `/api/households/{id}/tasks`
/// - WebSocket: `/ws/households/{id}/tasks` for real-time task events
/// - Falls back to sample data when backend is unavailable
@Observable
public final class HouseholdWebSocket: @unchecked Sendable {

    // MARK: - Published State

    public var tasks: [HouseholdTask] = []
    public var members: [HouseholdMember] = []
    public var households: [Household] = []
    public var currentHouseholdId: UUID?
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

    /// Active (pending/in_progress) tasks sorted by priority then due date.
    public var activeTasks: [HouseholdTask] {
        tasks
            .filter { $0.status.isActive }
            .sorted {
                if $0.priority.sortOrder != $1.priority.sortOrder {
                    return $0.priority.sortOrder < $1.priority.sortOrder
                }
                let d0 = $0.dueDate ?? .distantFuture
                let d1 = $1.dueDate ?? .distantFuture
                return d0 < d1
            }
    }

    /// Completed tasks sorted by completion date (most recent first).
    public var completedTasks: [HouseholdTask] {
        tasks
            .filter { $0.status == .completed }
            .sorted { ($0.completedAt ?? $0.updatedAt) > ($1.completedAt ?? $1.updatedAt) }
    }

    /// Active tasks grouped by inferred category.
    public var tasksByCategory: [HouseholdTaskCategory: [HouseholdTask]] {
        Dictionary(grouping: activeTasks, by: \.category)
    }

    /// Active tasks for a specific member (by user_id).
    public func activeTasks(for userId: String) -> [HouseholdTask] {
        activeTasks.filter { $0.assignedTo == userId }
    }

    /// Count of overdue tasks.
    public var overdueCount: Int {
        activeTasks.filter(\.isOverdue).count
    }

    // MARK: - Connection

    public func connect() {
        isIntentionalDisconnect = false
        reconnectAttempt = 0
        fetchHouseholdsAndConnect()
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

    /// Fetch households via REST, pick the first one, load members, then open WebSocket.
    private func fetchHouseholdsAndConnect() {
        guard let url = URL(string: "http://\(host):\(port)/api/households") else {
            loadSampleData()
            return
        }

        Task { @MainActor in
            do {
                let (data, response) = try await session.data(from: url)
                guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200 else {
                    loadSampleData()
                    return
                }
                let decoded = try Household.decodeArray(from: data)
                self.households = decoded

                guard let household = decoded.first else {
                    // No households yet — show sample data
                    loadSampleData()
                    return
                }

                self.currentHouseholdId = household.id
                await fetchMembers(householdId: household.id)
                openWebSocket(householdId: household.id)
            } catch {
                loadSampleData()
            }
        }
    }

    /// Fetch household members via REST.
    private func fetchMembers(householdId: UUID) async {
        guard let url = URL(string: "http://\(host):\(port)/api/households/\(householdId.uuidString.lowercased())/members") else { return }

        do {
            let (data, response) = try await session.data(from: url)
            guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200 else { return }
            let decoded = try HouseholdMember.decodeArray(from: data)
            await MainActor.run {
                self.members = decoded
            }
        } catch {
            // Members will be empty — views handle this gracefully
        }
    }

    /// Open WebSocket for real-time task updates.
    private func openWebSocket(householdId: UUID) {
        let idStr = householdId.uuidString.lowercased()
        guard let url = URL(string: "ws://\(host):\(port)/ws/households/\(idStr)/tasks") else {
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
        currentHouseholdId = HouseholdMember.sampleHouseholdId
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
        case .tasksSync(_, let synced):
            tasks = synced
        case .taskCreated(let task):
            if !tasks.contains(where: { $0.id == task.id }) {
                tasks.append(task)
            }
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

    // MARK: - REST Actions

    /// Create a task via REST POST.
    public func createTask(
        title: String,
        description: String? = nil,
        priority: HouseholdTaskPriority = .medium,
        assignedTo: String? = nil,
        dueDate: Date? = nil,
        recurrence: RecurrenceRule? = nil
    ) {
        guard let householdId = currentHouseholdId else { return }

        if usingSampleData {
            let task = HouseholdTask(
                householdId: householdId, title: title, description: description,
                priority: priority, assignedTo: assignedTo, dueDate: dueDate,
                recurrence: recurrence, createdBy: "user"
            )
            tasks.append(task)
            return
        }

        let idStr = householdId.uuidString.lowercased()
        guard let url = URL(string: "http://\(host):\(port)/api/households/\(idStr)/tasks") else { return }

        var body: [String: Any] = ["title": title]
        if let description { body["description"] = description }
        body["priority"] = priority.rawValue
        if let assignedTo { body["assigned_to"] = assignedTo }
        if let dueDate {
            let formatter = ISO8601DateFormatter()
            body["due_date"] = formatter.string(from: dueDate)
        }
        if let recurrence {
            let encoder = JSONEncoder()
            if let recData = try? encoder.encode(recurrence),
               let recValue = try? JSONSerialization.jsonObject(with: recData) {
                body["recurrence"] = recValue
            }
        }

        postJSON(url: url, body: body)
    }

    /// Update a task via REST PUT.
    public func updateTask(_ task: HouseholdTask) {
        if usingSampleData {
            if let index = tasks.firstIndex(where: { $0.id == task.id }) {
                tasks[index] = task
            }
            return
        }

        guard let householdId = currentHouseholdId else { return }
        let hid = householdId.uuidString.lowercased()
        let tid = task.id.uuidString.lowercased()
        guard let url = URL(string: "http://\(host):\(port)/api/households/\(hid)/tasks/\(tid)") else { return }

        var body: [String: Any] = [
            "title": task.title,
            "status": task.status.rawValue,
            "priority": task.priority.rawValue,
        ]
        if let desc = task.description { body["description"] = desc }
        if let assignedTo = task.assignedTo { body["assigned_to"] = assignedTo }
        if let dueDate = task.dueDate {
            let formatter = ISO8601DateFormatter()
            body["due_date"] = formatter.string(from: dueDate)
        }
        if let recurrence = task.recurrence {
            let encoder = JSONEncoder()
            if let recData = try? encoder.encode(recurrence),
               let recValue = try? JSONSerialization.jsonObject(with: recData) {
                body["recurrence"] = recValue
            }
        }

        putJSON(url: url, body: body)

        // Optimistic update
        if let index = tasks.firstIndex(where: { $0.id == task.id }) {
            tasks[index] = task
        }
    }

    /// Complete a task — sets status to .completed via REST.
    public func complete(taskId: UUID) {
        if usingSampleData {
            if let index = tasks.firstIndex(where: { $0.id == taskId }) {
                tasks[index].status = .completed
                tasks[index].completedAt = Date()
                tasks[index].updatedAt = Date()
            }
            return
        }

        guard let householdId = currentHouseholdId else { return }
        let hid = householdId.uuidString.lowercased()
        let tid = taskId.uuidString.lowercased()
        guard let url = URL(string: "http://\(host):\(port)/api/households/\(hid)/tasks/\(tid)") else { return }

        putJSON(url: url, body: ["status": "completed"])

        if let index = tasks.firstIndex(where: { $0.id == taskId }) {
            tasks[index].status = .completed
            tasks[index].completedAt = Date()
            tasks[index].updatedAt = Date()
        }
    }

    /// Uncomplete a task — sets status back to .pending via REST.
    public func uncomplete(taskId: UUID) {
        if usingSampleData {
            if let index = tasks.firstIndex(where: { $0.id == taskId }) {
                tasks[index].status = .pending
                tasks[index].completedAt = nil
                tasks[index].updatedAt = Date()
            }
            return
        }

        guard let householdId = currentHouseholdId else { return }
        let hid = householdId.uuidString.lowercased()
        let tid = taskId.uuidString.lowercased()
        guard let url = URL(string: "http://\(host):\(port)/api/households/\(hid)/tasks/\(tid)") else { return }

        putJSON(url: url, body: ["status": "pending"])

        if let index = tasks.firstIndex(where: { $0.id == taskId }) {
            tasks[index].status = .pending
            tasks[index].completedAt = nil
            tasks[index].updatedAt = Date()
        }
    }

    /// Delete a task via REST DELETE.
    public func delete(taskId: UUID) {
        if usingSampleData {
            tasks.removeAll { $0.id == taskId }
            return
        }

        guard let householdId = currentHouseholdId else { return }
        let hid = householdId.uuidString.lowercased()
        let tid = taskId.uuidString.lowercased()
        guard let url = URL(string: "http://\(host):\(port)/api/households/\(hid)/tasks/\(tid)") else { return }

        var request = URLRequest(url: url)
        request.httpMethod = "DELETE"
        session.dataTask(with: request) { _, _, _ in }.resume()

        tasks.removeAll { $0.id == taskId }
    }

    // MARK: - HTTP Helpers

    private func postJSON(url: URL, body: [String: Any]) {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        session.dataTask(with: request) { _, _, _ in }.resume()
    }

    private func putJSON(url: URL, body: [String: Any]) {
        var request = URLRequest(url: url)
        request.httpMethod = "PUT"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        session.dataTask(with: request) { _, _, _ in }.resume()
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
            if let householdId = self.currentHouseholdId {
                self.openWebSocket(householdId: householdId)
            }
        }
    }

    private func reconnectDelay() -> TimeInterval {
        let delay = pow(2.0, Double(reconnectAttempt))
        return min(delay, maxReconnectDelay)
    }
}
