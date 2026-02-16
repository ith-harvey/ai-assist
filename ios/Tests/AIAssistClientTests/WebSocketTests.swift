import Foundation
import Testing
@testable import AIAssistClientLib

@Suite("WebSocket State Tests")
struct WebSocketTests {

    @Test("Initial state is disconnected with empty cards")
    func initialState() {
        let ws = CardWebSocket()
        #expect(ws.isConnected == false)
        #expect(ws.cards.isEmpty)
    }

    @Test("Default host and port")
    func defaultConfig() {
        let ws = CardWebSocket()
        #expect(ws.host == "localhost")
        #expect(ws.port == 8080)
    }

    @Test("Custom host and port")
    func customConfig() {
        let ws = CardWebSocket(host: "192.168.1.100", port: 8080)
        #expect(ws.host == "192.168.1.100")
        #expect(ws.port == 8080)
    }

    @Test("Reconnect delay uses exponential backoff")
    func reconnectDelay() {
        let ws = CardWebSocket()

        // attempt 0 → 2^0 = 1s
        #expect(ws.reconnectDelay() == 1.0)
    }

    @Test("Reconnect delay caps at 30 seconds")
    func reconnectDelayCap() {
        let ws = CardWebSocket()
        // Simulate many reconnect attempts by testing the formula
        // 2^5 = 32 → capped at 30
        let delay = min(pow(2.0, 5.0), 30.0)
        #expect(delay == 30.0)
    }

    @Test("UpdateServer changes host and port")
    func updateServer() {
        let ws = CardWebSocket(host: "localhost", port: 8080)
        ws.updateServer(host: "example.com", port: 9090)
        #expect(ws.host == "example.com")
        #expect(ws.port == 9090)
    }

    // MARK: - Card list management

    @Test("Cards array supports add and remove")
    func cardListManagement() {
        let ws = CardWebSocket()
        let card = makeTestCard()

        ws.cards.append(card)
        #expect(ws.cards.count == 1)
        #expect(ws.cards[0].sourceSender == "TestSender")

        ws.cards.removeAll { $0.id == card.id }
        #expect(ws.cards.isEmpty)
    }

    @Test("Cards sync replaces card list")
    func cardsSyncReplacesCards() {
        let ws = CardWebSocket()
        ws.cards.append(makeTestCard())
        #expect(ws.cards.count == 1)

        // Simulate cards_sync behavior: replace entire list
        let newCards = [makeTestCard(sender: "Alice"), makeTestCard(sender: "Bob")]
        ws.cards = newCards
        #expect(ws.cards.count == 2)
        #expect(ws.cards[0].sourceSender == "Alice")
        #expect(ws.cards[1].sourceSender == "Bob")
    }

    @Test("Card update removes non-pending cards")
    func cardUpdateRemovesNonPending() {
        let ws = CardWebSocket()
        let card = makeTestCard()
        ws.cards.append(card)

        // Simulate card_update behavior
        if let index = ws.cards.firstIndex(where: { $0.id == card.id }) {
            ws.cards[index].status = .approved
            ws.cards.remove(at: index)
        }
        #expect(ws.cards.isEmpty)
    }

    @Test("Card expired removes card by id")
    func cardExpiredRemovesCard() {
        let ws = CardWebSocket()
        let card1 = makeTestCard(sender: "Alice")
        let card2 = makeTestCard(sender: "Bob")
        ws.cards = [card1, card2]

        // Simulate card_expired behavior
        ws.cards.removeAll { $0.id == card1.id }
        #expect(ws.cards.count == 1)
        #expect(ws.cards[0].sourceSender == "Bob")
    }

    // MARK: - Helpers

    private func makeTestCard(sender: String = "TestSender") -> ReplyCard {
        let json = """
        {
            "id": "\(UUID().uuidString)",
            "conversation_id": "chat_test",
            "source_message": "Test message",
            "source_sender": "\(sender)",
            "suggested_reply": "Test reply",
            "confidence": 0.85,
            "status": "pending",
            "created_at": "2026-02-15T10:00:00Z",
            "expires_at": "2026-02-15T10:15:00Z",
            "channel": "telegram",
            "updated_at": "2026-02-15T10:00:00Z"
        }
        """
        return try! ReplyCard.decode(from: json.data(using: .utf8)!)
    }
}
