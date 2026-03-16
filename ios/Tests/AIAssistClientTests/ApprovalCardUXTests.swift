import Foundation
import SwiftUI
import Testing
@testable import AIAssistClientLib

// MARK: - Test Helpers

/// Builds test cards for various payload types.
private func makeCard(
    type: CardType,
    silo: CardSilo = .messages,
    id: UUID = UUID()
) -> ApprovalCard {
    let json: String
    switch type {
    case .reply:
        json = """
        {
            "id": "\(id.uuidString)",
            "card_type": "reply",
            "silo": "\(silo.rawValue)",
            "payload": {
                "channel": "telegram",
                "source_sender": "Alice",
                "source_message": "Hello",
                "suggested_reply": "Hi there!",
                "confidence": 0.9,
                "conversation_id": "chat_1"
            },
            "status": "pending",
            "created_at": "2026-03-15T10:00:00Z",
            "expires_at": "2026-03-15T10:15:00Z",
            "updated_at": "2026-03-15T10:00:00Z"
        }
        """
    case .compose:
        json = """
        {
            "id": "\(id.uuidString)",
            "card_type": "compose",
            "silo": "\(silo.rawValue)",
            "payload": {
                "channel": "email",
                "recipient": "bob@example.com",
                "subject": "Meeting",
                "draft_body": "Let's meet at 3pm",
                "confidence": 0.85
            },
            "status": "pending",
            "created_at": "2026-03-15T10:00:00Z",
            "expires_at": "2026-03-15T10:15:00Z",
            "updated_at": "2026-03-15T10:00:00Z"
        }
        """
    case .action:
        json = """
        {
            "id": "\(id.uuidString)",
            "card_type": "action",
            "silo": "\(silo.rawValue)",
            "payload": {
                "description": "Create a calendar event",
                "action_detail": "{ \\"event\\": \\"standup\\" }"
            },
            "status": "pending",
            "created_at": "2026-03-15T10:00:00Z",
            "expires_at": "2026-03-15T10:15:00Z",
            "updated_at": "2026-03-15T10:00:00Z"
        }
        """
    case .decision:
        json = """
        {
            "id": "\(id.uuidString)",
            "card_type": "decision",
            "silo": "\(silo.rawValue)",
            "payload": {
                "question": "Should we deploy?",
                "context": "All tests passing",
                "options": ["Yes", "No", "Wait"]
            },
            "status": "pending",
            "created_at": "2026-03-15T10:00:00Z",
            "expires_at": "2026-03-15T10:15:00Z",
            "updated_at": "2026-03-15T10:00:00Z"
        }
        """
    case .multipleChoice:
        json = """
        {
            "id": "\(id.uuidString)",
            "card_type": "multipleChoice",
            "silo": "\(silo.rawValue)",
            "payload": {
                "question": "Pick a response",
                "options": ["Sounds good", "Let me check", "Not now"]
            },
            "status": "pending",
            "created_at": "2026-03-15T10:00:00Z",
            "expires_at": "2026-03-15T10:15:00Z",
            "updated_at": "2026-03-15T10:00:00Z"
        }
        """
    }
    return try! ApprovalCard.decode(from: json.data(using: .utf8)!)
}

// MARK: - ApprovalCardContent Protocol Tests

@Suite("ApprovalCardContent Child Types")
struct ApprovalCardContentTests {

    // MARK: - MessageDraftApprovalCard

    @Test("MessageDraftApprovalCard supports refine for reply cards")
    func messageDraftReplySupportsRefine() {
        let card = makeCard(type: .reply)
        let ws = CardWebSocket()
        var text = ""
        let view = MessageDraftApprovalCard(
            card: card,
            cardSocket: ws,
            refineText: .init(get: { text }, set: { text = $0 })
        )
        #expect(view.supportsRefine == true)
        #expect(view.approveDisabled == false)
    }

    @Test("MessageDraftApprovalCard supports refine for compose cards")
    func messageDraftComposeSupportsRefine() {
        let card = makeCard(type: .compose)
        let ws = CardWebSocket()
        var text = ""
        let view = MessageDraftApprovalCard(
            card: card,
            cardSocket: ws,
            refineText: .init(get: { text }, set: { text = $0 })
        )
        #expect(view.supportsRefine == true)
        #expect(view.approveDisabled == false)
    }

    // MARK: - ActionApprovalCard

    @Test("ActionApprovalCard does not support refine for action cards")
    func actionCardNoRefine() {
        let card = makeCard(type: .action)
        let view = ActionApprovalCard(card: card)
        #expect(view.supportsRefine == false)
        #expect(view.approveDisabled == false)
    }

    @Test("ActionApprovalCard does not support refine for decision cards")
    func decisionCardNoRefine() {
        let card = makeCard(type: .decision)
        let view = ActionApprovalCard(card: card)
        #expect(view.supportsRefine == false)
        #expect(view.approveDisabled == false)
    }

    // MARK: - MultipleChoiceApprovalCard

    @Test("MultipleChoiceApprovalCard disables approve and does not support refine")
    func multipleChoiceDisablesApprove() {
        let card = makeCard(type: .multipleChoice)
        let ws = CardWebSocket()
        let view = MultipleChoiceApprovalCard(card: card, socket: ws)
        #expect(view.supportsRefine == false)
        #expect(view.approveDisabled == true)
    }
}

// MARK: - ApprovalSheetMode Tests

@Suite("ApprovalSheetMode")
struct ApprovalSheetModeTests {

    @Test("Queue mode has stable id")
    func queueModeId() {
        let mode = ApprovalSheetMode.queue
        #expect(mode.id == "queue")
    }

    @Test("Single mode id matches card UUID")
    func singleModeId() {
        let card = makeCard(type: .reply)
        let mode = ApprovalSheetMode.single(card)
        #expect(mode.id == card.id.uuidString)
    }
}

// MARK: - Card Source & Silo Filtering Tests

@Suite("Card Source and Silo Filtering")
struct CardSourceTests {

    @Test("CardWebSocket.cards(for:) filters by silo")
    func cardsForSiloFilters() {
        let ws = CardWebSocket()
        let messageCard = makeCard(type: .reply, silo: .messages)
        let todoCard = makeCard(type: .action, silo: .todos)
        ws.cards = [messageCard, todoCard]

        let messageCards = ws.cards(for: .messages)
        #expect(messageCards.count == 1)
        #expect(messageCards[0].id == messageCard.id)

        let todoCards = ws.cards(for: .todos)
        #expect(todoCards.count == 1)
        #expect(todoCards[0].id == todoCard.id)
    }

    @Test("Empty cards returns empty for any silo")
    func emptyCardsReturnsEmpty() {
        let ws = CardWebSocket()
        #expect(ws.cards(for: .messages).isEmpty)
        #expect(ws.cards(for: .todos).isEmpty)
    }

    @Test("Multiple message cards returned in order")
    func multipleMessageCardsOrdered() {
        let ws = CardWebSocket()
        let card1 = makeCard(type: .reply, silo: .messages)
        let card2 = makeCard(type: .compose, silo: .messages)
        let card3 = makeCard(type: .action, silo: .todos)
        ws.cards = [card1, card2, card3]

        let messageCards = ws.cards(for: .messages)
        #expect(messageCards.count == 2)
        #expect(messageCards[0].id == card1.id)
        #expect(messageCards[1].id == card2.id)
    }
}

// MARK: - Card Payload Type Tests

@Suite("Card Payload Routing")
struct CardPayloadRoutingTests {

    @Test("Reply card has .reply payload")
    func replyPayload() {
        let card = makeCard(type: .reply)
        if case .reply = card.payload { /* expected */ } else {
            Issue.record("Expected .reply payload")
        }
    }

    @Test("Compose card has .compose payload")
    func composePayload() {
        let card = makeCard(type: .compose)
        if case .compose = card.payload { /* expected */ } else {
            Issue.record("Expected .compose payload")
        }
    }

    @Test("Action card has .action payload")
    func actionPayload() {
        let card = makeCard(type: .action)
        if case .action = card.payload { /* expected */ } else {
            Issue.record("Expected .action payload")
        }
    }

    @Test("Decision card has .decision payload")
    func decisionPayload() {
        let card = makeCard(type: .decision)
        if case .decision = card.payload { /* expected */ } else {
            Issue.record("Expected .decision payload")
        }
    }

    @Test("MultipleChoice card has .multipleChoice payload")
    func multipleChoicePayload() {
        let card = makeCard(type: .multipleChoice)
        if case .multipleChoice = card.payload { /* expected */ } else {
            Issue.record("Expected .multipleChoice payload")
        }
    }

    @Test("Reply and compose are message-draft types (supportsRefine)")
    func messageDraftTypes() {
        let replyCard = makeCard(type: .reply)
        let composeCard = makeCard(type: .compose)
        let ws = CardWebSocket()
        var text = ""
        let binding = SwiftUI.Binding(get: { text }, set: { text = $0 })

        let replyView = MessageDraftApprovalCard(card: replyCard, cardSocket: ws, refineText: binding)
        let composeView = MessageDraftApprovalCard(card: composeCard, cardSocket: ws, refineText: binding)

        #expect(replyView.supportsRefine == true)
        #expect(composeView.supportsRefine == true)
    }

    @Test("Action and decision are action types (no refine)")
    func actionTypes() {
        let actionView = ActionApprovalCard(card: makeCard(type: .action))
        let decisionView = ActionApprovalCard(card: makeCard(type: .decision))

        #expect(actionView.supportsRefine == false)
        #expect(decisionView.supportsRefine == false)
    }

    @Test("MultipleChoice disables approve gesture")
    func multipleChoiceDisablesApprove() {
        let ws = CardWebSocket()
        let view = MultipleChoiceApprovalCard(card: makeCard(type: .multipleChoice), socket: ws)
        #expect(view.approveDisabled == true)
    }
}

// MARK: - CardWebSocket Action Tests

@Suite("CardWebSocket Approve/Dismiss/Refine Actions")
struct CardWebSocketActionTests {

    @Test("Approve removes card from cards array")
    func approveRemovesCard() {
        let ws = CardWebSocket()
        let card = makeCard(type: .reply)
        ws.cards = [card]
        ws.approve(cardId: card.id)
        #expect(ws.cards.isEmpty)
    }

    @Test("Dismiss removes card from cards array")
    func dismissRemovesCard() {
        let ws = CardWebSocket()
        let card = makeCard(type: .action)
        ws.cards = [card]
        ws.dismiss(cardId: card.id)
        #expect(ws.cards.isEmpty)
    }

    @Test("Approve only removes matching card")
    func approveOnlyRemovesMatchingCard() {
        let ws = CardWebSocket()
        let card1 = makeCard(type: .reply)
        let card2 = makeCard(type: .compose)
        ws.cards = [card1, card2]
        ws.approve(cardId: card1.id)
        #expect(ws.cards.count == 1)
        #expect(ws.cards[0].id == card2.id)
    }

    @Test("Refine sets isRefining to true")
    func refineSetsFlag() {
        let ws = CardWebSocket()
        let card = makeCard(type: .reply)
        ws.cards = [card]
        ws.refine(cardId: card.id, instruction: "Make it shorter")
        #expect(ws.isRefining == true)
    }

    @Test("SelectOption removes card from cards array")
    func selectOptionRemovesCard() {
        let ws = CardWebSocket()
        let card = makeCard(type: .multipleChoice)
        ws.cards = [card]
        ws.selectOption(cardId: card.id, selectedIndex: 0)
        #expect(ws.cards.isEmpty)
    }
}

// MARK: - Queue Progression Tests

@Suite("Queue Card Progression")
struct QueueProgressionTests {

    @Test("Approving first card exposes second card as first")
    func approveAdvancesQueue() {
        let ws = CardWebSocket()
        let card1 = makeCard(type: .reply)
        let card2 = makeCard(type: .compose)
        let card3 = makeCard(type: .action, silo: .todos)
        ws.cards = [card1, card2, card3]

        // Approve first card
        ws.approve(cardId: card1.id)
        #expect(ws.cards.first?.id == card2.id)

        // Messages silo now has only card2
        let messageCards = ws.cards(for: .messages)
        #expect(messageCards.count == 1)
        #expect(messageCards[0].id == card2.id)
    }

    @Test("Dismissing card advances queue same as approve")
    func dismissAdvancesQueue() {
        let ws = CardWebSocket()
        let card1 = makeCard(type: .reply)
        let card2 = makeCard(type: .reply)
        ws.cards = [card1, card2]

        ws.dismiss(cardId: card1.id)
        #expect(ws.cards.count == 1)
        #expect(ws.cards.first?.id == card2.id)
    }

    @Test("All cards processed leaves empty array")
    func allCardsProcessedEmpty() {
        let ws = CardWebSocket()
        let card1 = makeCard(type: .reply)
        let card2 = makeCard(type: .action)
        ws.cards = [card1, card2]

        ws.approve(cardId: card1.id)
        ws.dismiss(cardId: card2.id)
        #expect(ws.cards.isEmpty)
    }

    @Test("Mixed silo queue - messages filtered correctly during progression")
    func mixedSiloProgression() {
        let ws = CardWebSocket()
        let msg1 = makeCard(type: .reply, silo: .messages)
        let todo1 = makeCard(type: .action, silo: .todos)
        let msg2 = makeCard(type: .compose, silo: .messages)
        ws.cards = [msg1, todo1, msg2]

        // Messages view shows 2 cards
        #expect(ws.cards(for: .messages).count == 2)

        // Approve the first message card
        ws.approve(cardId: msg1.id)

        // Messages view shows 1 card, todo unaffected
        #expect(ws.cards(for: .messages).count == 1)
        #expect(ws.cards(for: .todos).count == 1)
        #expect(ws.cards(for: .messages).first?.id == msg2.id)
    }
}

// MARK: - Protocol Default Values Tests

@Suite("ApprovalCardContent Protocol Defaults")
struct ProtocolDefaultTests {

    @Test("Default supportsRefine is false")
    func defaultSupportsRefine() {
        let card = makeCard(type: .action)
        let view = ActionApprovalCard(card: card)
        #expect(view.supportsRefine == false)
    }

    @Test("Default approveDisabled is false")
    func defaultApproveDisabled() {
        let card = makeCard(type: .action)
        let view = ActionApprovalCard(card: card)
        #expect(view.approveDisabled == false)
    }

    @Test("MessageDraftApprovalCard overrides supportsRefine to true")
    func messageDraftOverridesRefine() {
        let ws = CardWebSocket()
        var text = ""
        let view = MessageDraftApprovalCard(
            card: makeCard(type: .reply),
            cardSocket: ws,
            refineText: .init(get: { text }, set: { text = $0 })
        )
        #expect(view.supportsRefine == true)
    }

    @Test("MultipleChoiceApprovalCard overrides approveDisabled to true")
    func multipleChoiceOverridesApproveDisabled() {
        let ws = CardWebSocket()
        let view = MultipleChoiceApprovalCard(card: makeCard(type: .multipleChoice), socket: ws)
        #expect(view.approveDisabled == true)
    }
}

// MARK: - Card Decoding For All Types

@Suite("Card Type Decoding")
struct CardTypeDecodingTests {

    @Test("Decode action card from JSON")
    func decodeActionCard() throws {
        let card = makeCard(type: .action, silo: .todos)
        #expect(card.cardType == .action)
        #expect(card.silo == .todos)
        if case .action(let desc, let detail) = card.payload {
            #expect(desc == "Create a calendar event")
            #expect(detail != nil)
        } else {
            Issue.record("Expected .action payload")
        }
    }

    @Test("Decode decision card from JSON")
    func decodeDecisionCard() throws {
        let card = makeCard(type: .decision)
        #expect(card.cardType == .decision)
        if case .decision(let question, let context, let options) = card.payload {
            #expect(question == "Should we deploy?")
            #expect(context == "All tests passing")
            #expect(options == ["Yes", "No", "Wait"])
        } else {
            Issue.record("Expected .decision payload")
        }
    }

    @Test("Decode multipleChoice card from JSON")
    func decodeMultipleChoiceCard() throws {
        let card = makeCard(type: .multipleChoice)
        #expect(card.cardType == .multipleChoice)
        if case .multipleChoice(let question, let options) = card.payload {
            #expect(question == "Pick a response")
            #expect(options.count == 3)
        } else {
            Issue.record("Expected .multipleChoice payload")
        }
    }

    @Test("Decode compose card from JSON")
    func decodeComposeCard() throws {
        let card = makeCard(type: .compose)
        #expect(card.cardType == .compose)
        if case .compose(let channel, let recipient, let subject, let body, let confidence) = card.payload {
            #expect(channel == "email")
            #expect(recipient == "bob@example.com")
            #expect(subject == "Meeting")
            #expect(body == "Let's meet at 3pm")
            #expect((confidence - 0.85).magnitude < 0.01)
        } else {
            Issue.record("Expected .compose payload")
        }
    }
}
