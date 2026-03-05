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

    public let host: String
    public let port: Int

    private var baseURLString: String { "http://\(host):\(port)" }

    private var decoder: JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        d.dateDecodingStrategy = .iso8601
        return d
    }

    public init(host: String = "localhost", port: Int = 3001) {
        self.host = host
        self.port = port
    }

    /// Fetch documents for a specific todo.
    @MainActor
    public func fetchDocuments(forTodoId todoId: UUID) async {
        isLoading = true
        error = nil

        let todoIdStr = todoId.uuidString.lowercased()
        guard let url = URL(string: "\(baseURLString)/api/documents?todo_id=\(todoIdStr)") else {
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

        guard let url = URL(string: "\(baseURLString)/api/documents?limit=\(limit)") else {
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
        guard let url = URL(string: "\(baseURLString)/api/documents/\(idStr)") else {
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
