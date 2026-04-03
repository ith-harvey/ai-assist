import Foundation
import SwiftUI

// MARK: - Backend Enums (match server contract)

/// Task lifecycle status — matches backend `HouseholdTaskStatus`.
public enum HouseholdTaskStatus: String, Codable, Sendable, CaseIterable {
    case pending
    case inProgress = "in_progress"
    case completed
    case cancelled

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
        case .inProgress: "arrow.right.circle"
        case .completed: "checkmark.circle.fill"
        case .cancelled: "xmark.circle"
        }
    }

    public var color: Color {
        switch self {
        case .pending: .secondary
        case .inProgress: .blue
        case .completed: .green
        case .cancelled: .gray
        }
    }

    public var isActive: Bool {
        self == .pending || self == .inProgress
    }
}

/// Task priority — matches backend `HouseholdTaskPriority`.
public enum HouseholdTaskPriority: String, Codable, Sendable, CaseIterable {
    case low
    case medium
    case high
    case urgent

    public var label: String {
        switch self {
        case .low: "Low"
        case .medium: "Medium"
        case .high: "High"
        case .urgent: "Urgent"
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

    public var sortOrder: Int {
        switch self {
        case .urgent: 0
        case .high: 1
        case .medium: 2
        case .low: 3
        }
    }
}

/// Recurrence rule — matches backend `RecurrenceRule`.
/// The backend `Custom { interval_days }` variant uses tagged enum serialization.
public enum RecurrenceRule: Codable, Sendable, Hashable {
    case daily
    case weekly
    case biweekly
    case monthly
    case custom(intervalDays: UInt32)

    public var label: String {
        switch self {
        case .daily: "Daily"
        case .weekly: "Weekly"
        case .biweekly: "Every 2 weeks"
        case .monthly: "Monthly"
        case .custom(let days): "Every \(days) days"
        }
    }

    // Custom Codable to handle Rust tagged enum serialization
    private enum CodingKeys: String, CodingKey {
        case daily, weekly, biweekly, monthly, custom
    }

    private struct CustomPayload: Codable {
        let intervalDays: UInt32

        enum CodingKeys: String, CodingKey {
            case intervalDays = "interval_days"
        }
    }

    public init(from decoder: Decoder) throws {
        // First try as a simple string
        if let container = try? decoder.singleValueContainer(),
           let value = try? container.decode(String.self) {
            switch value {
            case "daily": self = .daily
            case "weekly": self = .weekly
            case "biweekly": self = .biweekly
            case "monthly": self = .monthly
            default: self = .daily
            }
            return
        }

        // Then try as tagged enum object: { "custom": { "interval_days": N } }
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let payload = try container.decodeIfPresent(CustomPayload.self, forKey: .custom) {
            self = .custom(intervalDays: payload.intervalDays)
            return
        }

        self = .daily
    }

    public func encode(to encoder: Encoder) throws {
        switch self {
        case .daily:
            var container = encoder.singleValueContainer()
            try container.encode("daily")
        case .weekly:
            var container = encoder.singleValueContainer()
            try container.encode("weekly")
        case .biweekly:
            var container = encoder.singleValueContainer()
            try container.encode("biweekly")
        case .monthly:
            var container = encoder.singleValueContainer()
            try container.encode("monthly")
        case .custom(let days):
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(CustomPayload(intervalDays: days), forKey: .custom)
        }
    }
}

/// Household member role — matches backend `HouseholdRole`.
public enum HouseholdRole: String, Codable, Sendable {
    case owner
    case admin
    case member
}

// MARK: - UI-Only Enums

/// Category for grouping tasks in the UI. Not persisted to the backend.
/// Inferred from task title/description keywords.
public enum HouseholdTaskCategory: String, Sendable, CaseIterable, Identifiable {
    case chores
    case errands
    case meals
    case custom

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .chores: "Chores"
        case .errands: "Errands"
        case .meals: "Meals"
        case .custom: "Other"
        }
    }

    public var icon: String {
        switch self {
        case .chores: "bubbles.and.sparkles"
        case .errands: "car.fill"
        case .meals: "fork.knife"
        case .custom: "square.grid.2x2"
        }
    }

    public var color: Color {
        switch self {
        case .chores: .blue
        case .errands: .orange
        case .meals: .green
        case .custom: .purple
        }
    }

    /// Infer category from task title and description keywords.
    public static func infer(title: String, description: String?) -> HouseholdTaskCategory {
        let text = "\(title) \(description ?? "")".lowercased()

        let choreWords = ["clean", "vacuum", "mop", "sweep", "dust", "laundry", "wash", "dishes",
                          "dishwasher", "trash", "garbage", "recycle", "tidy", "organize", "scrub",
                          "iron", "fold", "bathroom", "mow", "lawn", "garden", "weed", "rake"]
        if choreWords.contains(where: { text.contains($0) }) { return .chores }

        let errandWords = ["pick up", "drop off", "return", "buy", "grocery", "store", "shop",
                           "pharmacy", "post office", "bank", "appointment", "vet", "doctor",
                           "dentist", "dry clean", "gas", "fill up", "mail", "deliver", "package"]
        if errandWords.contains(where: { text.contains($0) }) { return .errands }

        let mealWords = ["cook", "dinner", "lunch", "breakfast", "meal", "prep", "recipe",
                         "bake", "grill", "stir fry", "roast", "marinate", "defrost", "thaw",
                         "snack", "food", "kitchen", "oven"]
        if mealWords.contains(where: { text.contains($0) }) { return .meals }

        return .custom
    }
}

// MARK: - Models

/// A household member — matches backend `HouseholdMember`.
public struct HouseholdMember: Identifiable, Hashable, Codable, Sendable {
    public let id: UUID
    public var householdId: UUID
    public var userId: String
    public var displayName: String
    public var role: HouseholdRole
    public var joinedAt: Date

    public init(
        id: UUID = UUID(),
        householdId: UUID = UUID(),
        userId: String,
        displayName: String,
        role: HouseholdRole = .member,
        joinedAt: Date = Date()
    ) {
        self.id = id
        self.householdId = householdId
        self.userId = userId
        self.displayName = displayName
        self.role = role
        self.joinedAt = joinedAt
    }

    enum CodingKeys: String, CodingKey {
        case id, householdId, userId, displayName, role, joinedAt
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UUID.self, forKey: .id)
        householdId = try container.decode(UUID.self, forKey: .householdId)
        userId = try container.decode(String.self, forKey: .userId)
        displayName = try container.decode(String.self, forKey: .displayName)
        role = try container.decode(HouseholdRole.self, forKey: .role)
        joinedAt = try container.decode(Date.self, forKey: .joinedAt)
    }

    /// Emoji derived from role for display.
    public var emoji: String {
        switch role {
        case .owner: "👑"
        case .admin: "⭐"
        case .member: "👤"
        }
    }

    public static func decodeArray(from data: Data) throws -> [HouseholdMember] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode([HouseholdMember].self, from: data)
    }
}

/// A household — matches backend `Household`.
public struct Household: Identifiable, Hashable, Codable, Sendable {
    public let id: UUID
    public var name: String
    public var createdBy: String
    public var createdAt: Date
    public var updatedAt: Date

    public init(
        id: UUID = UUID(),
        name: String,
        createdBy: String = "",
        createdAt: Date = Date(),
        updatedAt: Date = Date()
    ) {
        self.id = id
        self.name = name
        self.createdBy = createdBy
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    enum CodingKeys: String, CodingKey {
        case id, name, createdBy, createdAt, updatedAt
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UUID.self, forKey: .id)
        name = try container.decode(String.self, forKey: .name)
        createdBy = try container.decode(String.self, forKey: .createdBy)
        createdAt = try container.decode(Date.self, forKey: .createdAt)
        updatedAt = try container.decode(Date.self, forKey: .updatedAt)
    }

    public static func decode(from data: Data) throws -> Household {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(Household.self, from: data)
    }

    public static func decodeArray(from data: Data) throws -> [Household] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode([Household].self, from: data)
    }
}

/// A task in a shared household task list — matches backend `HouseholdTask`.
public struct HouseholdTask: Identifiable, Hashable, Sendable {
    public let id: UUID
    public var householdId: UUID
    public var title: String
    public var description: String?
    public var status: HouseholdTaskStatus
    public var priority: HouseholdTaskPriority
    public var assignedTo: String?
    public var dueDate: Date?
    public var recurrence: RecurrenceRule?
    public var createdBy: String
    public var createdAt: Date
    public var updatedAt: Date
    public var completedAt: Date?

    public init(
        id: UUID = UUID(),
        householdId: UUID = UUID(),
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

    /// Whether the due date has passed.
    public var isOverdue: Bool {
        guard let due = dueDate else { return false }
        return due < Date() && status.isActive
    }

    /// Whether this task is done.
    public var isCompleted: Bool {
        status == .completed
    }

    /// UI-only category inferred from title and description.
    public var category: HouseholdTaskCategory {
        HouseholdTaskCategory.infer(title: title, description: description)
    }

    /// Display name for the assignee — resolves user_id to member display name.
    public func assigneeName(in members: [HouseholdMember]) -> String? {
        guard let assignedTo else { return nil }
        return members.first(where: { $0.userId == assignedTo })?.displayName ?? assignedTo
    }
}

// MARK: - Codable

extension HouseholdTask: Codable {
    enum CodingKeys: String, CodingKey {
        case id, householdId, title, description, status, priority
        case assignedTo, dueDate, recurrence, createdBy, createdAt, updatedAt, completedAt
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UUID.self, forKey: .id)
        householdId = try container.decode(UUID.self, forKey: .householdId)
        title = try container.decode(String.self, forKey: .title)
        description = try container.decodeIfPresent(String.self, forKey: .description)
        status = try container.decode(HouseholdTaskStatus.self, forKey: .status)
        priority = try container.decodeIfPresent(HouseholdTaskPriority.self, forKey: .priority) ?? .medium
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

extension HouseholdMember {
    public static let sampleHouseholdId = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!

    public static let samples: [HouseholdMember] = [
        HouseholdMember(householdId: sampleHouseholdId, userId: "mom", displayName: "Mom", role: .owner),
        HouseholdMember(householdId: sampleHouseholdId, userId: "dad", displayName: "Dad", role: .admin),
        HouseholdMember(householdId: sampleHouseholdId, userId: "emma", displayName: "Emma", role: .member),
        HouseholdMember(householdId: sampleHouseholdId, userId: "jake", displayName: "Jake", role: .member),
    ]
}

extension HouseholdTask {
    public static let samples: [HouseholdTask] = {
        let hid = HouseholdMember.sampleHouseholdId

        return [
            HouseholdTask(
                householdId: hid, title: "Vacuum living room and hallway",
                description: "Don't forget under the couch cushions",
                assignedTo: "emma",
                dueDate: Calendar.current.date(byAdding: .hour, value: 4, to: Date()),
                recurrence: .weekly, createdBy: "mom"
            ),
            HouseholdTask(
                householdId: hid, title: "Unload dishwasher",
                assignedTo: "jake",
                dueDate: Calendar.current.date(byAdding: .hour, value: 2, to: Date()),
                recurrence: .daily, createdBy: "mom"
            ),
            HouseholdTask(
                householdId: hid, title: "Pick up dry cleaning",
                description: "The blue suit and two shirts — receipt is on the fridge",
                assignedTo: "dad",
                dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
                createdBy: "mom"
            ),
            HouseholdTask(
                householdId: hid, title: "Grocery run — Whole Foods",
                description: "Milk, eggs, sourdough, chicken thighs, broccoli, strawberries",
                assignedTo: "mom",
                dueDate: Calendar.current.date(byAdding: .hour, value: 6, to: Date()),
                createdBy: "mom"
            ),
            HouseholdTask(
                householdId: hid, title: "Prep lunches for the week",
                description: "Turkey wraps for kids, salads for adults. Prep containers in bottom drawer.",
                assignedTo: "mom",
                dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
                recurrence: .weekly, createdBy: "mom"
            ),
            HouseholdTask(
                householdId: hid, title: "Dinner — chicken stir fry",
                description: "Recipe pinned on the fridge. Rice in the instant pot.",
                assignedTo: "dad",
                dueDate: Calendar.current.date(bySettingHour: 18, minute: 0, second: 0, of: Date()),
                createdBy: "mom"
            ),
            HouseholdTask(
                householdId: hid, title: "Take dog to the vet",
                description: "Annual checkup — appointment at 2pm. Bring vaccination records.",
                assignedTo: "mom",
                dueDate: Calendar.current.date(byAdding: .day, value: 3, to: Date()),
                createdBy: "dad"
            ),
            HouseholdTask(
                householdId: hid, title: "Clean bathrooms",
                description: "Both upstairs bathrooms. New cleaning supplies under the kitchen sink.",
                assignedTo: "emma",
                dueDate: Calendar.current.date(byAdding: .day, value: 2, to: Date()),
                recurrence: .weekly, createdBy: "mom"
            ),
            HouseholdTask(
                householdId: hid, title: "Mow the lawn",
                status: .completed, assignedTo: "jake",
                dueDate: Calendar.current.date(byAdding: .day, value: 2, to: Date()),
                recurrence: .weekly, createdBy: "dad",
                completedAt: Calendar.current.date(byAdding: .hour, value: -3, to: Date())
            ),
            HouseholdTask(
                householdId: hid, title: "Return Amazon package",
                description: "Wrong size shoes — label printed, box by the front door",
                status: .completed, createdBy: "mom",
                completedAt: Calendar.current.date(byAdding: .hour, value: -5, to: Date())
            ),
            HouseholdTask(
                householdId: hid, title: "Schedule piano tuning",
                description: "Call Music Masters — (555) 123-4567",
                priority: .low, createdBy: "mom"
            ),
        ]
    }()
}
