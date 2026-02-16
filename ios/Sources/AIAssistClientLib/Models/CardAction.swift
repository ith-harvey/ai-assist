import Foundation

/// Actions a client can take on a card.
/// Matches Rust `CardAction` with `#[serde(tag = "action", rename_all = "snake_case")]`.
///
/// JSON format:
/// - `{"action":"approve","card_id":"uuid"}`
/// - `{"action":"dismiss","card_id":"uuid"}`
/// - `{"action":"edit","card_id":"uuid","new_text":"..."}`
public enum CardAction: Encodable, Sendable {
    case approve(cardId: UUID)
    case dismiss(cardId: UUID)
    case edit(cardId: UUID, newText: String)

    private enum CodingKeys: String, CodingKey {
        case action
        case cardId = "card_id"
        case newText = "new_text"
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .approve(let cardId):
            try container.encode("approve", forKey: .action)
            try container.encode(cardId, forKey: .cardId)
        case .dismiss(let cardId):
            try container.encode("dismiss", forKey: .action)
            try container.encode(cardId, forKey: .cardId)
        case .edit(let cardId, let newText):
            try container.encode("edit", forKey: .action)
            try container.encode(cardId, forKey: .cardId)
            try container.encode(newText, forKey: .newText)
        }
    }

    /// Encode this action to JSON data.
    public func toData() throws -> Data {
        try JSONEncoder().encode(self)
    }
}
