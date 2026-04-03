import Foundation

/// REST API client for registering and unregistering device tokens with the server.
public final class DeviceTokenAPI: Sendable {
    private let config: ServerConfig

    public init(config: ServerConfig = ServerConfig()) {
        self.config = config
    }

    /// Register a device token for push notifications.
    public func register(token: String) async throws {
        guard let url = URL(string: "\(config.baseURL)/api/device-tokens") else {
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

    /// Unregister a device token. Token is sent in the request body, not the URL path.
    public func unregister(token: String) async throws {
        guard let url = URL(string: "\(config.baseURL)/api/device-tokens") else {
            throw URLError(.badURL)
        }
        var request = URLRequest(url: url)
        request.httpMethod = "DELETE"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        let body: [String: String] = ["token": token]
        request.httpBody = try JSONEncoder().encode(body)

        let (_, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw URLError(.badServerResponse)
        }
    }
}
