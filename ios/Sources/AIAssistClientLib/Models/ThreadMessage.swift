import Foundation

/// A message in an email thread — provides conversation context for reply cards.
/// Mirrors Rust `ThreadMessage` struct.
public struct ThreadMessage: Codable, Identifiable, Sendable {
    public var id: String { "\(sender)-\(timestamp)" }
    public let sender: String
    public let content: String
    public let timestamp: String
    public let isOutgoing: Bool
}
