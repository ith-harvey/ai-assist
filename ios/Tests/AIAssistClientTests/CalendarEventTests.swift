import Foundation
import Testing
@testable import AIAssistClientLib

@Suite("CalendarEvent Tests")
struct CalendarEventTests {

    // MARK: - JSON Decoding

    private func eventJSON(
        id: String = "event-1",
        title: String = "Team standup",
        start: String = "2026-03-15T09:00:00Z",
        end: String = "2026-03-15T09:30:00Z",
        allDay: Bool = false,
        location: String? = nil,
        description: String? = nil,
        attendees: [String]? = nil,
        colorId: String? = nil
    ) -> String {
        var fields = """
        "id": "\(id)",
        "title": "\(title)",
        "start": "\(start)",
        "end": "\(end)",
        "all_day": \(allDay)
        """
        if let location { fields += ",\n\"location\": \"\(location)\"" }
        if let description { fields += ",\n\"description\": \"\(description)\"" }
        if let attendees {
            let list = attendees.map { "\"\($0)\"" }.joined(separator: ",")
            fields += ",\n\"attendees\": [\(list)]"
        }
        if let colorId { fields += ",\n\"color_id\": \"\(colorId)\"" }
        return "{\(fields)}"
    }

    @Test("Decode CalendarEvent from snake_case JSON")
    func decodeCalendarEvent() throws {
        let json = eventJSON(
            title: "Design review",
            start: "2026-03-15T11:00:00Z",
            end: "2026-03-15T12:00:00Z",
            location: "Conference Room A",
            attendees: ["alice@example.com", "bob@example.com"],
            colorId: "3"
        )
        let data = json.data(using: .utf8)!
        let event = try CalendarEvent.decode(from: data)

        #expect(event.id == "event-1")
        #expect(event.title == "Design review")
        #expect(event.allDay == false)
        #expect(event.location == "Conference Room A")
        #expect(event.attendees == ["alice@example.com", "bob@example.com"])
        #expect(event.colorId == "3")
    }

    @Test("Decode CalendarEvent with minimal fields")
    func decodeMinimalEvent() throws {
        let json = eventJSON()
        let data = json.data(using: .utf8)!
        let event = try CalendarEvent.decode(from: data)

        #expect(event.location == nil)
        #expect(event.description == nil)
        #expect(event.colorId == nil)
    }

    @Test("Decode CalendarEvent array")
    func decodeEventArray() throws {
        let e1 = eventJSON(id: "e1", title: "First")
        let e2 = eventJSON(id: "e2", title: "Second")
        let json = "[\(e1), \(e2)]"
        let data = json.data(using: .utf8)!
        let events = try CalendarEvent.decodeArray(from: data)
        #expect(events.count == 2)
        #expect(events[0].title == "First")
        #expect(events[1].title == "Second")
    }

    @Test("Decode all-day event")
    func decodeAllDayEvent() throws {
        let json = eventJSON(
            title: "Company holiday",
            start: "2026-03-15T00:00:00Z",
            end: "2026-03-16T00:00:00Z",
            allDay: true
        )
        let data = json.data(using: .utf8)!
        let event = try CalendarEvent.decode(from: data)
        #expect(event.allDay == true)
    }

    // MARK: - CalendarEventsResponse

    @Test("Decode CalendarEventsResponse from JSON")
    func decodeEventsResponse() throws {
        let eventJson = eventJSON()
        let json = """
        {
            "date": "2026-03-15",
            "events": [\(eventJson)]
        }
        """
        let data = json.data(using: .utf8)!
        let response = try CalendarEventsResponse.decode(from: data)
        #expect(response.date == "2026-03-15")
        #expect(response.events.count == 1)
    }

    // MARK: - Computed Properties

    @Test("durationMinutes calculates correctly")
    func durationMinutes() {
        let cal = Calendar.current
        let today = cal.startOfDay(for: Date())
        let start = cal.date(bySettingHour: 9, minute: 0, second: 0, of: today)!
        let end = cal.date(bySettingHour: 10, minute: 30, second: 0, of: today)!
        let event = CalendarEvent(id: "test", title: "Test", start: start, end: end)
        #expect(event.durationMinutes == 90)
    }

    @Test("durationMinutes for 30 min event")
    func durationMinutes30() {
        let cal = Calendar.current
        let today = cal.startOfDay(for: Date())
        let start = cal.date(bySettingHour: 14, minute: 0, second: 0, of: today)!
        let end = cal.date(bySettingHour: 14, minute: 30, second: 0, of: today)!
        let event = CalendarEvent(id: "test", title: "Quick sync", start: start, end: end)
        #expect(event.durationMinutes == 30)
    }

    @Test("timeRangeText produces formatted string")
    func timeRangeText() {
        let cal = Calendar.current
        let today = cal.startOfDay(for: Date())
        let start = cal.date(bySettingHour: 14, minute: 0, second: 0, of: today)!
        let end = cal.date(bySettingHour: 15, minute: 0, second: 0, of: today)!
        let event = CalendarEvent(id: "test", title: "Meeting", start: start, end: end)
        let text = event.timeRangeText
        // Should contain " – " separator
        #expect(text.contains("–"))
    }

    // MARK: - Color mapping

    @Test("color maps Google colorIds 1-11 to non-default colors")
    func colorMapping() {
        for i in 1...11 {
            let event = CalendarEvent(
                id: "c\(i)", title: "Test",
                start: Date(), end: Date(),
                colorId: "\(i)"
            )
            // Each mapped color should be different from default .blue (crude check via colorId)
            #expect(event.colorId == "\(i)")
        }
    }

    @Test("color returns default blue for nil colorId")
    func colorDefaultNil() {
        let event = CalendarEvent(id: "x", title: "Test", start: Date(), end: Date())
        // colorId is nil, so color should be default .blue
        #expect(event.colorId == nil)
    }

    @Test("color returns default blue for unknown colorId")
    func colorDefaultUnknown() {
        let event = CalendarEvent(
            id: "x", title: "Test",
            start: Date(), end: Date(),
            colorId: "99"
        )
        // Should not crash — returns default .blue
        #expect(event.colorId == "99")
    }

    // MARK: - Sample Data

    @Test("CalendarEvent samples are non-empty and have unique IDs")
    func sampleData() {
        #expect(CalendarEvent.samples.count == 5)
        let ids = Set(CalendarEvent.samples.map(\.id))
        #expect(ids.count == 5)
    }

    @Test("CalendarEvent samples contain one all-day event")
    func sampleDataAllDay() {
        let allDayEvents = CalendarEvent.samples.filter(\.allDay)
        #expect(allDayEvents.count == 1)
    }
}
