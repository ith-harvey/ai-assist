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

    public init(todoId: UUID, host: String = "localhost", port: Int = 8080) {
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
            print("📡 [ActivitySocket] Invalid URL for todo \(todoIdStr)")
            return
        }
        print("📡 [ActivitySocket] Connecting to \(url)")
        let task = session.webSocketTask(with: url)
        self.webSocketTask = task
        task.resume()
        isConnected = true
        reconnectAttempt = 0
        print("📡 [ActivitySocket] Task resumed, starting receive loop")
        receiveMessage()
    }

    // MARK: - Receiving

    private func receiveMessage() {
        webSocketTask?.receive { [weak self] result in
            guard let self else {
                print("📡 [ActivitySocket] self was deallocated in receive callback")
                return
            }
            switch result {
            case .success(let message):
                switch message {
                case .string(let text):
                    print("📡 [ActivitySocket] Received text (\(text.count) chars): \(String(text.prefix(150)))")
                case .data(let data):
                    print("📡 [ActivitySocket] Received data (\(data.count) bytes)")
                @unknown default:
                    print("📡 [ActivitySocket] Received unknown message type")
                }
                self.handleMessage(message)
                self.receiveMessage()
            case .failure(let error):
                print("📡 [ActivitySocket] Receive FAILED: \(error)")
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

        do {
            let activityMessage = try ActivityMessage.decode(from: data)
            print("📡 [ActivitySocket] Decoded: \(activityMessage.id) (terminal: \(activityMessage.isTerminal))")
            DispatchQueue.main.async { [weak self] in
                self?.messages.append(activityMessage)
                self?.latestActivity = activityMessage
                print("📡 [ActivitySocket] UI updated — messages: \(self?.messages.count ?? 0), isConnected: \(self?.isConnected ?? false)")
            }
        } catch {
            print("📡 [ActivitySocket] DECODE FAILED: \(error)")
            if let text = String(data: data, encoding: .utf8) {
                print("📡 [ActivitySocket] Raw JSON: \(String(text.prefix(300)))")
            }
        }
    }

    // MARK: - Reconnection

    private func handleDisconnect() {
        print("📡 [ActivitySocket] handleDisconnect called (intentional: \(isIntentionalDisconnect), finished: \(isFinished), attempt: \(reconnectAttempt))")
        DispatchQueue.main.async { [weak self] in
            self?.isConnected = false
        }

        guard !isIntentionalDisconnect else {
            print("📡 [ActivitySocket] Intentional disconnect, not reconnecting")
            return
        }

        // Don't reconnect if stream already finished
        if isFinished {
            print("📡 [ActivitySocket] Stream finished, not reconnecting")
            return
        }

        let delay = reconnectDelay()
        reconnectAttempt += 1
        print("📡 [ActivitySocket] Scheduling reconnect in \(delay)s (attempt \(reconnectAttempt))")

        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self, !self.isIntentionalDisconnect else { return }
            print("📡 [ActivitySocket] Reconnecting now...")
            self.openConnection()
        }
    }

    /// Exponential backoff: 1s, 2s, 4s, 8s, ... capped at `maxReconnectDelay`.
    public func reconnectDelay() -> TimeInterval {
        let delay = pow(2.0, Double(reconnectAttempt))
        return min(delay, maxReconnectDelay)
    }
}
