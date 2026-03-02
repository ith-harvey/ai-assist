import Foundation
import Observation

// MARK: - StatusEvent Model

/// Represents a typed agent activity event from the server.
public struct StatusEvent: Identifiable, Sendable {
    public let id = UUID()
    public let kind: StatusKind
    public let timestamp = Date()

    public enum StatusKind: Sendable {
        case thinking(String)
        case toolStarted(name: String)
        case toolCompleted(name: String, success: Bool)
        case toolResult(name: String, preview: String)
        case error(String)
        case status(String)
    }
}

// MARK: - ChatWebSocket

/// WebSocket client for the Brain chat system.
/// Connects to the Rust server at `/ws/chat`, sends user messages,
/// and receives responses, status updates, and streaming chunks.
@Observable
public final class ChatWebSocket: @unchecked Sendable {
    // MARK: - Published state

    public var messages: [ChatMessage] = []
    public var isConnected: Bool = false

    /// Current agent activity. Set during processing, cleared on response.
    public var currentStatus: StatusEvent?

    /// Convenience — true when the agent is actively working.
    public var isThinking: Bool { currentStatus != nil }

    /// Thread ID for conversation continuity. Persisted in UserDefaults.
    public var currentThreadId: String?

    // MARK: - Thread Persistence

    private static let threadIdKey = "ai_assist_chat_thread_id"

    /// Persistent thread ID — reused across app launches.
    public var threadId: String {
        if let stored = UserDefaults.standard.string(forKey: Self.threadIdKey) {
            return stored
        }
        let newId = UUID().uuidString
        UserDefaults.standard.set(newId, forKey: Self.threadIdKey)
        return newId
    }

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

    /// IDs of messages loaded from history, used to dedup live WS messages.
    private var knownMessageIds: Set<UUID> = []

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
        loadHistory()
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

        // Send over WebSocket (include thread_id for conversation continuity)
        let payload: [String: String] = [
            "type": "message",
            "content": trimmed,
            "thread_id": threadId,
        ]
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

        // --- Rich status events ---

        case "thinking":
            // {"type":"thinking","message":"Processing..."}
            let msg = json["message"] as? String ?? ""
            currentStatus = StatusEvent(kind: .thinking(msg))

        case "tool_started":
            // {"type":"tool_started","name":"shell"}
            let name = json["name"] as? String ?? "tool"
            currentStatus = StatusEvent(kind: .toolStarted(name: name))

        case "tool_completed":
            // {"type":"tool_completed","name":"shell","success":true}
            let name = json["name"] as? String ?? "tool"
            let success = json["success"] as? Bool ?? true
            currentStatus = StatusEvent(kind: .toolCompleted(name: name, success: success))

        case "tool_result":
            // {"type":"tool_result","name":"shell","preview":"3 files found"}
            let name = json["name"] as? String ?? "tool"
            let preview = json["preview"] as? String ?? ""
            currentStatus = StatusEvent(kind: .toolResult(name: name, preview: preview))

        case "error":
            // {"type":"error","message":"Something went wrong"}
            let msg = json["message"] as? String ?? "Unknown error"
            currentStatus = StatusEvent(kind: .error(msg))

        // --- Legacy status fallback ---

        case "status":
            if let kind = json["kind"] as? String {
                // Old format: {"type":"status","kind":"thinking"}
                if kind == "thinking" {
                    currentStatus = StatusEvent(kind: .thinking(""))
                } else {
                    currentStatus = StatusEvent(kind: .status(kind))
                }
            } else if let msg = json["message"] as? String {
                // New format: {"type":"status","message":"General status info"}
                currentStatus = StatusEvent(kind: .status(msg))
            }

        // --- Content messages ---

        case "stream_chunk":
            // {"type":"stream_chunk","content":"partial text","thread_id":"abc-123"}
            if let threadId = json["thread_id"] as? String {
                currentThreadId = threadId
            }
            // Clear thinking status once content starts flowing
            currentStatus = nil
            if let content = json["content"] as? String {
                appendStreamChunk(content)
            }

        case "response":
            // {"type":"response","content":"full reply","thread_id":"abc-123"}
            if let threadId = json["thread_id"] as? String {
                currentThreadId = threadId
            }
            currentStatus = nil
            finalizeStream()
            if let content = json["content"] as? String {
                // Dedup: skip if this message was already loaded from history
                if let idStr = json["id"] as? String,
                   let serverId = UUID(uuidString: idStr),
                   knownMessageIds.contains(serverId) {
                    break
                }
                let aiMessage = ChatMessage(content: content, isFromUser: false)
                messages.append(aiMessage)
            }

        // --- Onboarding ---

        case "onboarding_phase":
            // {"type":"onboarding_phase","phase":"complete","completed":true}
            let phase = json["phase"] as? String ?? "unknown"
            let completed = json["completed"] as? Bool ?? false
            if completed {
                // Post notification so OnboardingChatView can transition
                NotificationCenter.default.post(
                    name: Notification.Name("ai_assist_onboarding_completed"),
                    object: nil,
                    userInfo: ["phase": phase]
                )
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
            messages[index].content += chunk
        } else {
            let aiMessage = ChatMessage(content: chunk, isFromUser: false)
            streamingMessageId = aiMessage.id
            messages.append(aiMessage)
        }
    }

    /// End the current streaming message (if any).
    private func finalizeStream() {
        streamingMessageId = nil
    }

    // MARK: - History

    /// Load previous messages from the REST API so conversations survive app restarts.
    private func loadHistory() {
        let tid = threadId
        guard let url = URL(string: "http://\(host):\(port)/api/chat/history?thread_id=\(tid)&limit=50") else { return }

        URLSession.shared.dataTask(with: url) { [weak self] data, _, _ in
            guard let self, let data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let messagesJson = json["messages"] as? [[String: Any]] else { return }

            let isoFormatter = ISO8601DateFormatter()
            isoFormatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]

            let historyMessages: [ChatMessage] = messagesJson.compactMap { msg in
                guard let role = msg["role"] as? String,
                      let content = msg["content"] as? String else { return nil }

                let id = UUID(uuidString: msg["id"] as? String ?? "") ?? UUID()
                let timestamp: Date
                if let ts = msg["timestamp"] as? String {
                    timestamp = isoFormatter.date(from: ts) ?? Date()
                } else {
                    timestamp = Date()
                }

                return ChatMessage(
                    id: id,
                    content: content,
                    isFromUser: role == "user",
                    timestamp: timestamp
                )
            }

            DispatchQueue.main.async {
                // Track known IDs for dedup against live WS messages
                self.knownMessageIds = Set(historyMessages.map(\.id))
                // Only replace if we haven't received live messages yet
                if self.messages.isEmpty {
                    self.messages = historyMessages
                } else {
                    // Prepend history before any live messages, skipping dupes
                    let liveIds = Set(self.messages.map(\.id))
                    let newHistory = historyMessages.filter { !liveIds.contains($0.id) }
                    self.messages = newHistory + self.messages
                }
            }
        }.resume()
    }

    // MARK: - Reconnection

    private func handleDisconnect() {
        DispatchQueue.main.async { [weak self] in
            self?.isConnected = false
            self?.currentStatus = nil
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
