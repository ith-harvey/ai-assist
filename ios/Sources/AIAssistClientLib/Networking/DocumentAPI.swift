import Foundation
import Observation

/// REST API client for documents.
///
/// Fetches documents from the ai-assist backend via `/api/documents` endpoints.
/// Uses `@Observable` pattern for SwiftUI integration.
@Observable
public final class DocumentAPI: @unchecked Sendable {
    public var documents: [Document] = []
    public var isLoading = false
    public var error: String?

    private let config: ServerConfig

    private var decoder: JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        d.dateDecodingStrategy = .iso8601
        return d
    }

    public init(config: ServerConfig = ServerConfig()) {
        self.config = config
    }

    /// Fetch documents for a specific todo.
    @MainActor
    public func fetchDocuments(forTodoId todoId: UUID) async {
        isLoading = true
        error = nil

        let todoIdStr = todoId.uuidString.lowercased()
        guard let url = URL(string: "\(config.baseURL)/api/documents?todo_id=\(todoIdStr)") else {
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

            let wrapper = try decoder.decode(DocumentListResponse.self, from: data)
            documents = wrapper.documents
        } catch {
            self.error = error.localizedDescription
        }

        isLoading = false
    }

    /// Fetch all documents (up to limit).
    @MainActor
    public func fetchAllDocuments(limit: Int = 50) async {
        isLoading = true
        error = nil

        guard let url = URL(string: "\(config.baseURL)/api/documents?limit=\(limit)") else {
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

            let wrapper = try decoder.decode(DocumentListResponse.self, from: data)
            documents = wrapper.documents
        } catch {
            self.error = error.localizedDescription
        }

        isLoading = false
    }

    /// Fetch a single document by ID.
    public func fetchDocument(id: UUID) async throws -> Document {
        let idStr = id.uuidString.lowercased()
        guard let url = URL(string: "\(config.baseURL)/api/documents/\(idStr)") else {
            throw URLError(.badURL)
        }
        let (data, response) = try await URLSession.shared.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw URLError(.badServerResponse)
        }
        return try decoder.decode(Document.self, from: data)
    }
}

/// Wrapper for the list endpoint response.
private struct DocumentListResponse: Codable {
    let documents: [Document]
}
