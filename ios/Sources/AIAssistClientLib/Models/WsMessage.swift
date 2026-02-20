import Foundation

/// Messages sent over WebSocket from server to client.
/// Matches Rust `WsMessage` with `#[serde(tag = "type", rename_all = "snake_case")]`.
///
/// JSON discriminator is the `"type"` field:
/// - `{"type":"new_card","card":{...}}`
/// - `{"type":"card_update","id":"uuid","status":"approved"}`
/// - `{"type":"card_expired","id":"uuid"}`
/// - `{"type":"cards_sync","cards":[...]}`
/// - `{"type":"ping"}`
public enum WsMessage: Sendable {
    case newCard(card: ReplyCard)
    case cardUpdate(id: UUID, status: CardStatus)
    case cardExpired(id: UUID)
    case cardsSync(cards: [ReplyCard])
    case cardRefreshed(card: ReplyCard)
    case ping
}

extension WsMessage: Decodable {
    private enum CodingKeys: String, CodingKey {
        case type
    }

    /// Keys for individual message variants (snake_case to match Rust serde).
    private enum PayloadKeys: String, CodingKey {
        case card
        case id
        case status
        case cards
    }

    public init(from decoder: Decoder) throws {
        let typeContainer = try decoder.container(keyedBy: CodingKeys.self)
        let type = try typeContainer.decode(String.self, forKey: .type)

        let container = try decoder.container(keyedBy: PayloadKeys.self)

        switch type {
        case "new_card":
            let card = try container.decode(ReplyCard.self, forKey: .card)
            self = .newCard(card: card)
        case "card_update":
            let id = try container.decode(UUID.self, forKey: .id)
            let status = try container.decode(CardStatus.self, forKey: .status)
            self = .cardUpdate(id: id, status: status)
        case "card_expired":
            let id = try container.decode(UUID.self, forKey: .id)
            self = .cardExpired(id: id)
        case "cards_sync":
            let cards = try container.decode([ReplyCard].self, forKey: .cards)
            self = .cardsSync(cards: cards)
        case "card_refreshed":
            let card = try container.decode(ReplyCard.self, forKey: .card)
            self = .cardRefreshed(card: card)
        case "ping":
            self = .ping
        default:
            throw DecodingError.dataCorrupted(
                DecodingError.Context(
                    codingPath: typeContainer.codingPath,
                    debugDescription: "Unknown WsMessage type: \(type)"
                )
            )
        }
    }

    /// Decode a `WsMessage` from snake_case JSON data.
    public static func decode(from data: Data) throws -> WsMessage {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(WsMessage.self, from: data)
    }
}
