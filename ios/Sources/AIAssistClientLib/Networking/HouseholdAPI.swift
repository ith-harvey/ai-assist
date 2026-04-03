import Foundation

/// REST API client for household management — CRUD for households, members, and tasks.
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

    // MARK: - Households

    /// Create a new household. The creator is automatically added as Owner.
    public func createHousehold(name: String) async throws -> Household {
        let body: [String: String] = ["name": name]
        return try await post("/api/households", body: body)
    }

    /// List all households the current user belongs to.
    public func listHouseholds() async throws -> [Household] {
        try await get("/api/households")
    }

    /// Get a single household by ID.
    public func getHousehold(id: UUID) async throws -> Household {
        try await get("/api/households/\(id.uuidString.lowercased())")
    }

    /// Update a household's name.
    public func updateHousehold(id: UUID, name: String) async throws -> Household {
        let body: [String: String] = ["name": name]
        return try await put("/api/households/\(id.uuidString.lowercased())", body: body)
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
        return try await post("/api/households/\(householdId.uuidString.lowercased())/members", body: body)
    }

    /// List all members of a household.
    public func listMembers(householdId: UUID) async throws -> [HouseholdMember] {
        try await get("/api/households/\(householdId.uuidString.lowercased())/members")
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
        return try await postJSON("/api/households/\(householdId.uuidString.lowercased())/tasks", body: body)
    }

    /// List tasks in a household, optionally filtered by status.
    public func listTasks(householdId: UUID, status: HouseholdTaskStatus? = nil) async throws -> [HouseholdTask] {
        var path = "/api/households/\(householdId.uuidString.lowercased())/tasks"
        if let status { path += "?status=\(status.rawValue)" }
        return try await get(path)
    }

    /// Get tasks assigned to the current user across all households.
    public func myTasks() async throws -> [HouseholdTask] {
        try await get("/api/households/tasks/mine")
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
