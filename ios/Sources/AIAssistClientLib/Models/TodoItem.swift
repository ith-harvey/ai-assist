import Foundation
import SwiftUI

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

    /// Status color for UI indicators (stripe, icon tint).
    public var color: Color {
        switch self {
        case .created: .blue
        case .agentWorking: .orange
        case .readyForReview: .green
        case .waitingOnYou: .purple
        case .snoozed: .gray
        case .completed: .green
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
    public var parentId: UUID?
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
        parentId: UUID? = nil,
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
        self.parentId = parentId
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
        case dueDate, context, sourceCardId, snoozedUntil, parentId, createdAt, updatedAt
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
        parentId = try container.decodeIfPresent(UUID.self, forKey: .parentId)
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
            title: "Fix Stellar fee rounding bug",
            description: "Luca flagged failing integration tests on the fee calculation path. Check the rounding logic in the stablecoin transfer module.",
            todoType: .deliverable,
            bucket: .humanOnly,
            status: .waitingOnYou,
            priority: 1,
            dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
            context: "M0 release blocker — needs fix before Thursday cut"
        ),
        TodoItem(
            title: "Research Nashville flights for Joey's bachelor party",
            description: "Check Southwest and Frontier from LAS to BNA, March 15-17. Budget ~$400 for airfare. Send options to Joey.",
            todoType: .research,
            bucket: .agentStartable,
            status: .agentWorking,
            priority: 2,
            dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
            context: "Joey asked via email — wants response by tomorrow"
        ),
        TodoItem(
            title: "Pick up groceries from Whole Foods",
            description: "Milk, eggs, sourdough bread. Christina asked.",
            todoType: .errand,
            bucket: .humanOnly,
            status: .created,
            priority: 3,
            dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date())
        ),
        TodoItem(
            title: "Review Atlanta location scout photos",
            description: "Mike sent two house options for the slasher scenes. Review photos, pick top choice, and send notes on lighting angles.",
            todoType: .creative,
            bucket: .humanOnly,
            status: .created,
            priority: 3,
            dueDate: Calendar.current.date(byAdding: .day, value: 3, to: Date()),
            context: "Slasher film production — Atlanta shoot"
        ),
        TodoItem(
            title: "Upgrade OpenClaw to v2026.2.24",
            description: "Per-agent model overrides, session cleanup improvements. Will fix Clark's contextTokens mismatch.",
            todoType: .administrative,
            bucket: .agentStartable,
            status: .created,
            priority: 4,
            dueDate: Calendar.current.date(byAdding: .day, value: 5, to: Date()),
            context: "Changelog has migration notes — read before upgrading"
        ),
        TodoItem(
            title: "Watch Karpathy on Lex Fridman — tool use and agents",
            description: "Episode #428. Directly relevant to AI Assist architecture. Take notes for Second Brain.",
            todoType: .learning,
            bucket: .humanOnly,
            status: .created,
            priority: 5,
            dueDate: Calendar.current.date(byAdding: .day, value: 7, to: Date())
        ),
        TodoItem(
            title: "Address Rex's PR review comments",
            description: "Two comments on migration backfill query. Handle NULL payload rows edge case.",
            todoType: .review,
            bucket: .humanOnly,
            status: .readyForReview,
            priority: 2,
            dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
            context: "GitHub notification — Rex requesting changes"
        ),
        TodoItem(
            title: "Confirm Friday electrician appointment",
            description: "Christina asked — Thursday or Friday. Picked Friday. Need to confirm morning or afternoon.",
            todoType: .administrative,
            bucket: .humanOnly,
            status: .created,
            priority: 3,
            dueDate: Calendar.current.date(byAdding: .day, value: 3, to: Date()),
            context: "Christina asked via WhatsApp"
        ),
        TodoItem(
            title: "Merge PR #64 — todo row styling",
            description: "Card width matching, inline input on expand, removed bottom bar.",
            todoType: .review,
            bucket: .humanOnly,
            status: .completed,
            priority: 2,
            dueDate: Calendar.current.date(byAdding: .day, value: -1, to: Date())
        ),
        TodoItem(
            title: "Draft AI Assist onboarding flow copy",
            description: "Write the 5-screen onboarding sequence: account creation, connect services, personality setup, preferences, UI tutorial. Goal: value in under 2 minutes.",
            todoType: .deliverable,
            bucket: .agentStartable,
            status: .agentWorking,
            priority: 3,
            dueDate: Calendar.current.date(byAdding: .day, value: 5, to: Date()),
            context: "From UX brainstorm doc — four silos, two patterns"
        ),
    ]
}
