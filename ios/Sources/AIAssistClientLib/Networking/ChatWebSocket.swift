import Foundation
import Observation

/// WebSocket client for the Brain chat system.
/// Connects to the Rust server at `/ws/chat`, sends user messages,
/// and receives responses, status updates, and streaming chunks.
@Observable
public final class ChatWebSocket: @unchecked Sendable {
    // MARK: - Published state

    public var messages: [ChatMessage] = []
    public var isConnected: Bool = false
    public var isThinking: Bool = false

    // MARK: - Configuration

    public private(set) var host: String
    public private(set) var port: Int

    // MARK: - Private

    private var webSocketTask: URLSessionWebSocketTask?
    private let session: URLSession
    private var reconnectAttempt: Int = 0
    private let maxReconnectDelay: TimeInterval = 30.0
    private var isIntentionalDisconnect = false

    /// Tracks the in-progress streaming message so chunks append to it.
    private var streamingMessageId: UUID?

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
        guard let url = URL(string: "ws://\(host):\(port)/ws/chat") else { return }
        let task = session.webSocketTask(with: url)
        self.webSocketTask = task
        task.resume()
        isConnected = true
        reconnectAttempt = 0
        receiveMessage()
    }

    // MARK: - Sending

    /// Send a user message to the AI agent.
    public func send(text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        // Add the user message to local state immediately
        let userMessage = ChatMessage(content: trimmed, isFromUser: true)
        DispatchQueue.main.async { [weak self] in
            self?.messages.append(userMessage)
        }

        // Send over WebSocket
        let payload: [String: String] = ["type": "message", "content": trimmed]
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
              let jsonString = String(data: data, encoding: .utf8) else { return }
        webSocketTask?.send(.string(jsonString)) { _ in }
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

        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = json["type"] as? String else { return }

        DispatchQueue.main.async { [weak self] in
            self?.applyMessage(type: type, json: json)
        }
    }

    private func applyMessage(type: String, json: [String: Any]) {
        switch type {
        case "status":
            // {"type":"status","kind":"thinking"}
            if let kind = json["kind"] as? String {
                isThinking = (kind == "thinking")
            }

        case "stream_chunk":
            // {"type":"stream_chunk","content":"partial text"}
            isThinking = false
            if let content = json["content"] as? String {
                appendStreamChunk(content)
            }

        case "response":
            // {"type":"response","content":"full reply"}
            isThinking = false
            finalizeStream()
            if let content = json["content"] as? String {
                let aiMessage = ChatMessage(content: content, isFromUser: false)
                messages.append(aiMessage)
            }

        default:
            break
        }
    }

    // MARK: - Streaming

    /// Append a chunk to the current streaming AI message, creating one if needed.
    private func appendStreamChunk(_ chunk: String) {
        if let id = streamingMessageId,
           let index = messages.firstIndex(where: { $0.id == id }) {
            // Append to existing streaming message
            messages[index].content += chunk
        } else {
            // Start a new streaming message
            let aiMessage = ChatMessage(content: chunk, isFromUser: false)
            streamingMessageId = aiMessage.id
            messages.append(aiMessage)
        }
    }

    /// End the current streaming message (if any).
    private func finalizeStream() {
        streamingMessageId = nil
    }

    // MARK: - Reconnection

    private func handleDisconnect() {
        DispatchQueue.main.async { [weak self] in
            self?.isConnected = false
            self?.isThinking = false
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
