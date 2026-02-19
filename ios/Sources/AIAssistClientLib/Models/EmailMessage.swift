import Foundation

/// A message in an email thread with full headers (From/To/CC/Subject/Message-ID).
/// Mirrors Rust `EmailMessage` struct from `channels::email_types`.
public struct EmailMessage: Identifiable, Sendable {
    public var id: String { "\(from)-\(timestamp)" }
    public let from: String
    public let to: [String]
    public let cc: [String]
    public let subject: String
    public let messageId: String
    public let content: String
    public let timestamp: String
    public let isOutgoing: Bool
}

extension EmailMessage: Codable {
    // Explicit CodingKeys to avoid conflict between `from` property
    // and `init(from decoder:)` parameter name.
    enum CodingKeys: String, CodingKey {
        case from, to, cc, subject, messageId, content, timestamp, isOutgoing
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.from = try container.decode(String.self, forKey: .from)
        self.to = try container.decodeIfPresent([String].self, forKey: .to) ?? []
        self.cc = try container.decodeIfPresent([String].self, forKey: .cc) ?? []
        self.subject = try container.decode(String.self, forKey: .subject)
        self.messageId = try container.decode(String.self, forKey: .messageId)
        self.content = try container.decode(String.self, forKey: .content)
        self.timestamp = try container.decode(String.self, forKey: .timestamp)
        self.isOutgoing = try container.decode(Bool.self, forKey: .isOutgoing)
    }
}
