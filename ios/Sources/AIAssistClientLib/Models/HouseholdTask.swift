import Foundation
import SwiftUI

// MARK: - Enums

/// Status of a household task — mirrors server enum.
public enum HouseholdTaskStatus: String, Codable, Sendable, CaseIterable, Identifiable {
    case pending
    case inProgress = "in_progress"
    case completed
    case cancelled

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .pending: "Pending"
        case .inProgress: "In Progress"
        case .completed: "Completed"
        case .cancelled: "Cancelled"
        }
    }

    public var icon: String {
        switch self {
        case .pending: "circle"
        case .inProgress: "circle.dotted.circle"
        case .completed: "checkmark.circle.fill"
        case .cancelled: "xmark.circle"
        }
    }

    public var color: Color {
        switch self {
        case .pending: .orange
        case .inProgress: .blue
        case .completed: .green
        case .cancelled: .secondary
        }
    }
}

/// Priority level of a household task — mirrors server enum.
public enum HouseholdTaskPriority: String, Codable, Sendable, CaseIterable, Identifiable {
    case low
    case medium
    case high
    case urgent

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .low: "Low"
        case .medium: "Medium"
        case .high: "High"
        case .urgent: "Urgent"
        }
    }

    public var icon: String {
        switch self {
        case .low: "arrow.down"
        case .medium: "minus"
        case .high: "arrow.up"
        case .urgent: "exclamationmark.2"
        }
    }

    public var color: Color {
        switch self {
        case .low: .secondary
        case .medium: .blue
        case .high: .orange
        case .urgent: .red
        }
    }
}

/// How often a recurring task repeats — mirrors server enum.
public enum RecurrenceRule: Codable, Sendable, Hashable, Identifiable {
    case daily
    case weekly
    case biweekly
    case monthly
    case custom(intervalDays: Int)

    public var id: String {
        switch self {
        case .daily: "daily"
        case .weekly: "weekly"
        case .biweekly: "biweekly"
        case .monthly: "monthly"
        case .custom(let d): "custom_\(d)"
        }
    }

    public var label: String {
        switch self {
        case .daily: "Daily"
        case .weekly: "Weekly"
        case .biweekly: "Every 2 weeks"
        case .monthly: "Monthly"
        case .custom(let d): "Every \(d) days"
        }
    }

    /// Simple presets for picker UI (excludes custom).
    public static let presets: [RecurrenceRule] = [.daily, .weekly, .biweekly, .monthly]

    // Custom Codable to match server's tagged enum format
    public init(from decoder: Decoder) throws {
        // Try as a simple string first
        if let container = try? decoder.singleValueContainer(),
           let raw = try? container.decode(String.self) {
            switch raw {
            case "daily": self = .daily
            case "weekly": self = .weekly
            case "biweekly": self = .biweekly
            case "monthly": self = .monthly
            default: self = .daily
            }
            return
        }
        // Try as tagged object: {"custom": {"interval_days": N}}
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let nested = try? container.nestedContainer(keyedBy: CustomKeys.self, forKey: .custom) {
            let days = try nested.decode(Int.self, forKey: .intervalDays)
            self = .custom(intervalDays: days)
        } else {
            self = .daily
        }
    }

    public func encode(to encoder: Encoder) throws {
        switch self {
        case .daily, .weekly, .biweekly, .monthly:
            var container = encoder.singleValueContainer()
            try container.encode(id)
        case .custom(let days):
            var container = encoder.container(keyedBy: CodingKeys.self)
            var nested = container.nestedContainer(keyedBy: CustomKeys.self, forKey: .custom)
            try nested.encode(days, forKey: .intervalDays)
        }
    }

    private enum CodingKeys: String, CodingKey { case custom }
    private enum CustomKeys: String, CodingKey { case intervalDays = "interval_days" }
}

// MARK: - Model

/// A shared task within a household — matches server `HouseholdTask` contract.
public struct HouseholdTask: Identifiable, Hashable, Sendable {
    public let id: UUID
    public let householdId: UUID
    public var title: String
    public var description: String?
    public var status: HouseholdTaskStatus
    public var priority: HouseholdTaskPriority
    public var assignedTo: String?
    public var dueDate: Date?
    public var recurrence: RecurrenceRule?
    public let createdBy: String
    public let createdAt: Date
    public var updatedAt: Date
    public var completedAt: Date?

    public init(
        id: UUID = UUID(),
        householdId: UUID,
        title: String,
        description: String? = nil,
        status: HouseholdTaskStatus = .pending,
        priority: HouseholdTaskPriority = .medium,
        assignedTo: String? = nil,
        dueDate: Date? = nil,
        recurrence: RecurrenceRule? = nil,
        createdBy: String = "",
        createdAt: Date = Date(),
        updatedAt: Date = Date(),
        completedAt: Date? = nil
    ) {
        self.id = id
        self.householdId = householdId
        self.title = title
        self.description = description
        self.status = status
        self.priority = priority
        self.assignedTo = assignedTo
        self.dueDate = dueDate
        self.recurrence = recurrence
        self.createdBy = createdBy
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.completedAt = completedAt
    }

    /// Whether the task is completed.
    public var isCompleted: Bool { status == .completed }

    /// Whether the task is active (not completed or cancelled).
    public var isActive: Bool { status == .pending || status == .inProgress }

    /// Whether the due date has passed.
    public var isOverdue: Bool {
        guard let due = dueDate else { return false }
        return due < Date() && isActive
    }

    /// Display name for the assignee, resolved from a member list.
    public func assigneeName(from members: [HouseholdMember]) -> String? {
        guard let userId = assignedTo else { return nil }
        return members.first(where: { $0.userId == userId })?.displayName
    }
}

// MARK: - Codable

extension HouseholdTask: Codable {
    enum CodingKeys: String, CodingKey {
        case id, title, description, status, priority
        case householdId, assignedTo, dueDate, recurrence
        case createdBy, createdAt, updatedAt, completedAt
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UUID.self, forKey: .id)
        householdId = try container.decode(UUID.self, forKey: .householdId)
        title = try container.decode(String.self, forKey: .title)
        description = try container.decodeIfPresent(String.self, forKey: .description)
        status = try container.decode(HouseholdTaskStatus.self, forKey: .status)
        priority = try container.decode(HouseholdTaskPriority.self, forKey: .priority)
        assignedTo = try container.decodeIfPresent(String.self, forKey: .assignedTo)
        dueDate = try container.decodeIfPresent(Date.self, forKey: .dueDate)
        recurrence = try container.decodeIfPresent(RecurrenceRule.self, forKey: .recurrence)
        createdBy = try container.decode(String.self, forKey: .createdBy)
        createdAt = try container.decode(Date.self, forKey: .createdAt)
        updatedAt = try container.decode(Date.self, forKey: .updatedAt)
        completedAt = try container.decodeIfPresent(Date.self, forKey: .completedAt)
    }

    public static func decode(from data: Data) throws -> HouseholdTask {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(HouseholdTask.self, from: data)
    }

    public static func decodeArray(from data: Data) throws -> [HouseholdTask] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode([HouseholdTask].self, from: data)
    }
}

// MARK: - Sample Data

extension HouseholdTask {
    public static let samples: [HouseholdTask] = {
        let householdId = Household.sample.id
        let members = HouseholdMember.samples

        return [
            HouseholdTask(
                householdId: householdId,
                title: "Vacuum living room and hallway",
                description: "Don't forget under the couch cushions",
                priority: .medium,
                assignedTo: "emma",
                dueDate: Calendar.current.date(byAdding: .hour, value: 4, to: Date()),
                recurrence: .weekly,
                createdBy: "mom"
            ),
            HouseholdTask(
                householdId: householdId,
                title: "Unload dishwasher",
                priority: .high,
                assignedTo: "jake",
                dueDate: Calendar.current.date(byAdding: .hour, value: 2, to: Date()),
                recurrence: .daily,
                createdBy: "mom"
            ),
            HouseholdTask(
                householdId: householdId,
                title: "Pick up dry cleaning",
                description: "The blue suit and two shirts -- receipt is on the fridge",
                priority: .medium,
                assignedTo: "dad",
                dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
                createdBy: "mom"
            ),
            HouseholdTask(
                householdId: householdId,
                title: "Grocery run -- Whole Foods",
                description: "Milk, eggs, sourdough, chicken thighs, broccoli, strawberries",
                priority: .high,
                assignedTo: "mom",
                dueDate: Calendar.current.date(byAdding: .hour, value: 6, to: Date()),
                createdBy: "mom"
            ),
            HouseholdTask(
                householdId: householdId,
                title: "Prep lunches for the week",
                description: "Turkey wraps for kids, salads for adults. Prep containers in bottom drawer.",
                priority: .medium,
                assignedTo: "mom",
                dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
                recurrence: .weekly,
                createdBy: "dad"
            ),
            HouseholdTask(
                householdId: householdId,
                title: "Take dog to the vet",
                description: "Annual checkup -- appointment at 2pm. Bring vaccination records.",
                priority: .high,
                assignedTo: "mom",
                dueDate: Calendar.current.date(byAdding: .day, value: 3, to: Date()),
                createdBy: "dad"
            ),
            HouseholdTask(
                householdId: householdId,
                title: "Clean bathrooms",
                description: "Both upstairs bathrooms. New cleaning supplies under the kitchen sink.",
                priority: .medium,
                assignedTo: "emma",
                dueDate: Calendar.current.date(byAdding: .day, value: 2, to: Date()),
                recurrence: .weekly,
                createdBy: "mom"
            ),
            HouseholdTask(
                householdId: householdId,
                title: "Mow the lawn",
                status: .completed,
                priority: .low,
                assignedTo: "jake",
                dueDate: Calendar.current.date(byAdding: .day, value: 2, to: Date()),
                recurrence: .weekly,
                createdBy: "dad",
                completedAt: Calendar.current.date(byAdding: .hour, value: -3, to: Date())
            ),
            HouseholdTask(
                householdId: householdId,
                title: "Return Amazon package",
                description: "Wrong size shoes -- label printed, box by the front door",
                status: .completed,
                priority: .low,
                createdBy: "mom",
                completedAt: Calendar.current.date(byAdding: .hour, value: -5, to: Date())
            ),
            HouseholdTask(
                householdId: householdId,
                title: "Schedule piano tuning",
                description: "Call Music Masters",
                priority: .low,
                createdBy: "dad"
            ),
        ]
    }()
}
