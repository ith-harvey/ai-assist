import SwiftUI

/// Setup screen shown in the Calendar tab when Google Calendar is not yet connected.
/// Follows the OnboardingView pattern: centered content with a single CTA button.
struct CalendarSetupView: View {
    @AppStorage("ai_assist_gcal_connected") private var gcalConnected = false
    @State private var isConnecting = false
    @State private var errorMessage: String?

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
    }

    // MARK: - Connection

    // TODO: Replace with real Google OAuth flow (ASWebAuthenticationSession or Google Sign-In SDK)
    private func connectGoogleCalendar() async {
        isConnecting = true
        errorMessage = nil

        do {
            try await Task.sleep(for: .seconds(1))
            gcalConnected = true
        } catch {
            errorMessage = "Could not connect to Google Calendar. Please try again."
        }

        isConnecting = false
    }
}
