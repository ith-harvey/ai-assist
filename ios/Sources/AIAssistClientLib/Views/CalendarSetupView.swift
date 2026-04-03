import ObjectiveC
import SwiftUI
import AuthenticationServices

/// Setup screen shown in the Calendar tab when Google Calendar is not yet connected.
/// Initiates a server-side OAuth flow via ASWebAuthenticationSession.
struct CalendarSetupView: View {
    @AppStorage("ai_assist_gcal_connected") private var gcalConnected = false
    @State private var isConnecting = false
    @State private var errorMessage: String?
    @State private var calendarAvailable = true

    private var serverBaseURL: String {
        ServerConfig().baseURL
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
                .disabled(isConnecting || !calendarAvailable)
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
                // Try to extract a specific error message from the server response
                if let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                   let serverError = json["error"] as? String {
                    errorMessage = serverError
                } else {
                    errorMessage = "Server not available. Check your connection."
                }
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
        } catch let error as ASWebAuthenticationSessionError where error.code == .canceledLogin {
            // User cancelled the auth session via the system dialog
        } catch {
            errorMessage = "Could not connect to Google Calendar. Please try again."
            print("[CalendarSetup] OAuth error: \(error)")
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
            let contextProvider = AuthContextProvider()
            session.presentationContextProvider = contextProvider
            objc_setAssociatedObject(session, "contextProvider", contextProvider, .OBJC_ASSOCIATION_RETAIN)
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
            if let available = json["available"] as? Bool {
                calendarAvailable = available
                if !available {
                    errorMessage = "Google Calendar is not configured on the server."
                }
            }
        } catch {
            // Silently fail — don't disrupt the UI on status check failure
        }
    }
}

/// Provides a presentation anchor for ASWebAuthenticationSession.
private class AuthContextProvider: NSObject, ASWebAuthenticationPresentationContextProviding {
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        let scene = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first
        return scene?.windows.first(where: \.isKeyWindow) ?? ASPresentationAnchor()
    }
}