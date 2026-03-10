import SwiftUI

/// First-launch onboarding screen. Asks the user for their server URL,
/// validates via `/health`, and persists host + port to UserDefaults.
public struct OnboardingView: View {
    @AppStorage("ai_assist_onboarding_complete") private var onboardingComplete = false

    @State private var serverInput = ""
    @State private var isConnecting = false
    @State private var errorMessage: String?

    public init() {}

    public var body: some View {
        VStack(spacing: 32) {
            Spacer()

            // App title
            VStack(spacing: 8) {
                Image(systemName: "brain.head.profile")
                    .font(.system(size: 56))
                    .foregroundStyle(.tint)
                Text("AI Assist")
                    .font(.largeTitle.bold())
                Text("Enter your server address to get started.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }

            // Server URL field
            VStack(spacing: 12) {
                TextField("mac-studio:8081", text: $serverInput)
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
                    .autocorrectionDisabled()
                    .textFieldStyle(.roundedBorder)
                    .padding(.horizontal, 40)

                if let errorMessage {
                    Text(errorMessage)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 40)
                }
            }

            // Connect button
            Button {
                Task { await connect() }
            } label: {
                if isConnecting {
                    ProgressView()
                        .controlSize(.small)
                        .frame(maxWidth: .infinity)
                } else {
                    Text("Connect")
                        .fontWeight(.semibold)
                        .frame(maxWidth: .infinity)
                }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(serverInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isConnecting)
            .padding(.horizontal, 40)

            #if DEBUG
            Button("Skip") {
                onboardingComplete = true
            }
            .foregroundStyle(.secondary)
            #endif

            Spacer()
            Spacer()
        }
        .onAppear {
            let host = UserDefaults.standard.string(forKey: "ai_assist_host") ?? ""
            let port = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int
            if !host.isEmpty {
                serverInput = port != nil ? "\(host):\(port!)" : host
            }
        }
    }

    // MARK: - Connection

    private func connect() async {
        let (host, port) = parseServerInput(serverInput)

        isConnecting = true
        errorMessage = nil

        guard let url = URL(string: "http://\(host):\(port)/health") else {
            errorMessage = "Invalid server address"
            isConnecting = false
            return
        }

        do {
            let (_, response) = try await URLSession.shared.data(from: url)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
                errorMessage = "Server returned an error. Check the address and try again."
                isConnecting = false
                return
            }

            // Success — save to UserDefaults
            UserDefaults.standard.set(host, forKey: "ai_assist_host")
            UserDefaults.standard.set(port, forKey: "ai_assist_port")
            onboardingComplete = true
        } catch {
            errorMessage = "Could not connect to server. Check the address and try again."
        }

        isConnecting = false
    }

    /// Parse user input into (host, port).
    /// Handles: `http://host:port`, `host:port`, `host`
    private func parseServerInput(_ input: String) -> (String, Int) {
        var cleaned = input.trimmingCharacters(in: .whitespacesAndNewlines)

        // Strip scheme if present
        if cleaned.hasPrefix("http://") {
            cleaned = String(cleaned.dropFirst(7))
        } else if cleaned.hasPrefix("https://") {
            cleaned = String(cleaned.dropFirst(8))
        }

        // Strip trailing path
        if let slashIdx = cleaned.firstIndex(of: "/") {
            cleaned = String(cleaned[..<slashIdx])
        }

        // Split host:port
        if let colonIdx = cleaned.lastIndex(of: ":"),
           let port = Int(cleaned[cleaned.index(after: colonIdx)...]) {
            let host = String(cleaned[..<colonIdx])
            return (host, port)
        }

        // Default port 8080 if not specified
        return (cleaned, 8080)
    }
}
