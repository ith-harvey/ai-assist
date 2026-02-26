import Foundation
import Observation

/// WebSocket client for a single todo's agent activity stream.
///
/// Connects to `/ws/todos/{todoId}/activity` and receives `ActivityMessage` events.
/// On connect, the server replays stored history then streams live events.
///
/// Mirrors the `TodoWebSocket` / `CardWebSocket` pattern.
@Observable
public final class TodoActivitySocket: @unchecked Sendable {

    // MARK: - Published State

    /// The latest activity event for this todo (replaces on each update).
    public var latestActivity: ActivityMessage? = nil
    /// Full history kept internally for replay on reconnect.
    public private(set) var messages: [ActivityMessage] = []
    public var isConnected: Bool = false

    // MARK: - Configuration

    public let todoId: UUID
    public private(set) var host: String
    public private(set) var port: Int

    // MARK: - Private

    private var webSocketTask: URLSessionWebSocketTask?
    private let session: URLSession
    private var reconnectAttempt: Int = 0
    private let maxReconnectDelay: TimeInterval = 30.0
    private var isIntentionalDisconnect = false

    public init(todoId: UUID, host: String = "192.168.0.5", port: Int = 8080) {
        self.todoId = todoId
        self.host = host
        self.port = port
        self.session = URLSession(configuration: .default)
    }

    // MARK: - Computed

    /// Whether the activity stream has reached a terminal state (completed or failed).
    public var isFinished: Bool {
        messages.last?.isTerminal ?? false
    }

    /// The most recent message, if any.
    public var latestMessage: ActivityMessage? {
        messages.last
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

    private func openConnection() {
        let todoIdStr = todoId.uuidString.lowercased()
        guard let url = URL(string: "ws://\(host):\(port)/ws/todos/\(todoIdStr)/activity") else {
            return
        }
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

        guard let activityMessage = try? ActivityMessage.decode(from: data) else { return }

        DispatchQueue.main.async { [weak self] in
            self?.messages.append(activityMessage)
            self?.latestActivity = activityMessage
        }
    }

    // MARK: - Reconnection

    private func handleDisconnect() {
        DispatchQueue.main.async { [weak self] in
            self?.isConnected = false
        }

        guard !isIntentionalDisconnect else { return }

        // Don't reconnect if stream already finished
        if isFinished { return }

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
