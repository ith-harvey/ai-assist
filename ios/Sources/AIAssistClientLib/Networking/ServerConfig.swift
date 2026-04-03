import Foundation

/// Centralized server configuration for the networking layer.
/// All API clients and WebSocket connections read scheme, host, and port from here.
///
/// Defaults to HTTPS for production safety. Set `useSecureTransport = false`
/// for local development servers that don't have TLS configured.
public struct ServerConfig: Sendable {
    public let host: String
    public let port: Int
    public let useSecureTransport: Bool

    /// HTTP(S) scheme based on transport setting.
    public var httpScheme: String { useSecureTransport ? "https" : "http" }

    /// WebSocket scheme based on transport setting.
    public var wsScheme: String { useSecureTransport ? "wss" : "ws" }

    /// Base URL for REST API calls (e.g. "https://example.com:443").
    public var baseURL: String { "\(httpScheme)://\(host):\(port)" }

    /// Base URL for WebSocket connections (e.g. "wss://example.com:443").
    public var wsBaseURL: String { "\(wsScheme)://\(host):\(port)" }

    public init(
        host: String = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost",
        port: Int = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080,
        useSecureTransport: Bool = UserDefaults.standard.object(forKey: "ai_assist_secure_transport") as? Bool ?? true
    ) {
        self.host = host
        self.port = port
        self.useSecureTransport = useSecureTransport
    }
}
