import Foundation

// MARK: - Enums

/// Type/category of a to-do item.
public enum TodoType: String, Codable, Sendable, CaseIterable {
    case deliverable
    case research
    case errand
    case learning
    case administrative
    case creative
    case review

    /// Human-readable label for display.
    public var label: String {
        switch self {
        case .deliverable: "Deliverable"
        case .research: "Research"
        case .errand: "Errand"
        case .learning: "Learning"
        case .administrative: "Admin"
        case .creative: "Creative"
        case .review: "Review"
        }
    }

    /// Badge color for the type pill.
    public var color: String {
        switch self {
        case .deliverable: "blue"
        case .research: "purple"
        case .errand: "orange"
        case .learning: "green"
        case .administrative: "gray"
        case .creative: "pink"
        case .review: "yellow"
        }
    }
}

/// Whether this todo can be started by the agent or requires human action.
public enum TodoBucket: String, Codable, Sendable {
    case agentStartable = "agent_startable"
    case humanOnly = "human_only"
}

/// Lifecycle status of a to-do item.
public enum TodoStatus: String, Codable, Sendable {
    case created
    case agentWorking = "agent_working"
    case readyForReview = "ready_for_review"
    case waitingOnYou = "waiting_on_you"
    case snoozed
    case completed

    /// SF Symbol name for this status.
    public var iconName: String {
        switch self {
        case .created: "doc.text"
        case .agentWorking: "gearshape.2"
        case .readyForReview: "checkmark.circle"
        case .waitingOnYou: "person.fill"
        case .snoozed: "moon.zzz"
        case .completed: "checkmark.circle.fill"
        }
    }

    /// Display label.
    public var label: String {
        switch self {
        case .created: "Created"
        case .agentWorking: "Agent working"
        case .readyForReview: "Ready for review"
        case .waitingOnYou: "Waiting on you"
        case .snoozed: "Snoozed"
        case .completed: "Completed"
        }
    }

    /// Whether this status counts as "active" (not completed or snoozed).
    public var isActive: Bool {
        switch self {
        case .completed, .snoozed: false
        default: true
        }
    }
}

// MARK: - Model

/// A to-do item. Mirrors the backend TodoItem struct.
public struct TodoItem: Identifiable, Hashable, Sendable {
    public let id: UUID
    public var title: String
    public var description: String?
    public var todoType: TodoType
    public var bucket: TodoBucket
    public var status: TodoStatus
    public var priority: Int
    public var dueDate: Date?
    public var context: String?
    public var sourceCardId: UUID?
    public var snoozedUntil: Date?
    public var createdAt: Date
    public var updatedAt: Date

    public init(
        id: UUID = UUID(),
        title: String,
        description: String? = nil,
        todoType: TodoType = .deliverable,
        bucket: TodoBucket = .humanOnly,
        status: TodoStatus = .created,
        priority: Int = 3,
        dueDate: Date? = nil,
        context: String? = nil,
        sourceCardId: UUID? = nil,
        snoozedUntil: Date? = nil,
        createdAt: Date = Date(),
        updatedAt: Date = Date()
    ) {
        self.id = id
        self.title = title
        self.description = description
        self.todoType = todoType
        self.bucket = bucket
        self.status = status
        self.priority = priority
        self.dueDate = dueDate
        self.context = context
        self.sourceCardId = sourceCardId
        self.snoozedUntil = snoozedUntil
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    /// Whether the due date has passed.
    public var isOverdue: Bool {
        guard let due = dueDate else { return false }
        return due < Date() && status != .completed
    }
}

// MARK: - Codable

extension TodoItem: Codable {
    enum CodingKeys: String, CodingKey {
        case id, title, description, todoType, bucket, status, priority
        case dueDate, context, sourceCardId, snoozedUntil, createdAt, updatedAt
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UUID.self, forKey: .id)
        title = try container.decode(String.self, forKey: .title)
        description = try container.decodeIfPresent(String.self, forKey: .description)
        todoType = try container.decode(TodoType.self, forKey: .todoType)
        bucket = try container.decode(TodoBucket.self, forKey: .bucket)
        status = try container.decode(TodoStatus.self, forKey: .status)
        priority = try container.decode(Int.self, forKey: .priority)
        dueDate = try container.decodeIfPresent(Date.self, forKey: .dueDate)
        context = try container.decodeIfPresent(String.self, forKey: .context)
        sourceCardId = try container.decodeIfPresent(UUID.self, forKey: .sourceCardId)
        snoozedUntil = try container.decodeIfPresent(Date.self, forKey: .snoozedUntil)
        createdAt = try container.decode(Date.self, forKey: .createdAt)
        updatedAt = try container.decode(Date.self, forKey: .updatedAt)
    }

    /// Decode from snake_case JSON.
    public static func decode(from data: Data) throws -> TodoItem {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(TodoItem.self, from: data)
    }

    /// Decode an array from snake_case JSON.
    public static func decodeArray(from data: Data) throws -> [TodoItem] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode([TodoItem].self, from: data)
    }
}

// MARK: - Sample Data

extension TodoItem {
    /// Sample data for previews and development (backend not yet ready).
    public static let samples: [TodoItem] = [
        TodoItem(
            title: "Review Q1 budget proposal",
            description: "Sarah sent the updated numbers. Check the marketing line items.",
            todoType: .review,
            bucket: .humanOnly,
            status: .waitingOnYou,
            priority: 1,
            dueDate: Calendar.current.date(byAdding: .hour, value: 4, to: Date())
        ),
        TodoItem(
            title: "Research vector DB options for embeddings",
            description: "Compare pgvector, Qdrant, and Pinecone for our use case. Focus on cost at 10M vectors.",
            todoType: .research,
            bucket: .agentStartable,
            status: .agentWorking,
            priority: 2
        ),
        TodoItem(
            title: "Pick up dry cleaning",
            todoType: .errand,
            bucket: .humanOnly,
            status: .created,
            priority: 4,
            dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date())
        ),
        TodoItem(
            title: "Write blog post on tool-use patterns",
            description: "Draft based on the ironclaw vs ai-assist comparison. Focus on the enforcement stack.",
            todoType: .creative,
            bucket: .agentStartable,
            status: .readyForReview,
            priority: 3
        ),
        TodoItem(
            title: "Learn Swift concurrency model",
            description: "Structured concurrency, actors, sendable. Work through the WWDC sessions.",
            todoType: .learning,
            bucket: .humanOnly,
            status: .created,
            priority: 5
        ),
        TodoItem(
            title: "Renew AWS certificates",
            todoType: .administrative,
            bucket: .agentStartable,
            status: .snoozed,
            priority: 3,
            snoozedUntil: Calendar.current.date(byAdding: .day, value: 3, to: Date())
        ),
        TodoItem(
            title: "File expense report",
            todoType: .administrative,
            bucket: .humanOnly,
            status: .completed,
            priority: 2,
            dueDate: Calendar.current.date(byAdding: .day, value: -1, to: Date())
        ),
    ]
}
