import Foundation

/// REST API client for fetching todo detail with documents.
public final class TodoAPI: @unchecked Sendable {
    public let host: String
    public let port: Int

    private var baseURLString: String { "http://\(host):\(port)" }

    private var decoder: JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        d.dateDecodingStrategy = .iso8601
        return d
    }

    public init(host: String, port: Int) {
        self.host = host
        self.port = port
    }

    /// Response from GET /api/todos/{id}.
    public struct TodoDetail: Codable, Sendable {
        public let todo: TodoItem
        public let documents: [Document]
    }

    /// Fetch a single todo with its documents (included when completed).
    public func fetchTodoDetail(id: UUID) async throws -> TodoDetail {
        let idStr = id.uuidString.lowercased()
        guard let url = URL(string: "\(baseURLString)/api/todos/\(idStr)") else {
            throw URLError(.badURL)
        }
        let (data, response) = try await URLSession.shared.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw URLError(.badServerResponse)
        }
        return try decoder.decode(TodoDetail.self, from: data)
    }
}
