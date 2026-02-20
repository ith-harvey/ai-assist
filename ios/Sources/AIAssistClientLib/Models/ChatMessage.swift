import Foundation

/// A single message in the Brain chat conversation.
public struct ChatMessage: Identifiable, Sendable {
    public let id: UUID
    public var content: String
    public let isFromUser: Bool
    public let timestamp: Date

    public init(id: UUID = UUID(), content: String, isFromUser: Bool, timestamp: Date = Date()) {
        self.id = id
        self.content = content
        self.isFromUser = isFromUser
        self.timestamp = timestamp
    }
}
