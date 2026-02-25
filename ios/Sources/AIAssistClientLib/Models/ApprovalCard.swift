import Foundation

/// Status of an approval card in the queue.
/// Matches Rust `CardStatus` with `#[serde(rename_all = "snake_case")]`.
public enum CardStatus: String, Codable, Sendable {
    case pending
    case approved
    case dismissed
    case expired
    case sent
}

/// Which tab/silo this card belongs to in the UI.
public enum CardSilo: String, Codable, Sendable {
    case messages
    case todos
    case calendar
}

/// The kind of card.
public enum CardType: String, Codable, Sendable {
    case reply
    case compose
    case action
    case decision
}

/// Type-specific payload for each card variant.
public enum CardPayload: Sendable {
    case reply(
        channel: String,
        sourceSender: String,
        sourceMessage: String,
        suggestedReply: String,
        confidence: Float,
        conversationId: String,
        thread: [ThreadMessage],
        emailThread: [EmailMessage],
        replyMetadata: AnyCodableSendable?,
        messageId: String?
    )
    case compose(
        channel: String,
        recipient: String,
        subject: String?,
        draftBody: String,
        confidence: Float
    )
    case action(
        description: String,
        actionDetail: String?
    )
    case decision(
        question: String,
        context: String,
        options: [String]
    )
}

/// Minimal Sendable wrapper for untyped JSON values (reply_metadata).
public struct AnyCodableSendable: @unchecked Sendable {
    public let value: Any
    public init(_ value: Any) { self.value = value }
}

/// A universal approval card — reply suggestions, actions, decisions, etc.
/// Mirrors Rust `ApprovalCard` struct.
///
/// Backend JSON shape (flattened CardPayload):
/// ```json
/// {
///   "id": "uuid",
///   "silo": "messages",
///   "card_type": "reply",
///   "payload": { ... },
///   "status": "pending",
///   "created_at": "...",
///   "expires_at": "...",
///   "updated_at": "..."
/// }
/// ```
public struct ApprovalCard: Identifiable, Sendable {
    public let id: UUID
    public let silo: CardSilo
    public let cardType: CardType
    public let payload: CardPayload
    public var status: CardStatus
    public let createdAt: String
    public let expiresAt: String
    public let updatedAt: String
}

// MARK: - Convenience Computed Properties (minimize downstream changes)

extension ApprovalCard {
    /// Channel extracted from payload (Reply/Compose), or empty string.
    public var channel: String {
        switch payload {
        case .reply(let channel, _, _, _, _, _, _, _, _, _): return channel
        case .compose(let channel, _, _, _, _): return channel
        default: return ""
        }
    }

    /// Source sender (Reply only).
    public var sourceSender: String {
        if case .reply(_, let sender, _, _, _, _, _, _, _, _) = payload { return sender }
        return ""
    }

    /// Source message (Reply only).
    public var sourceMessage: String {
        if case .reply(_, _, let msg, _, _, _, _, _, _, _) = payload { return msg }
        return ""
    }

    /// Suggested reply text (Reply only).
    public var suggestedReply: String {
        if case .reply(_, _, _, let reply, _, _, _, _, _, _) = payload { return reply }
        if case .compose(_, _, _, let body, _) = payload { return body }
        return ""
    }

    /// Confidence score (Reply/Compose), or 0.
    public var confidence: Float {
        switch payload {
        case .reply(_, _, _, _, let c, _, _, _, _, _): return c
        case .compose(_, _, _, _, let c): return c
        default: return 0
        }
    }

    /// Conversation ID (Reply only).
    public var conversationId: String {
        if case .reply(_, _, _, _, _, let cid, _, _, _, _) = payload { return cid }
        return ""
    }

    /// Thread messages (Reply only).
    public var thread: [ThreadMessage] {
        if case .reply(_, _, _, _, _, _, let t, _, _, _) = payload { return t }
        return []
    }

    /// Email thread with full headers (Reply only).
    public var emailThread: [EmailMessage] {
        if case .reply(_, _, _, _, _, _, _, let et, _, _) = payload { return et }
        return []
    }
}

// MARK: - Codable

extension ApprovalCard: Codable {
    enum CodingKeys: String, CodingKey {
        case id, silo, cardType, payload, status, createdAt, expiresAt, updatedAt
    }

    // Keys inside the "payload" container, used per card_type
    enum ReplyPayloadKeys: String, CodingKey {
        case channel, sourceSender, sourceMessage, suggestedReply, confidence
        case conversationId, thread, emailThread, replyMetadata, messageId
    }
    enum ComposePayloadKeys: String, CodingKey {
        case channel, recipient, subject, draftBody, confidence
    }
    enum ActionPayloadKeys: String, CodingKey {
        case description, actionDetail
    }
    enum DecisionPayloadKeys: String, CodingKey {
        case question, context, options
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)

        id = try container.decode(UUID.self, forKey: .id)
        silo = try container.decodeIfPresent(CardSilo.self, forKey: .silo) ?? .messages
        status = try container.decode(CardStatus.self, forKey: .status)
        createdAt = try container.decode(String.self, forKey: .createdAt)
        expiresAt = try container.decode(String.self, forKey: .expiresAt)
        updatedAt = try container.decode(String.self, forKey: .updatedAt)

        let type = try container.decodeIfPresent(CardType.self, forKey: .cardType) ?? .reply
        cardType = type

        switch type {
        case .reply:
            let p = try container.nestedContainer(keyedBy: ReplyPayloadKeys.self, forKey: .payload)
            payload = .reply(
                channel: try p.decode(String.self, forKey: .channel),
                sourceSender: try p.decode(String.self, forKey: .sourceSender),
                sourceMessage: try p.decode(String.self, forKey: .sourceMessage),
                suggestedReply: try p.decode(String.self, forKey: .suggestedReply),
                confidence: try p.decode(Float.self, forKey: .confidence),
                conversationId: try p.decode(String.self, forKey: .conversationId),
                thread: try p.decodeIfPresent([ThreadMessage].self, forKey: .thread) ?? [],
                emailThread: try p.decodeIfPresent([EmailMessage].self, forKey: .emailThread) ?? [],
                replyMetadata: nil, // We don't need to parse this on iOS
                messageId: try p.decodeIfPresent(String.self, forKey: .messageId)
            )
        case .compose:
            let p = try container.nestedContainer(keyedBy: ComposePayloadKeys.self, forKey: .payload)
            payload = .compose(
                channel: try p.decode(String.self, forKey: .channel),
                recipient: try p.decode(String.self, forKey: .recipient),
                subject: try p.decodeIfPresent(String.self, forKey: .subject),
                draftBody: try p.decode(String.self, forKey: .draftBody),
                confidence: try p.decode(Float.self, forKey: .confidence)
            )
        case .action:
            let p = try container.nestedContainer(keyedBy: ActionPayloadKeys.self, forKey: .payload)
            payload = .action(
                description: try p.decode(String.self, forKey: .description),
                actionDetail: try p.decodeIfPresent(String.self, forKey: .actionDetail)
            )
        case .decision:
            let p = try container.nestedContainer(keyedBy: DecisionPayloadKeys.self, forKey: .payload)
            payload = .decision(
                question: try p.decode(String.self, forKey: .question),
                context: try p.decode(String.self, forKey: .context),
                options: try p.decodeIfPresent([String].self, forKey: .options) ?? []
            )
        }
    }

    public func encode(to encoder: Encoder) throws {
        // Encoding not needed for iOS client, but satisfy Codable conformance
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(silo, forKey: .silo)
        try container.encode(cardType, forKey: .cardType)
        try container.encode(status, forKey: .status)
        try container.encode(createdAt, forKey: .createdAt)
        try container.encode(expiresAt, forKey: .expiresAt)
        try container.encode(updatedAt, forKey: .updatedAt)
        // payload encoding omitted — not needed
    }
}

extension ApprovalCard {
    /// Decode an `ApprovalCard` from snake_case JSON data.
    public static func decode(from data: Data) throws -> ApprovalCard {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(ApprovalCard.self, from: data)
    }

    /// Decode an array of `ApprovalCard` from snake_case JSON data.
    public static func decodeArray(from data: Data) throws -> [ApprovalCard] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode([ApprovalCard].self, from: data)
    }
}
