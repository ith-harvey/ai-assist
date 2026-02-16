import Foundation
import Testing
@testable import AIAssistClientLib

@Suite("Model JSON Tests")
struct ModelTests {

    // MARK: - ReplyCard decoding

    @Test("Decode ReplyCard from snake_case JSON")
    func decodeReplyCard() throws {
        let json = """
        {
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "conversation_id": "chat_123",
            "source_message": "Hey, are you coming to the meeting?",
            "source_sender": "Alice",
            "suggested_reply": "Yes, I will be there!",
            "confidence": 0.92,
            "status": "pending",
            "created_at": "2026-02-15T10:00:00Z",
            "expires_at": "2026-02-15T10:15:00Z",
            "channel": "telegram",
            "updated_at": "2026-02-15T10:00:00Z"
        }
        """
        let data = json.data(using: .utf8)!
        let card = try ReplyCard.decode(from: data)

        #expect(card.id == UUID(uuidString: "550e8400-e29b-41d4-a716-446655440000")!)
        #expect(card.conversationId == "chat_123")
        #expect(card.sourceMessage == "Hey, are you coming to the meeting?")
        #expect(card.sourceSender == "Alice")
        #expect(card.suggestedReply == "Yes, I will be there!")
        #expect(card.confidence == 0.92)
        #expect(card.status == .pending)
        #expect(card.channel == "telegram")
    }

    @Test("Decode ReplyCard array")
    func decodeReplyCardArray() throws {
        let json = """
        [
            {
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "conversation_id": "chat_1",
                "source_message": "msg1",
                "source_sender": "Alice",
                "suggested_reply": "reply1",
                "confidence": 0.9,
                "status": "pending",
                "created_at": "2026-02-15T10:00:00Z",
                "expires_at": "2026-02-15T10:15:00Z",
                "channel": "telegram",
                "updated_at": "2026-02-15T10:00:00Z"
            },
            {
                "id": "660e8400-e29b-41d4-a716-446655440000",
                "conversation_id": "chat_2",
                "source_message": "msg2",
                "source_sender": "Bob",
                "suggested_reply": "reply2",
                "confidence": 0.75,
                "status": "approved",
                "created_at": "2026-02-15T11:00:00Z",
                "expires_at": "2026-02-15T11:15:00Z",
                "channel": "whatsapp",
                "updated_at": "2026-02-15T11:00:00Z"
            }
        ]
        """
        let data = json.data(using: .utf8)!
        let cards = try ReplyCard.decodeArray(from: data)
        #expect(cards.count == 2)
        #expect(cards[0].sourceSender == "Alice")
        #expect(cards[1].sourceSender == "Bob")
        #expect(cards[1].status == .approved)
    }

    // MARK: - CardStatus

    @Test("Decode all CardStatus variants")
    func decodeCardStatus() throws {
        let cases: [(String, CardStatus)] = [
            ("\"pending\"", .pending),
            ("\"approved\"", .approved),
            ("\"dismissed\"", .dismissed),
            ("\"expired\"", .expired),
            ("\"sent\"", .sent),
        ]
        for (json, expected) in cases {
            let data = json.data(using: .utf8)!
            let status = try JSONDecoder().decode(CardStatus.self, from: data)
            #expect(status == expected)
        }
    }

    // MARK: - CardAction encoding

    @Test("Encode approve action to correct JSON")
    func encodeApproveAction() throws {
        let cardId = UUID(uuidString: "550e8400-e29b-41d4-a716-446655440000")!
        let action = CardAction.approve(cardId: cardId)
        let data = try action.toData()
        let dict = try JSONSerialization.jsonObject(with: data) as! [String: Any]

        #expect(dict["action"] as? String == "approve")
        #expect(dict["card_id"] as? String == "550E8400-E29B-41D4-A716-446655440000")
        #expect(dict["new_text"] == nil)
    }

    @Test("Encode dismiss action to correct JSON")
    func encodeDismissAction() throws {
        let cardId = UUID(uuidString: "550e8400-e29b-41d4-a716-446655440000")!
        let action = CardAction.dismiss(cardId: cardId)
        let data = try action.toData()
        let dict = try JSONSerialization.jsonObject(with: data) as! [String: Any]

        #expect(dict["action"] as? String == "dismiss")
        #expect(dict["card_id"] as? String == "550E8400-E29B-41D4-A716-446655440000")
    }

    @Test("Encode edit action with new_text")
    func encodeEditAction() throws {
        let cardId = UUID(uuidString: "550e8400-e29b-41d4-a716-446655440000")!
        let action = CardAction.edit(cardId: cardId, newText: "Custom reply text")
        let data = try action.toData()
        let dict = try JSONSerialization.jsonObject(with: data) as! [String: Any]

        #expect(dict["action"] as? String == "edit")
        #expect(dict["card_id"] as? String == "550E8400-E29B-41D4-A716-446655440000")
        #expect(dict["new_text"] as? String == "Custom reply text")
    }

    // MARK: - WsMessage decoding

    @Test("Decode new_card WsMessage")
    func decodeNewCard() throws {
        let json = """
        {
            "type": "new_card",
            "card": {
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "conversation_id": "chat_123",
                "source_message": "hello",
                "source_sender": "Bob",
                "suggested_reply": "hi!",
                "confidence": 0.9,
                "status": "pending",
                "created_at": "2026-02-15T10:00:00Z",
                "expires_at": "2026-02-15T10:15:00Z",
                "channel": "telegram",
                "updated_at": "2026-02-15T10:00:00Z"
            }
        }
        """
        let data = json.data(using: .utf8)!
        let msg = try WsMessage.decode(from: data)

        guard case .newCard(let card) = msg else {
            Issue.record("Expected newCard, got \(msg)")
            return
        }
        #expect(card.sourceSender == "Bob")
        #expect(card.suggestedReply == "hi!")
    }

    @Test("Decode card_update WsMessage")
    func decodeCardUpdate() throws {
        let json = """
        {
            "type": "card_update",
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "status": "approved"
        }
        """
        let data = json.data(using: .utf8)!
        let msg = try WsMessage.decode(from: data)

        guard case .cardUpdate(let id, let status) = msg else {
            Issue.record("Expected cardUpdate, got \(msg)")
            return
        }
        #expect(id == UUID(uuidString: "550e8400-e29b-41d4-a716-446655440000")!)
        #expect(status == .approved)
    }

    @Test("Decode card_expired WsMessage")
    func decodeCardExpired() throws {
        let json = """
        {
            "type": "card_expired",
            "id": "550e8400-e29b-41d4-a716-446655440000"
        }
        """
        let data = json.data(using: .utf8)!
        let msg = try WsMessage.decode(from: data)

        guard case .cardExpired(let id) = msg else {
            Issue.record("Expected cardExpired, got \(msg)")
            return
        }
        #expect(id == UUID(uuidString: "550e8400-e29b-41d4-a716-446655440000")!)
    }

    @Test("Decode cards_sync WsMessage")
    func decodeCardsSync() throws {
        let json = """
        {
            "type": "cards_sync",
            "cards": [
                {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "conversation_id": "chat_1",
                    "source_message": "msg",
                    "source_sender": "Alice",
                    "suggested_reply": "reply",
                    "confidence": 0.8,
                    "status": "pending",
                    "created_at": "2026-02-15T10:00:00Z",
                    "expires_at": "2026-02-15T10:15:00Z",
                    "channel": "telegram",
                    "updated_at": "2026-02-15T10:00:00Z"
                }
            ]
        }
        """
        let data = json.data(using: .utf8)!
        let msg = try WsMessage.decode(from: data)

        guard case .cardsSync(let cards) = msg else {
            Issue.record("Expected cardsSync, got \(msg)")
            return
        }
        #expect(cards.count == 1)
        #expect(cards[0].sourceSender == "Alice")
    }

    @Test("Decode ping WsMessage")
    func decodePing() throws {
        let json = """
        {"type": "ping"}
        """
        let data = json.data(using: .utf8)!
        let msg = try WsMessage.decode(from: data)

        guard case .ping = msg else {
            Issue.record("Expected ping, got \(msg)")
            return
        }
    }

    @Test("Unknown WsMessage type throws")
    func decodeUnknownType() throws {
        let json = """
        {"type": "unknown_type"}
        """
        let data = json.data(using: .utf8)!
        #expect(throws: DecodingError.self) {
            _ = try WsMessage.decode(from: data)
        }
    }
}
