import Foundation

// MARK: - Enums

/// Type/category of a document.
public enum DocumentType: String, Codable, Sendable, CaseIterable {
    case research
    case instructions
    case notes
    case report
    case design
    case summary
    case other

    /// Human-readable label for display.
    public var label: String {
        switch self {
        case .research: "Research"
        case .instructions: "Instructions"
        case .notes: "Notes"
        case .report: "Report"
        case .design: "Design"
        case .summary: "Summary"
        case .other: "Other"
        }
    }

    /// SF Symbol name for the document type.
    public var iconName: String {
        switch self {
        case .research: "magnifyingglass.circle"
        case .instructions: "list.bullet.rectangle"
        case .notes: "note.text"
        case .report: "doc.richtext"
        case .design: "paintbrush"
        case .summary: "text.alignleft"
        case .other: "doc"
        }
    }
}

// MARK: - Model

/// A document produced by an agent during task execution.
public struct Document: Identifiable, Hashable, Sendable {
    public let id: UUID
    public var todoId: UUID?
    public var title: String
    public var content: String
    public var docType: DocumentType
    public var createdBy: String
    public var createdAt: Date
    public var updatedAt: Date

    public init(
        id: UUID = UUID(),
        todoId: UUID? = nil,
        title: String,
        content: String,
        docType: DocumentType = .other,
        createdBy: String = "agent",
        createdAt: Date = Date(),
        updatedAt: Date = Date()
    ) {
        self.id = id
        self.todoId = todoId
        self.title = title
        self.content = content
        self.docType = docType
        self.createdBy = createdBy
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }
}

// MARK: - Codable

extension Document: Codable {
    enum CodingKeys: String, CodingKey {
        case id, todoId, title, content, docType, createdBy, createdAt, updatedAt
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UUID.self, forKey: .id)
        todoId = try container.decodeIfPresent(UUID.self, forKey: .todoId)
        title = try container.decode(String.self, forKey: .title)
        content = try container.decode(String.self, forKey: .content)
        docType = try container.decode(DocumentType.self, forKey: .docType)
        createdBy = try container.decode(String.self, forKey: .createdBy)
        createdAt = try container.decode(Date.self, forKey: .createdAt)
        updatedAt = try container.decode(Date.self, forKey: .updatedAt)
    }

    /// Decode from snake_case JSON.
    public static func decode(from data: Data) throws -> Document {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(Document.self, from: data)
    }

    /// Decode an array from snake_case JSON.
    public static func decodeArray(from data: Data) throws -> [Document] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode([Document].self, from: data)
    }
}
