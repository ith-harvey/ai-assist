import Foundation
import Testing
@testable import AIAssistClientLib

@Suite("Model JSON Tests")
struct ModelTests {

    // MARK: - Helpers

    /// Standard reply card JSON with payload format.
    private func replyCardJSON(
        id: String = "550e8400-e29b-41d4-a716-446655440000",
        sender: String = "Alice",
        message: String = "Hey, are you coming to the meeting?",
        reply: String = "Yes, I will be there!",
        confidence: Double = 0.92,
        channel: String = "telegram",
        status: String = "pending",
        conversationId: String = "chat_123",
        thread: String? = nil,
        emailThread: String? = nil
    ) -> String {
        var payloadFields = """
            "channel": "\(channel)",
            "source_sender": "\(sender)",
            "source_message": "\(message)",
            "suggested_reply": "\(reply)",
            "confidence": \(confidence),
            "conversation_id": "\(conversationId)"
        """
        if let thread {
            payloadFields += ",\n            \"thread\": \(thread)"
        }
        if let emailThread {
            payloadFields += ",\n            \"email_thread\": \(emailThread)"
        }
        return """
        {
            "id": "\(id)",
            "card_type": "reply",
            "silo": "messages",
            "payload": {
                \(payloadFields)
            },
            "status": "\(status)",
            "created_at": "2026-02-15T10:00:00Z",
            "expires_at": "2026-02-15T10:15:00Z",
            "updated_at": "2026-02-15T10:00:00Z"
        }
        """
    }

    // MARK: - ApprovalCard decoding

    @Test("Decode ApprovalCard from snake_case JSON")
    func decodeApprovalCard() throws {
        let json = replyCardJSON()
        let data = json.data(using: .utf8)!
        let card = try ApprovalCard.decode(from: data)

        #expect(card.id == UUID(uuidString: "550e8400-e29b-41d4-a716-446655440000")!)
        #expect(card.conversationId == "chat_123")
        #expect(card.sourceMessage == "Hey, are you coming to the meeting?")
        #expect(card.sourceSender == "Alice")
        #expect(card.suggestedReply == "Yes, I will be there!")
        #expect((card.confidence - 0.92).magnitude < 0.01)
        #expect(card.status == .pending)
        #expect(card.channel == "telegram")
    }

    @Test("Decode ApprovalCard array")
    func decodeApprovalCardArray() throws {
        let card1 = replyCardJSON(
            id: "550e8400-e29b-41d4-a716-446655440000",
            sender: "Alice", message: "msg1", reply: "reply1",
            confidence: 0.9, channel: "telegram", conversationId: "chat_1"
        )
        let card2 = replyCardJSON(
            id: "660e8400-e29b-41d4-a716-446655440000",
            sender: "Bob", message: "msg2", reply: "reply2",
            confidence: 0.75, channel: "whatsapp", status: "approved", conversationId: "chat_2"
        )
        let json = "[\(card1), \(card2)]"
        let data = json.data(using: .utf8)!
        let cards = try ApprovalCard.decodeArray(from: data)
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
        let cardJson = replyCardJSON(sender: "Bob", message: "hello", reply: "hi!", confidence: 0.9)
        let json = """
        {
            "type": "new_card",
            "card": \(cardJson)
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
        let cardJson = replyCardJSON(sender: "Alice", message: "msg", reply: "reply", confidence: 0.8, conversationId: "chat_1")
        let json = """
        {
            "type": "cards_sync",
            "cards": [\(cardJson)]
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

    // MARK: - ThreadMessage decoding

    @Test("Decode ThreadMessage from snake_case JSON")
    func decodeThreadMessage() throws {
        let json = """
        {
            "sender": "alice@example.com",
            "content": "Hey, following up on our discussion",
            "timestamp": "2026-02-15T10:00:00Z",
            "is_outgoing": false
        }
        """
        let data = json.data(using: .utf8)!
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let msg = try decoder.decode(ThreadMessage.self, from: data)
        #expect(msg.sender == "alice@example.com")
        #expect(msg.content == "Hey, following up on our discussion")
        #expect(msg.timestamp == "2026-02-15T10:00:00Z")
        #expect(msg.isOutgoing == false)
    }

    // MARK: - ApprovalCard with thread

    @Test("Decode ApprovalCard with thread array")
    func decodeApprovalCardWithThread() throws {
        let threadJSON = """
        [
            {"sender": "alice@example.com", "content": "Original question", "timestamp": "2026-02-15T08:00:00Z", "is_outgoing": false},
            {"sender": "me@example.com", "content": "My reply", "timestamp": "2026-02-15T09:00:00Z", "is_outgoing": true}
        ]
        """
        let json = replyCardJSON(channel: "email", thread: threadJSON)
        let data = json.data(using: .utf8)!
        let card = try ApprovalCard.decode(from: data)
        #expect(card.thread.count == 2)
        #expect(card.thread[0].sender == "alice@example.com")
        #expect(card.thread[0].isOutgoing == false)
        #expect(card.thread[1].sender == "me@example.com")
        #expect(card.thread[1].isOutgoing == true)
    }

    @Test("Decode ApprovalCard without thread field defaults to empty array")
    func decodeApprovalCardWithoutThread() throws {
        let json = replyCardJSON(sender: "Bob", message: "Hey there", reply: "Hi!", confidence: 0.8)
        let data = json.data(using: .utf8)!
        let card = try ApprovalCard.decode(from: data)
        #expect(card.thread.isEmpty)
        #expect(card.emailThread.isEmpty)
        #expect(card.sourceSender == "Bob")
    }

    // MARK: - EmailMessage decoding

    @Test("Decode EmailMessage from snake_case JSON")
    func decodeEmailMessage() throws {
        let json = """
        {
            "from": "alice@example.com",
            "to": ["bob@example.com"],
            "cc": ["carol@example.com"],
            "subject": "Re: Meeting",
            "message_id": "<abc@example.com>",
            "content": "Sounds good!",
            "timestamp": "2026-02-15T10:00:00Z",
            "is_outgoing": false
        }
        """
        let data = json.data(using: .utf8)!
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let msg = try decoder.decode(EmailMessage.self, from: data)
        #expect(msg.from == "alice@example.com")
        #expect(msg.to == ["bob@example.com"])
        #expect(msg.cc == ["carol@example.com"])
        #expect(msg.subject == "Re: Meeting")
        #expect(msg.messageId == "<abc@example.com>")
        #expect(msg.isOutgoing == false)
    }

    @Test("Decode EmailMessage without CC defaults to empty")
    func decodeEmailMessageWithoutCC() throws {
        let json = """
        {
            "from": "alice@example.com",
            "to": ["bob@example.com"],
            "subject": "Test",
            "message_id": "<id@example.com>",
            "content": "Hello",
            "timestamp": "2026-02-15T10:00:00Z",
            "is_outgoing": true
        }
        """
        let data = json.data(using: .utf8)!
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let msg = try decoder.decode(EmailMessage.self, from: data)
        #expect(msg.cc.isEmpty)
        #expect(msg.isOutgoing == true)
    }

    // MARK: - ApprovalCard with emailThread

    @Test("Decode ApprovalCard with emailThread array")
    func decodeApprovalCardWithEmailThread() throws {
        let emailThreadJSON = """
        [
            {"from": "alice@example.com", "to": ["bob@example.com"], "cc": ["carol@example.com"], "subject": "Re: Meeting", "message_id": "<abc@example.com>", "content": "Sounds good!", "timestamp": "2026-02-15T08:00:00Z", "is_outgoing": false},
            {"from": "bob@example.com", "to": ["alice@example.com"], "subject": "Re: Meeting", "message_id": "<def@example.com>", "content": "See you there", "timestamp": "2026-02-15T09:00:00Z", "is_outgoing": true}
        ]
        """
        let json = replyCardJSON(channel: "email", emailThread: emailThreadJSON)
        let data = json.data(using: .utf8)!
        let card = try ApprovalCard.decode(from: data)
        #expect(card.emailThread.count == 2)
        #expect(card.emailThread[0].from == "alice@example.com")
        #expect(card.emailThread[0].cc == ["carol@example.com"])
        #expect(card.emailThread[1].from == "bob@example.com")
        #expect(card.emailThread[1].cc.isEmpty)
        #expect(card.emailThread[1].isOutgoing == true)
    }

    @Test("Decode ApprovalCard without emailThread field defaults to empty")
    func decodeApprovalCardWithoutEmailThread() throws {
        let json = replyCardJSON(sender: "Bob", message: "hi", reply: "hey", confidence: 0.9, channel: "email", conversationId: "chat_1")
        let data = json.data(using: .utf8)!
        let card = try ApprovalCard.decode(from: data)
        #expect(card.emailThread.isEmpty)
    }
}
