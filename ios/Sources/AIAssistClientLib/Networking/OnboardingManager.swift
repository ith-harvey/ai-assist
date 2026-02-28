import Foundation
import Observation

/// Manages onboarding state by polling the server's REST API.
///
/// On app launch, `checkStatus()` hits `GET /api/onboarding/status` to determine
/// whether the user has completed onboarding. If not, the UI presents the
/// onboarding flow as a full-screen cover.
@Observable
public final class OnboardingManager: @unchecked Sendable {
    // MARK: - Published State

    /// Whether onboarding has been completed. Drives the `.fullScreenCover` dismiss.
    public var isOnboardingComplete: Bool = true

    /// Current onboarding phase name from the server (e.g. "identity", "complete").
    public var currentPhase: String = "not_started"

    /// Whether we've finished the initial status check.
    public var hasCheckedStatus: Bool = false

    /// User's display name, populated after onboarding completes.
    public var userName: String?

    // MARK: - Configuration

    public private(set) var host: String
    public private(set) var port: Int

    public init(host: String = "192.168.0.5", port: Int = 8080) {
        self.host = host
        self.port = port
    }

    // MARK: - Status Check

    /// Check onboarding status from the server.
    /// Call on app launch to decide whether to show the onboarding flow.
    public func checkStatus() {
        guard let url = URL(string: "http://\(host):\(port)/api/onboarding/status") else {
            hasCheckedStatus = true
            return
        }

        URLSession.shared.dataTask(with: url) { [weak self] data, response, error in
            DispatchQueue.main.async {
                guard let self else { return }
                self.hasCheckedStatus = true

                guard let data,
                      let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
                else {
                    // If server is unreachable, assume onboarding is complete
                    // (don't block the user from using the app)
                    self.isOnboardingComplete = true
                    return
                }

                let completed = json["onboarding_completed"] as? Bool ?? true
                let phase = json["phase"] as? String ?? "not_started"

                self.isOnboardingComplete = completed
                self.currentPhase = phase

                // Extract user name from profile if available
                if let profile = json["profile"] as? [String: Any] {
                    self.userName = profile["name"] as? String
                }
            }
        }.resume()
    }

    /// Mark onboarding as complete locally (called when the chat flow finishes).
    public func markComplete() {
        isOnboardingComplete = true
        currentPhase = "complete"
    }

    /// Re-check status from the server (e.g. after onboarding chat completes).
    public func refresh() {
        checkStatus()
    }

    /// Update server connection info.
    public func updateServer(host: String, port: Int) {
        self.host = host
        self.port = port
    }
}
