import Foundation
import SwiftUI

// MARK: - Household Member

/// A household member whose calendar events can be displayed.
public struct HouseholdMember: Identifiable, Codable, Sendable, Hashable {
    public let id: String
    public var name: String
    public var colorHex: String?

    public init(id: String, name: String, colorHex: String? = nil) {
        self.id = id
        self.name = name
        self.colorHex = colorHex
    }

    /// Assigned color for this member. Falls back to a palette based on hash.
    public var color: Color {
        if let hex = colorHex {
            return Color(hex: hex)
        }
        let palette: [Color] = [
            .blue, .purple, .orange, .green, .pink, .teal, .indigo, .mint
        ]
        let index = abs(id.hashValue) % palette.count
        return palette[index]
    }
}

// MARK: - Reminder Option

/// Reminder intervals for event creation.
public enum ReminderOption: Int, CaseIterable, Identifiable, Sendable {
    case none = 0
    case fiveMinutes = 5
    case fifteenMinutes = 15
    case thirtyMinutes = 30
    case oneHour = 60
    case oneDay = 1440

    public var id: Int { rawValue }

    public var label: String {
        switch self {
        case .none: return "None"
        case .fiveMinutes: return "5 minutes before"
        case .fifteenMinutes: return "15 minutes before"
        case .thirtyMinutes: return "30 minutes before"
        case .oneHour: return "1 hour before"
        case .oneDay: return "1 day before"
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
    public var memberId: String?
    public var memberName: String?
    public var reminderMinutes: Int?

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
        memberId: String? = nil,
        memberName: String? = nil,
        reminderMinutes: Int? = nil
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
        self.memberId = memberId
        self.memberName = memberName
        self.reminderMinutes = reminderMinutes
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

    /// Color based on member assignment, falling back to Google Calendar color.
    public func memberColor(members: [HouseholdMember]) -> Color {
        guard let memberId else { return color }
        return members.first(where: { $0.id == memberId })?.color ?? color
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

// MARK: - Color Hex Extension

extension Color {
    /// Create a Color from a hex string (e.g., "#FF5733" or "FF5733").
    init(hex: String) {
        let hex = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        var rgbValue: UInt64 = 0
        Scanner(string: hex).scanHexInt64(&rgbValue)
        let r = Double((rgbValue & 0xFF0000) >> 16) / 255.0
        let g = Double((rgbValue & 0x00FF00) >> 8) / 255.0
        let b = Double(rgbValue & 0x0000FF) / 255.0
        self.init(red: r, green: g, blue: b)
    }
}

// MARK: - Sample Data

extension HouseholdMember {
    /// Sample household members for previews.
    public static let samples: [HouseholdMember] = [
        HouseholdMember(id: "mom", name: "Mom", colorHex: "#5B7FE8"),
        HouseholdMember(id: "dad", name: "Dad", colorHex: "#2EAD6B"),
        HouseholdMember(id: "emma", name: "Emma", colorHex: "#E87B5B"),
    ]
}

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
                memberId: "mom",
                memberName: "Mom"
            ),
            CalendarEvent(
                id: "sample-2",
                title: "Design review",
                start: cal.date(bySettingHour: 11, minute: 0, second: 0, of: today)!,
                end: cal.date(bySettingHour: 12, minute: 0, second: 0, of: today)!,
                colorId: "3",
                memberId: "dad",
                memberName: "Dad"
            ),
            CalendarEvent(
                id: "sample-3",
                title: "Lunch with Christina",
                start: cal.date(bySettingHour: 12, minute: 30, second: 0, of: today)!,
                end: cal.date(bySettingHour: 13, minute: 30, second: 0, of: today)!,
                location: "The Usual Spot",
                colorId: "5",
                memberId: "mom",
                memberName: "Mom"
            ),
            CalendarEvent(
                id: "sample-4",
                title: "Sprint planning",
                start: cal.date(bySettingHour: 14, minute: 0, second: 0, of: today)!,
                end: cal.date(bySettingHour: 15, minute: 0, second: 0, of: today)!,
                colorId: "7",
                memberId: "emma",
                memberName: "Emma"
            ),
            CalendarEvent(
                id: "sample-5",
                title: "Company all-hands",
                start: today,
                end: cal.date(byAdding: .day, value: 1, to: today)!,
                allDay: true,
                colorId: "11"
            ),
        ]
    }()
}
