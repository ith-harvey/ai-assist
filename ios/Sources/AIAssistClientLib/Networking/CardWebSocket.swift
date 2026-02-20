import Foundation
import Observation

/// WebSocket client for the card system.
/// Connects to the Rust server, decodes `WsMessage` variants, and sends `CardAction`.
@Observable
public final class CardWebSocket: @unchecked Sendable {
    // MARK: - Published state

    public var cards: [ReplyCard] = []
    public var isConnected: Bool = false
    /// True while waiting for a `card_refreshed` response after a refine action.
    public var isRefining: Bool = false

    // MARK: - Configuration

    public private(set) var host: String
    public private(set) var port: Int

    // MARK: - Private

    private var webSocketTask: URLSessionWebSocketTask?
    private let session: URLSession
    private var reconnectAttempt: Int = 0
    private let maxReconnectDelay: TimeInterval = 30.0
    private var isIntentionalDisconnect = false

    public init(host: String = "192.168.0.5", port: Int = 8080) {
        self.host = host
        self.port = port
        self.session = URLSession(configuration: .default)
    }

    // MARK: - Connection

    public func connect() {
        isIntentionalDisconnect = false
        reconnectAttempt = 0
        openConnection()
    }

    public func disconnect() {
        isIntentionalDisconnect = true
        webSocketTask?.cancel(with: .normalClosure, reason: nil)
        webSocketTask = nil
        isConnected = false
    }

    public func updateServer(host: String, port: Int) {
        let wasConnected = isConnected
        disconnect()
        self.host = host
        self.port = port
        if wasConnected {
            connect()
        }
    }

    private func openConnection() {
        guard let url = URL(string: "ws://\(host):\(port)/ws") else { return }
        let task = session.webSocketTask(with: url)
        self.webSocketTask = task
        task.resume()
        isConnected = true
        reconnectAttempt = 0
        receiveMessage()
    }

    // MARK: - Receiving

    private func receiveMessage() {
        webSocketTask?.receive { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let message):
                self.handleMessage(message)
                self.receiveMessage()
            case .failure:
                self.handleDisconnect()
            }
        }
    }

    private func handleMessage(_ message: URLSessionWebSocketTask.Message) {
        let data: Data
        switch message {
        case .string(let text):
            guard let textData = text.data(using: .utf8) else { return }
            data = textData
        case .data(let raw):
            data = raw
        @unknown default:
            return
        }

        guard let wsMessage = try? WsMessage.decode(from: data) else { return }

        DispatchQueue.main.async { [weak self] in
            self?.applyMessage(wsMessage)
        }
    }

    private func applyMessage(_ message: WsMessage) {
        switch message {
        case .newCard(let card):
            cards.append(card)
        case .cardUpdate(let id, let status):
            if let index = cards.firstIndex(where: { $0.id == id }) {
                cards[index].status = status
                if status != .pending {
                    cards.remove(at: index)
                }
            }
        case .cardExpired(let id):
            cards.removeAll { $0.id == id }
        case .cardsSync(let syncedCards):
            cards = syncedCards.filter { $0.status == .pending }
        case .cardRefreshed(let card):
            isRefining = false
            if let index = cards.firstIndex(where: { $0.id == card.id }) {
                cards[index] = card
            } else {
                // Card was removed in the interim — re-add if still pending
                if card.status == .pending {
                    cards.insert(card, at: 0)
                }
            }
        case .ping:
            break
        }
    }

    // MARK: - Sending

    public func send(action: CardAction) {
        guard let data = try? action.toData(),
              let text = String(data: data, encoding: .utf8) else { return }
        webSocketTask?.send(.string(text)) { _ in }
    }

    public func approve(cardId: UUID) {
        send(action: .approve(cardId: cardId))
        cards.removeAll { $0.id == cardId }
    }

    public func dismiss(cardId: UUID) {
        send(action: .dismiss(cardId: cardId))
        cards.removeAll { $0.id == cardId }
    }

    public func edit(cardId: UUID, newText: String) {
        send(action: .edit(cardId: cardId, newText: newText))
        cards.removeAll { $0.id == cardId }
    }

    public func refine(cardId: UUID, instruction: String) {
        isRefining = true
        send(action: .refine(cardId: cardId, instruction: instruction))
    }

    // MARK: - Reconnection

    private func handleDisconnect() {
        DispatchQueue.main.async { [weak self] in
            self?.isConnected = false
        }

        guard !isIntentionalDisconnect else { return }

        let delay = reconnectDelay()
        reconnectAttempt += 1

        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self, !self.isIntentionalDisconnect else { return }
            self.openConnection()
        }
    }

    /// Exponential backoff: 1s, 2s, 4s, 8s, ... capped at `maxReconnectDelay`.
    public func reconnectDelay() -> TimeInterval {
        let delay = pow(2.0, Double(reconnectAttempt))
        return min(delay, maxReconnectDelay)
    }
}
