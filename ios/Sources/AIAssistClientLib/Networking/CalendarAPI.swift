import Foundation

/// REST API client for fetching calendar events from the server.
public final class CalendarAPI: @unchecked Sendable {
    public let host: String
    public let port: Int

    private var baseURLString: String { "http://\(host):\(port)" }

    private var decoder: JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        d.dateDecodingStrategy = .iso8601
        return d
    }

    public init(
        host: String = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost",
        port: Int = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
    ) {
        self.host = host
        self.port = port
    }

    /// Fetch events for a single date.
    public func fetchEvents(date: String) async throws -> [CalendarEvent] {
        guard let url = URL(string: "\(baseURLString)/api/calendar/events?date=\(date)") else {
            throw URLError(.badURL)
        }
        let (data, response) = try await URLSession.shared.data(from: url)
        guard let http = response as? HTTPURLResponse else {
            throw URLError(.badServerResponse)
        }
        if http.statusCode == 401 {
            throw CalendarAPIError.notConnected
        }
        guard http.statusCode == 200 else {
            throw URLError(.badServerResponse)
        }
        let eventsResponse = try decoder.decode(CalendarEventsResponse.self, from: data)
        return eventsResponse.events
    }

    /// Fetch the calendar connection status.
    public func fetchStatus() async throws -> CalendarStatus {
        guard let url = URL(string: "\(baseURLString)/api/calendar/status") else {
            throw URLError(.badURL)
        }
        let (data, response) = try await URLSession.shared.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw URLError(.badServerResponse)
        }
        return try decoder.decode(CalendarStatus.self, from: data)
    }

    /// Disconnect the calendar.
    public func disconnect() async throws {
        guard let url = URL(string: "\(baseURLString)/api/calendar/connection") else {
            throw URLError(.badURL)
        }
        var request = URLRequest(url: url)
        request.httpMethod = "DELETE"
        let (_, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw URLError(.badServerResponse)
        }
    }
}

/// Calendar connection status from the server.
public struct CalendarStatus: Codable, Sendable {
    public let available: Bool
    public let connected: Bool
    public let email: String?
}

/// Calendar-specific API errors.
public enum CalendarAPIError: Error {
    case notConnected
}
