import Foundation
import SwiftUI

// MARK: - Enums

/// Category of a household task.
public enum HouseholdTaskCategory: String, Codable, Sendable, CaseIterable, Identifiable {
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
}

/// Recurrence rule for repeating tasks.
public enum RecurrenceRule: String, Codable, Sendable, CaseIterable, Identifiable {
    case daily
    case weekly
    case biweekly
    case monthly
    case custom

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .daily: "Daily"
        case .weekly: "Weekly"
        case .biweekly: "Every 2 weeks"
        case .monthly: "Monthly"
        case .custom: "Custom"
        }
    }
}

/// A household member who can be assigned tasks.
public struct HouseholdMember: Identifiable, Hashable, Codable, Sendable {
    public let id: UUID
    public var name: String
    public var emoji: String

    public init(id: UUID = UUID(), name: String, emoji: String = "👤") {
        self.id = id
        self.name = name
        self.emoji = emoji
    }
}

// MARK: - Model

/// A task in a shared household task list.
public struct HouseholdTask: Identifiable, Hashable, Sendable {
    public let id: UUID
    public var title: String
    public var notes: String?
    public var category: HouseholdTaskCategory
    public var assigneeId: UUID?
    public var assigneeName: String?
    public var dueDate: Date?
    public var recurrence: RecurrenceRule?
    public var isCompleted: Bool
    public var completedAt: Date?
    public var createdAt: Date
    public var updatedAt: Date

    public init(
        id: UUID = UUID(),
        title: String,
        notes: String? = nil,
        category: HouseholdTaskCategory = .custom,
        assigneeId: UUID? = nil,
        assigneeName: String? = nil,
        dueDate: Date? = nil,
        recurrence: RecurrenceRule? = nil,
        isCompleted: Bool = false,
        completedAt: Date? = nil,
        createdAt: Date = Date(),
        updatedAt: Date = Date()
    ) {
        self.id = id
        self.title = title
        self.notes = notes
        self.category = category
        self.assigneeId = assigneeId
        self.assigneeName = assigneeName
        self.dueDate = dueDate
        self.recurrence = recurrence
        self.isCompleted = isCompleted
        self.completedAt = completedAt
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    /// Whether the due date has passed.
    public var isOverdue: Bool {
        guard let due = dueDate else { return false }
        return due < Date() && !isCompleted
    }
}

// MARK: - Codable

extension HouseholdTask: Codable {
    enum CodingKeys: String, CodingKey {
        case id, title, notes, category, assigneeId, assigneeName
        case dueDate, recurrence, isCompleted, completedAt, createdAt, updatedAt
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UUID.self, forKey: .id)
        title = try container.decode(String.self, forKey: .title)
        notes = try container.decodeIfPresent(String.self, forKey: .notes)
        category = try container.decode(HouseholdTaskCategory.self, forKey: .category)
        assigneeId = try container.decodeIfPresent(UUID.self, forKey: .assigneeId)
        assigneeName = try container.decodeIfPresent(String.self, forKey: .assigneeName)
        dueDate = try container.decodeIfPresent(Date.self, forKey: .dueDate)
        recurrence = try container.decodeIfPresent(RecurrenceRule.self, forKey: .recurrence)
        isCompleted = try container.decodeIfPresent(Bool.self, forKey: .isCompleted) ?? false
        completedAt = try container.decodeIfPresent(Date.self, forKey: .completedAt)
        createdAt = try container.decode(Date.self, forKey: .createdAt)
        updatedAt = try container.decode(Date.self, forKey: .updatedAt)
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
    public static let samples: [HouseholdMember] = [
        HouseholdMember(name: "Mom", emoji: "👩"),
        HouseholdMember(name: "Dad", emoji: "👨"),
        HouseholdMember(name: "Emma", emoji: "👧"),
        HouseholdMember(name: "Jake", emoji: "👦"),
    ]
}

extension HouseholdTask {
    public static let samples: [HouseholdTask] = {
        let members = HouseholdMember.samples
        let mom = members[0]
        let dad = members[1]
        let emma = members[2]
        let jake = members[3]

        return [
            HouseholdTask(
                title: "Vacuum living room and hallway",
                notes: "Don't forget under the couch cushions",
                category: .chores,
                assigneeId: emma.id,
                assigneeName: emma.name,
                dueDate: Calendar.current.date(byAdding: .hour, value: 4, to: Date()),
                recurrence: .weekly
            ),
            HouseholdTask(
                title: "Unload dishwasher",
                category: .chores,
                assigneeId: jake.id,
                assigneeName: jake.name,
                dueDate: Calendar.current.date(byAdding: .hour, value: 2, to: Date()),
                recurrence: .daily
            ),
            HouseholdTask(
                title: "Pick up dry cleaning",
                notes: "The blue suit and two shirts — receipt is on the fridge",
                category: .errands,
                assigneeId: dad.id,
                assigneeName: dad.name,
                dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date())
            ),
            HouseholdTask(
                title: "Grocery run — Whole Foods",
                notes: "Milk, eggs, sourdough, chicken thighs, broccoli, strawberries",
                category: .errands,
                assigneeId: mom.id,
                assigneeName: mom.name,
                dueDate: Calendar.current.date(byAdding: .hour, value: 6, to: Date())
            ),
            HouseholdTask(
                title: "Prep lunches for the week",
                notes: "Turkey wraps for kids, salads for adults. Prep containers in bottom drawer.",
                category: .meals,
                assigneeId: mom.id,
                assigneeName: mom.name,
                dueDate: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
                recurrence: .weekly
            ),
            HouseholdTask(
                title: "Dinner — chicken stir fry",
                notes: "Recipe pinned on the fridge. Rice in the instant pot.",
                category: .meals,
                assigneeId: dad.id,
                assigneeName: dad.name,
                dueDate: Calendar.current.date(bySettingHour: 18, minute: 0, second: 0, of: Date())
            ),
            HouseholdTask(
                title: "Take dog to the vet",
                notes: "Annual checkup — appointment at 2pm. Bring vaccination records.",
                category: .errands,
                assigneeId: mom.id,
                assigneeName: mom.name,
                dueDate: Calendar.current.date(byAdding: .day, value: 3, to: Date())
            ),
            HouseholdTask(
                title: "Clean bathrooms",
                notes: "Both upstairs bathrooms. New cleaning supplies under the kitchen sink.",
                category: .chores,
                assigneeId: emma.id,
                assigneeName: emma.name,
                dueDate: Calendar.current.date(byAdding: .day, value: 2, to: Date()),
                recurrence: .weekly
            ),
            HouseholdTask(
                title: "Mow the lawn",
                category: .chores,
                assigneeId: jake.id,
                assigneeName: jake.name,
                dueDate: Calendar.current.date(byAdding: .day, value: 2, to: Date()),
                recurrence: .weekly,
                isCompleted: true,
                completedAt: Calendar.current.date(byAdding: .hour, value: -3, to: Date())
            ),
            HouseholdTask(
                title: "Return Amazon package",
                notes: "Wrong size shoes — label printed, box by the front door",
                category: .errands,
                isCompleted: true,
                completedAt: Calendar.current.date(byAdding: .hour, value: -5, to: Date())
            ),
            HouseholdTask(
                title: "Schedule piano tuning",
                notes: "Call Music Masters — (555) 123-4567",
                category: .custom
            ),
        ]
    }()
}
