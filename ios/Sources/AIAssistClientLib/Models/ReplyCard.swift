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
public struct ReplyCard: Codable, Identifiable, Sendable {
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
