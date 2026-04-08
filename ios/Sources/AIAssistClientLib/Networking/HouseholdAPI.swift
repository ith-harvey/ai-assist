import Foundation

/// REST API client for household management — CRUD for households, members, and tasks.
///
/// All list endpoints return wrapped envelopes (`{"households": [...]}` etc.)
/// so we decode through typed response structs, not bare arrays.
public final class HouseholdAPI: @unchecked Sendable {
    public let host: String
    public let port: Int

    private var baseURL: String { "http://\(host):\(port)" }

    private var decoder: JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        d.dateDecodingStrategy = .iso8601
        return d
    }

    private var encoder: JSONEncoder {
        let e = JSONEncoder()
        e.keyEncodingStrategy = .convertToSnakeCase
        e.dateEncodingStrategy = .iso8601
        return e
    }

    public init(
        host: String = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost",
        port: Int = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
    ) {
        self.host = host
        self.port = port
    }

    // MARK: - Response Envelopes

    private struct HouseholdCreateResponse: Decodable {
        let id: String
        let household: Household
    }

    private struct HouseholdsListResponse: Decodable {
        let households: [Household]
    }

    private struct HouseholdUpdateResponse: Decodable {
        let household: Household
    }

    private struct MemberCreateResponse: Decodable {
        let id: String
        let member: HouseholdMember
    }

    private struct MembersListResponse: Decodable {
        let members: [HouseholdMember]
    }

    private struct TaskCreateResponse: Decodable {
        let id: String
        let task: HouseholdTask
    }

    private struct TasksListResponse: Decodable {
        let tasks: [HouseholdTask]
    }

    private struct TaskUpdateResponse: Decodable {
        let task: HouseholdTask
    }

    // MARK: - Households

    /// Create a new household. The creator is automatically added as Owner.
    public func createHousehold(name: String) async throws -> Household {
        let body: [String: String] = ["name": name]
        let envelope: HouseholdCreateResponse = try await post("/api/households", body: body)
        return envelope.household
    }

    /// List all households the current user belongs to.
    public func listHouseholds() async throws -> [Household] {
        let envelope: HouseholdsListResponse = try await get("/api/households")
        return envelope.households
    }

    /// Get a single household by ID. Returns bare object (no envelope).
    public func getHousehold(id: UUID) async throws -> Household {
        try await get("/api/households/\(id.uuidString.lowercased())")
    }

    /// Update a household's name.
    public func updateHousehold(id: UUID, name: String) async throws -> Household {
        let body: [String: String] = ["name": name]
        let envelope: HouseholdUpdateResponse = try await put("/api/households/\(id.uuidString.lowercased())", body: body)
        return envelope.household
    }

    /// Delete a household (cascades to members and tasks).
    public func deleteHousehold(id: UUID) async throws {
        try await delete("/api/households/\(id.uuidString.lowercased())")
    }

    // MARK: - Members

    /// Add a member to a household.
    public func addMember(householdId: UUID, userId: String, displayName: String, role: HouseholdRole? = nil) async throws -> HouseholdMember {
        var body: [String: String] = [
            "user_id": userId,
            "display_name": displayName,
        ]
        if let role { body["role"] = role.rawValue }
        let envelope: MemberCreateResponse = try await post("/api/households/\(householdId.uuidString.lowercased())/members", body: body)
        return envelope.member
    }

    /// List all members of a household.
    public func listMembers(householdId: UUID) async throws -> [HouseholdMember] {
        let envelope: MembersListResponse = try await get("/api/households/\(householdId.uuidString.lowercased())/members")
        return envelope.members
    }

    /// Remove a member from a household.
    public func removeMember(householdId: UUID, userId: String) async throws {
        try await delete("/api/households/\(householdId.uuidString.lowercased())/members/\(userId)")
    }

    // MARK: - Tasks

    /// Create a task in a household.
    public func createTask(householdId: UUID, title: String, description: String? = nil, priority: HouseholdTaskPriority? = nil, assignedTo: String? = nil, dueDate: Date? = nil, recurrence: RecurrenceRule? = nil) async throws -> HouseholdTask {
        var body: [String: Any] = ["title": title]
        if let description { body["description"] = description }
        if let priority { body["priority"] = priority.rawValue }
        if let assignedTo { body["assigned_to"] = assignedTo }
        if let dueDate {
            let formatter = ISO8601DateFormatter()
            body["due_date"] = formatter.string(from: dueDate)
        }
        if let recurrence { body["recurrence"] = recurrence.id }
        let envelope: TaskCreateResponse = try await postJSON("/api/households/\(householdId.uuidString.lowercased())/tasks", body: body)
        return envelope.task
    }

    /// List tasks in a household, optionally filtered by status.
    public func listTasks(householdId: UUID, status: HouseholdTaskStatus? = nil) async throws -> [HouseholdTask] {
        var path = "/api/households/\(householdId.uuidString.lowercased())/tasks"
        if let status { path += "?status=\(status.rawValue)" }
        let envelope: TasksListResponse = try await get(path)
        return envelope.tasks
    }

    /// Get tasks assigned to the current user across all households.
    public func myTasks() async throws -> [HouseholdTask] {
        let envelope: TasksListResponse = try await get("/api/households/tasks/mine")
        return envelope.tasks
    }

    // MARK: - HTTP Helpers

    private func get<T: Decodable>(_ path: String) async throws -> T {
        guard let url = URL(string: "\(baseURL)\(path)") else { throw URLError(.badURL) }
        let (data, response) = try await URLSession.shared.data(from: url)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw URLError(.badServerResponse)
        }
        return try decoder.decode(T.self, from: data)
    }

    private func post<T: Decodable, B: Encodable>(_ path: String, body: B) async throws -> T {
        guard let url = URL(string: "\(baseURL)\(path)") else { throw URLError(.badURL) }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try encoder.encode(body)
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw URLError(.badServerResponse)
        }
        return try decoder.decode(T.self, from: data)
    }

    private func postJSON<T: Decodable>(_ path: String, body: [String: Any]) async throws -> T {
        guard let url = URL(string: "\(baseURL)\(path)") else { throw URLError(.badURL) }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw URLError(.badServerResponse)
        }
        return try decoder.decode(T.self, from: data)
    }

    private func put<T: Decodable, B: Encodable>(_ path: String, body: B) async throws -> T {
        guard let url = URL(string: "\(baseURL)\(path)") else { throw URLError(.badURL) }
        var request = URLRequest(url: url)
        request.httpMethod = "PUT"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try encoder.encode(body)
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw URLError(.badServerResponse)
        }
        return try decoder.decode(T.self, from: data)
    }

    private func delete(_ path: String) async throws {
        guard let url = URL(string: "\(baseURL)\(path)") else { throw URLError(.badURL) }
        var request = URLRequest(url: url)
        request.httpMethod = "DELETE"
        let (_, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw URLError(.badServerResponse)
        }
    }
}
