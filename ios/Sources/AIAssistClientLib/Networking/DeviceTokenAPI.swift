import Foundation

/// REST API client for registering and unregistering device tokens with the server.
public final class DeviceTokenAPI: @unchecked Sendable {
    private let host: String
    private let port: Int

    private var baseURLString: String { "http://\(host):\(port)" }

    public init(
        host: String = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost",
        port: Int = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
    ) {
        self.host = host
        self.port = port
    }

    /// Register a device token for push notifications.
    public func register(token: String) async throws {
        guard let url = URL(string: "\(baseURLString)/api/device-tokens") else {
            throw URLError(.badURL)
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        let body: [String: String] = [
            "token": token,
            "platform": "ios"
        ]
        request.httpBody = try JSONEncoder().encode(body)

        let (_, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw URLError(.badServerResponse)
        }
    }

    /// Unregister a device token.
    public func unregister(token: String) async throws {
        guard let url = URL(string: "\(baseURLString)/api/device-tokens/\(token)") else {
            throw URLError(.badURL)
        }
        var request = URLRequest(url: url)
        request.httpMethod = "DELETE"

        let (_, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw URLError(.badServerResponse)
        }
    }
}
