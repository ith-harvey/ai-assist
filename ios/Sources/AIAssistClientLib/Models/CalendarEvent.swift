import Foundation
import SwiftUI

// MARK: - Household Member

/// A household member who can be assigned to calendar events.
public struct HouseholdMember: Identifiable, Codable, Sendable, Hashable {
    public let id: String
    public var name: String
    public var emoji: String

    public init(id: String, name: String, emoji: String) {
        self.id = id
        self.name = name
        self.emoji = emoji
    }

    /// Color for this member based on a stable DJB2 hash of their ID.
    /// Uses a deterministic hash (not Swift's randomized `hashValue`) so
    /// colors stay consistent across app launches.
    public var color: Color {
        let colors: [Color] = [
            Color(red: 0.26, green: 0.52, blue: 0.96), // Blue
            Color(red: 0.91, green: 0.42, blue: 0.48), // Pink
            Color(red: 0.13, green: 0.67, blue: 0.53), // Green
            Color(red: 0.94, green: 0.60, blue: 0.22), // Orange
            Color(red: 0.54, green: 0.33, blue: 0.71), // Purple
            Color(red: 0.02, green: 0.65, blue: 0.72), // Teal
        ]
        var hash: UInt64 = 5381
        for byte in id.utf8 {
            hash = hash &* 33 &+ UInt64(byte)
        }
        return colors[Int(hash % UInt64(colors.count))]
    }

    /// Sample members for previews.
    public static let samples: [HouseholdMember] = [
        HouseholdMember(id: "mom-1", name: "Mom", emoji: "👩"),
        HouseholdMember(id: "dad-1", name: "Dad", emoji: "👨"),
        HouseholdMember(id: "emma-1", name: "Emma", emoji: "👧"),
        HouseholdMember(id: "jack-1", name: "Jack", emoji: "👦"),
    ]
}

// MARK: - Recurrence

/// Recurrence pattern for calendar events.
public enum RecurrenceRule: String, Codable, Sendable, CaseIterable {
    case none
    case daily
    case weekly
    case biweekly
    case monthly
    case yearly

    public var displayName: String {
        switch self {
        case .none: return "Does not repeat"
        case .daily: return "Daily"
        case .weekly: return "Weekly"
        case .biweekly: return "Every 2 weeks"
        case .monthly: return "Monthly"
        case .yearly: return "Yearly"
        }
    }
}

// MARK: - Model

/// A Google Calendar event. Mirrors the server's `CalendarEvent` struct.
public struct CalendarEvent: Identifiable, Codable, Sendable {
    public let id: String
    public var title: String
    public var start: Date
    public var end: Date
    public var allDay: Bool
    public var location: String?
    public var description: String?
    public var attendees: [String]
    public var colorId: String?
    public var householdMemberId: String?
    public var recurrence: RecurrenceRule?

    public init(
        id: String,
        title: String,
        start: Date,
        end: Date,
        allDay: Bool = false,
        location: String? = nil,
        description: String? = nil,
        attendees: [String] = [],
        colorId: String? = nil,
        householdMemberId: String? = nil,
        recurrence: RecurrenceRule? = nil
    ) {
        self.id = id
        self.title = title
        self.start = start
        self.end = end
        self.allDay = allDay
        self.location = location
        self.description = description
        self.attendees = attendees
        self.colorId = colorId
        self.householdMemberId = householdMemberId
        self.recurrence = recurrence
    }

    /// Google Calendar color ID → SwiftUI Color.
    /// Maps Google's colorId values (1-11) to colors.
    public var color: Color {
        switch colorId {
        case "1": return Color(red: 0.47, green: 0.53, blue: 0.87) // Lavender
        case "2": return Color(red: 0.13, green: 0.67, blue: 0.53) // Sage
        case "3": return Color(red: 0.54, green: 0.33, blue: 0.71) // Grape
        case "4": return Color(red: 0.91, green: 0.42, blue: 0.48) // Flamingo
        case "5": return Color(red: 0.94, green: 0.72, blue: 0.20) // Banana
        case "6": return Color(red: 0.94, green: 0.60, blue: 0.22) // Tangerine
        case "7": return Color(red: 0.02, green: 0.65, blue: 0.72) // Peacock
        case "8": return Color(red: 0.38, green: 0.38, blue: 0.38) // Graphite
        case "9": return Color(red: 0.26, green: 0.52, blue: 0.96) // Blueberry
        case "10": return Color(red: 0.05, green: 0.55, blue: 0.31) // Basil
        case "11": return Color(red: 0.86, green: 0.27, blue: 0.22) // Tomato
        default: return .blue // Default calendar color
        }
    }

    /// Resolve color from household member if available, otherwise fall back to Google colorId.
    public func memberColor(members: [HouseholdMember]) -> Color {
        if let memberId = householdMemberId,
           let member = members.first(where: { $0.id == memberId }) {
            return member.color
        }
        return color
    }

    /// Duration in minutes.
    public var durationMinutes: Int {
        Int(end.timeIntervalSince(start) / 60)
    }

    /// Formatted time range string (e.g., "2:00 – 3:00 PM").
    public var timeRangeText: String {
        let formatter = DateFormatter()
        formatter.dateFormat = "h:mm a"
        return "\(formatter.string(from: start)) – \(formatter.string(from: end))"
    }
}

// MARK: - Codable

extension CalendarEvent {
    /// Decode from snake_case JSON with ISO-8601 dates.
    public static func decode(from data: Data) throws -> CalendarEvent {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(CalendarEvent.self, from: data)
    }

    /// Decode an array from snake_case JSON.
    public static func decodeArray(from data: Data) throws -> [CalendarEvent] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode([CalendarEvent].self, from: data)
    }
}

// MARK: - API Response

/// Response from GET /api/calendar/events.
public struct CalendarEventsResponse: Codable, Sendable {
    public let date: String
    public let events: [CalendarEvent]

    public static func decode(from data: Data) throws -> CalendarEventsResponse {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(CalendarEventsResponse.self, from: data)
    }
}

// MARK: - Sample Data

extension CalendarEvent {
    /// Sample events for previews.
    public static let samples: [CalendarEvent] = {
        let cal = Calendar.current
        let today = cal.startOfDay(for: Date())

        return [
            CalendarEvent(
                id: "sample-1",
                title: "Team standup",
                start: cal.date(bySettingHour: 9, minute: 0, second: 0, of: today)!,
                end: cal.date(bySettingHour: 9, minute: 30, second: 0, of: today)!,
                location: "Zoom",
                attendees: ["alice@example.com", "bob@example.com"],
                colorId: "9",
                householdMemberId: "mom-1",
                recurrence: .weekly
            ),
            CalendarEvent(
                id: "sample-2",
                title: "Design review",
                start: cal.date(bySettingHour: 11, minute: 0, second: 0, of: today)!,
                end: cal.date(bySettingHour: 12, minute: 0, second: 0, of: today)!,
                colorId: "3",
                householdMemberId: "dad-1"
            ),
            CalendarEvent(
                id: "sample-3",
                title: "Lunch with Christina",
                start: cal.date(bySettingHour: 12, minute: 30, second: 0, of: today)!,
                end: cal.date(bySettingHour: 13, minute: 30, second: 0, of: today)!,
                location: "The Usual Spot",
                colorId: "5",
                householdMemberId: "mom-1"
            ),
            CalendarEvent(
                id: "sample-4",
                title: "Soccer practice",
                start: cal.date(bySettingHour: 14, minute: 0, second: 0, of: today)!,
                end: cal.date(bySettingHour: 15, minute: 0, second: 0, of: today)!,
                colorId: "7",
                householdMemberId: "emma-1",
                recurrence: .biweekly
            ),
            CalendarEvent(
                id: "sample-5",
                title: "Family game night",
                start: today,
                end: cal.date(byAdding: .day, value: 1, to: today)!,
                allDay: true,
                colorId: "11"
            ),
        ]
    }()
}
