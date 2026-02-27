import Foundation

// MARK: - Activity Message

/// A single event in the agent activity stream for a todo.
///
/// Mirrors the backend `TodoActivityMessage` enum (Rust):
/// ```rust
/// #[serde(tag = "type", rename_all = "snake_case")]
/// pub enum TodoActivityMessage { ... }
/// ```
///
/// Wire format example:
/// ```json
/// {"type": "tool_completed", "job_id": "...", "tool_name": "shell", "success": true, "summary": "..."}
/// ```
public enum ActivityMessage: Identifiable, Codable, Sendable {
    case started(jobId: UUID, todoId: UUID?)
    case thinking(jobId: UUID, iteration: UInt32)
    case toolStarted(jobId: UUID, toolName: String)
    case toolCompleted(jobId: UUID, toolName: String, success: Bool, summary: String)
    case reasoning(jobId: UUID, content: String)
    case agentResponse(jobId: UUID, content: String)
    case completed(jobId: UUID, summary: String)
    case failed(jobId: UUID, error: String)

    // MARK: - Identifiable

    /// Stable identity for SwiftUI list rendering.
    /// Combines type discriminator + job_id + variant-specific data for uniqueness.
    public var id: String {
        switch self {
        case .started(let jobId, _):
            return "started-\(jobId.uuidString)"
        case .thinking(let jobId, let iteration):
            return "thinking-\(jobId.uuidString)-\(iteration)"
        case .toolStarted(let jobId, let toolName):
            return "tool_started-\(jobId.uuidString)-\(toolName)"
        case .toolCompleted(let jobId, let toolName, _, _):
            return "tool_completed-\(jobId.uuidString)-\(toolName)"
        case .reasoning(let jobId, let content):
            return "reasoning-\(jobId.uuidString)-\(content.prefix(20).hashValue)"
        case .agentResponse(let jobId, let content):
            return "agent_response-\(jobId.uuidString)-\(content.prefix(20).hashValue)"
        case .completed(let jobId, _):
            return "completed-\(jobId.uuidString)"
        case .failed(let jobId, _):
            return "failed-\(jobId.uuidString)"
        }
    }

    // MARK: - Computed

    /// The job ID common to all variants.
    public var jobId: UUID {
        switch self {
        case .started(let id, _),
             .thinking(let id, _),
             .toolStarted(let id, _),
             .toolCompleted(let id, _, _, _),
             .reasoning(let id, _),
             .agentResponse(let id, _),
             .completed(let id, _),
             .failed(let id, _):
            return id
        }
    }

    /// Whether this is a terminal event (completed or failed).
    public var isTerminal: Bool {
        switch self {
        case .completed, .failed: return true
        default: return false
        }
    }

    // MARK: - Codable (tagged enum: "type" field)

    private enum CodingKeys: String, CodingKey {
        case type
        case jobId = "job_id"
        case todoId = "todo_id"
        case iteration
        case toolName = "tool_name"
        case success
        case summary
        case content
        case error
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)

        switch type {
        case "started":
            let jobId = try container.decode(UUID.self, forKey: .jobId)
            let todoId = try container.decodeIfPresent(UUID.self, forKey: .todoId)
            self = .started(jobId: jobId, todoId: todoId)

        case "thinking":
            let jobId = try container.decode(UUID.self, forKey: .jobId)
            let iteration = try container.decode(UInt32.self, forKey: .iteration)
            self = .thinking(jobId: jobId, iteration: iteration)

        case "tool_started":
            let jobId = try container.decode(UUID.self, forKey: .jobId)
            let toolName = try container.decode(String.self, forKey: .toolName)
            self = .toolStarted(jobId: jobId, toolName: toolName)

        case "tool_completed":
            let jobId = try container.decode(UUID.self, forKey: .jobId)
            let toolName = try container.decode(String.self, forKey: .toolName)
            let success = try container.decode(Bool.self, forKey: .success)
            let summary = try container.decode(String.self, forKey: .summary)
            self = .toolCompleted(jobId: jobId, toolName: toolName, success: success, summary: summary)

        case "reasoning":
            let jobId = try container.decode(UUID.self, forKey: .jobId)
            let content = try container.decode(String.self, forKey: .content)
            self = .reasoning(jobId: jobId, content: content)

        case "agent_response":
            let jobId = try container.decode(UUID.self, forKey: .jobId)
            let content = try container.decode(String.self, forKey: .content)
            self = .agentResponse(jobId: jobId, content: content)

        case "completed":
            let jobId = try container.decode(UUID.self, forKey: .jobId)
            let summary = try container.decode(String.self, forKey: .summary)
            self = .completed(jobId: jobId, summary: summary)

        case "failed":
            let jobId = try container.decode(UUID.self, forKey: .jobId)
            let error = try container.decode(String.self, forKey: .error)
            self = .failed(jobId: jobId, error: error)

        default:
            throw DecodingError.dataCorruptedError(
                forKey: .type,
                in: container,
                debugDescription: "Unknown activity message type: \(type)"
            )
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)

        switch self {
        case .started(let jobId, let todoId):
            try container.encode("started", forKey: .type)
            try container.encode(jobId, forKey: .jobId)
            try container.encodeIfPresent(todoId, forKey: .todoId)

        case .thinking(let jobId, let iteration):
            try container.encode("thinking", forKey: .type)
            try container.encode(jobId, forKey: .jobId)
            try container.encode(iteration, forKey: .iteration)

        case .toolStarted(let jobId, let toolName):
            try container.encode("tool_started", forKey: .type)
            try container.encode(jobId, forKey: .jobId)
            try container.encode(toolName, forKey: .toolName)

        case .toolCompleted(let jobId, let toolName, let success, let summary):
            try container.encode("tool_completed", forKey: .type)
            try container.encode(jobId, forKey: .jobId)
            try container.encode(toolName, forKey: .toolName)
            try container.encode(success, forKey: .success)
            try container.encode(summary, forKey: .summary)

        case .reasoning(let jobId, let content):
            try container.encode("reasoning", forKey: .type)
            try container.encode(jobId, forKey: .jobId)
            try container.encode(content, forKey: .content)

        case .agentResponse(let jobId, let content):
            try container.encode("agent_response", forKey: .type)
            try container.encode(jobId, forKey: .jobId)
            try container.encode(content, forKey: .content)

        case .completed(let jobId, let summary):
            try container.encode("completed", forKey: .type)
            try container.encode(jobId, forKey: .jobId)
            try container.encode(summary, forKey: .summary)

        case .failed(let jobId, let error):
            try container.encode("failed", forKey: .type)
            try container.encode(jobId, forKey: .jobId)
            try container.encode(error, forKey: .error)
        }
    }

    /// Decode from raw JSON data.
    public static func decode(from data: Data) throws -> ActivityMessage {
        let decoder = JSONDecoder()
        return try decoder.decode(ActivityMessage.self, from: data)
    }

    /// Decode an array from JSON data.
    public static func decodeArray(from data: Data) throws -> [ActivityMessage] {
        let decoder = JSONDecoder()
        return try decoder.decode([ActivityMessage].self, from: data)
    }
}
