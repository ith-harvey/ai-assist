import SwiftUI
import AuthenticationServices

/// Setup screen shown in the Calendar tab when Google Calendar is not yet connected.
/// Initiates a server-side OAuth flow via ASWebAuthenticationSession.
struct CalendarSetupView: View {
    @AppStorage("ai_assist_gcal_connected") private var gcalConnected = false
    @State private var isConnecting = false
    @State private var errorMessage: String?

    private var serverBaseURL: String {
        let host = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost"
        let port = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
        return "http://\(host):\(port)"
    }

    var body: some View {
        VStack(spacing: 32) {
            Spacer()

            VStack(spacing: 8) {
                Image(systemName: "calendar.badge.plus")
                    .font(.system(size: 56))
                    .foregroundStyle(.tint)
                Text("Set up your calendar")
                    .font(.title.bold())
                Text("Connect your Google Calendar to see your schedule and let your AI manage events.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 20)
            }

            VStack(spacing: 12) {
                Button {
                    Task { await connectGoogleCalendar() }
                } label: {
                    if isConnecting {
                        ProgressView()
                            .controlSize(.small)
                            .frame(maxWidth: .infinity)
                    } else {
                        Text("Connect Google Calendar")
                            .fontWeight(.semibold)
                            .frame(maxWidth: .infinity)
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(isConnecting)
                .padding(.horizontal, 40)

                if let errorMessage {
                    Text(errorMessage)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 40)
                }
            }

            Spacer()
            Spacer()
        }
        .task {
            await checkCalendarStatus()
        }
    }

    // MARK: - OAuth Flow

    private func connectGoogleCalendar() async {
        isConnecting = true
        errorMessage = nil

        do {
            // Step 1: Get the consent URL from the server
            let startURL = URL(string: "\(serverBaseURL)/auth/google/start")!
            let (data, response) = try await URLSession.shared.data(from: startURL)

            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
                errorMessage = "Server not available. Check your connection."
                isConnecting = false
                return
            }

            guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let consentURLString = json["url"] as? String,
                  let consentURL = URL(string: consentURLString) else {
                errorMessage = "Could not get authorization URL from server."
                isConnecting = false
                return
            }

            // Step 2: Open the Google consent page in ASWebAuthenticationSession
            let callbackURL = try await startOAuthSession(url: consentURL)

            // Step 3: Check if the callback indicates success
            if let host = callbackURL.host(), host == "calendar",
               callbackURL.path().contains("connected") {
                // Step 4: Verify with server
                await checkCalendarStatus()
            } else {
                errorMessage = "Calendar connection was cancelled or failed."
            }
        } catch is CancellationError {
            // User cancelled the auth session
        } catch {
            errorMessage = "Could not connect to Google Calendar. Please try again."
        }

        isConnecting = false
    }

    /// Open ASWebAuthenticationSession and return the callback URL.
    @MainActor
    private func startOAuthSession(url: URL) async throws -> URL {
        try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(
                url: url,
                callbackURLScheme: "aiassist"
            ) { callbackURL, error in
                if let error {
                    continuation.resume(throwing: error)
                } else if let callbackURL {
                    continuation.resume(returning: callbackURL)
                } else {
                    continuation.resume(throwing: CancellationError())
                }
            }
            session.prefersEphemeralWebBrowserSession = false
            session.start()
        }
    }

    // MARK: - Status Check

    /// Check server-side calendar connection status and sync with local state.
    private func checkCalendarStatus() async {
        guard let url = URL(string: "\(serverBaseURL)/api/calendar/status") else { return }

        do {
            let (data, response) = try await URLSession.shared.data(from: url)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else { return }
            guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let connected = json["connected"] as? Bool else { return }
            gcalConnected = connected
        } catch {
            // Silently fail — don't disrupt the UI on status check failure
        }
    }
}
