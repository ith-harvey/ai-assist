import Foundation

/// Status of a reply card in the queue.
/// Matches Rust `CardStatus` with `#[serde(rename_all = "snake_case")]`.
public enum CardStatus: String, Codable, Sendable {
    case pending
    case approved
    case dismissed
    case expired
    case sent
}

/// A reply suggestion card.
/// Mirrors Rust `ReplyCard` struct — all fields use snake_case in JSON.
public struct ReplyCard: Identifiable, Sendable {
    public let id: UUID
    public let conversationId: String
    public let sourceMessage: String
    public let sourceSender: String
    public let suggestedReply: String
    public let confidence: Float
    public var status: CardStatus
    public let createdAt: String
    public let expiresAt: String
    public let channel: String
    public let updatedAt: String
    /// Email thread context — previous messages in the conversation.
    /// Empty array when no thread context is available (backwards compatible).
    public let thread: [ThreadMessage]
    /// Email thread with full headers (From/To/CC/Subject/Message-ID) for rich display.
    /// Empty array when not available (backwards compatible).
    public let emailThread: [EmailMessage]
}

extension ReplyCard: Codable {
    enum CodingKeys: String, CodingKey {
        case id, conversationId, sourceMessage, sourceSender, suggestedReply
        case confidence, status, createdAt, expiresAt, channel, updatedAt, thread, emailThread
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UUID.self, forKey: .id)
        conversationId = try container.decode(String.self, forKey: .conversationId)
        sourceMessage = try container.decode(String.self, forKey: .sourceMessage)
        sourceSender = try container.decode(String.self, forKey: .sourceSender)
        suggestedReply = try container.decode(String.self, forKey: .suggestedReply)
        confidence = try container.decode(Float.self, forKey: .confidence)
        status = try container.decode(CardStatus.self, forKey: .status)
        createdAt = try container.decode(String.self, forKey: .createdAt)
        expiresAt = try container.decode(String.self, forKey: .expiresAt)
        channel = try container.decode(String.self, forKey: .channel)
        updatedAt = try container.decode(String.self, forKey: .updatedAt)
        thread = try container.decodeIfPresent([ThreadMessage].self, forKey: .thread) ?? []
        emailThread = try container.decodeIfPresent([EmailMessage].self, forKey: .emailThread) ?? []
    }
}

extension ReplyCard {
    /// Decode a `ReplyCard` from snake_case JSON data.
    public static func decode(from data: Data) throws -> ReplyCard {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(ReplyCard.self, from: data)
    }

    /// Decode an array of `ReplyCard` from snake_case JSON data.
    public static func decodeArray(from data: Data) throws -> [ReplyCard] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode([ReplyCard].self, from: data)
    }
}
