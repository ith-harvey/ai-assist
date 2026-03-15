import Foundation
import Observation

/// REST API client for deliverables (documents + message approval cards for a todo).
///
/// Fetches from `GET /api/todos/:id/deliverables` which returns both documents
/// and compose/reply approval cards. Merges them into a sorted `[DeliverableItem]`.
@Observable
public final class DeliverableAPI: @unchecked Sendable {
    public var deliverables: [DeliverableItem] = []
    public var isLoading = false
    public var error: String?

    public let host: String
    public let port: Int

    private var baseURLString: String { "http://\(host):\(port)" }

    public init(
        host: String = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost",
        port: Int = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
    ) {
        self.host = host
        self.port = port
    }

    /// Fetch deliverables (documents + message cards) for a specific todo.
    @MainActor
    public func fetchDeliverables(forTodoId todoId: UUID) async {
        isLoading = true
        error = nil

        let todoIdStr = todoId.uuidString.lowercased()
        guard let url = URL(string: "\(baseURLString)/api/todos/\(todoIdStr)/deliverables") else {
            error = "Invalid URL"
            isLoading = false
            return
        }

        do {
            let (data, response) = try await URLSession.shared.data(from: url)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
                error = "Server error"
                isLoading = false
                return
            }

            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            decoder.dateDecodingStrategy = .iso8601

            let result = try decoder.decode(DeliverablesResponse.self, from: data)

            // Merge documents and messages into a single sorted list
            var items: [DeliverableItem] = result.documents.map { .document($0) }
            items.append(contentsOf: result.messages.map { .message($0) })
            items.sort { $0.createdAt < $1.createdAt }

            deliverables = items
        } catch {
            self.error = error.localizedDescription
        }

        isLoading = false
    }
}

/// Response shape from `GET /api/todos/:id/deliverables`.
private struct DeliverablesResponse: Decodable {
    let documents: [Document]
    let messages: [ApprovalCard]
}
