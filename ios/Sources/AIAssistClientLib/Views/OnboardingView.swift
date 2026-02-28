import SwiftUI

/// Full-screen onboarding flow presented as a modal on first launch.
///
/// Three screens:
/// 1. **Welcome** — splash with app branding and "Get Started" button
/// 2. **Chat** — conversational onboarding via the existing Brain chat WebSocket
/// 3. **Complete** — confirmation screen with "Let's Go" button
public struct OnboardingView: View {
    @Binding var isPresented: Bool
    @State private var screen: OnboardingScreen = .welcome
    @State private var userName: String?

    /// Callback fired when onboarding finishes (after user taps "Let's Go").
    var onComplete: (() -> Void)?

    public init(isPresented: Binding<Bool>, onComplete: (() -> Void)? = nil) {
        self._isPresented = isPresented
        self.onComplete = onComplete
    }

    public var body: some View {
        ZStack {
            backgroundGradient

            switch screen {
            case .welcome:
                welcomeScreen
                    .transition(.opacity.combined(with: .scale(scale: 0.95)))

            case .chat:
                OnboardingChatView(onComplete: { name in
                    userName = name
                    withAnimation(.easeInOut(duration: 0.4)) {
                        screen = .complete
                    }
                })
                .transition(.opacity)

            case .complete:
                completeScreen
                    .transition(.opacity.combined(with: .scale(scale: 0.95)))
            }
        }
        #if os(iOS)
        .statusBarHidden(screen == .welcome)
        #endif
    }

    // MARK: - Screens

    private enum OnboardingScreen {
        case welcome, chat, complete
    }

    // MARK: - Welcome Screen

    private var welcomeScreen: some View {
        VStack(spacing: 32) {
            Spacer()

            // App icon / branding
            VStack(spacing: 16) {
                Image(systemName: "brain.head.profile")
                    .font(.system(size: 72))
                    .foregroundStyle(.white)
                    .shadow(color: .white.opacity(0.3), radius: 20)

                Text("AI Assist")
                    .font(.system(size: 36, weight: .bold, design: .rounded))
                    .foregroundStyle(.white)

                Text("Your personal AI agent")
                    .font(.title3)
                    .foregroundStyle(.white.opacity(0.8))
            }

            Spacer()

            // Feature highlights
            VStack(spacing: 20) {
                featureRow(
                    icon: "message.fill",
                    title: "Smart Conversations",
                    subtitle: "Chat naturally with your AI"
                )
                featureRow(
                    icon: "checklist",
                    title: "Task Management",
                    subtitle: "Your AI works on tasks autonomously"
                )
                featureRow(
                    icon: "gearshape.fill",
                    title: "Personalized",
                    subtitle: "Learns your preferences and style"
                )
            }
            .padding(.horizontal, 32)

            Spacer()

            // CTA
            Button {
                withAnimation(.easeInOut(duration: 0.4)) {
                    screen = .chat
                }
            } label: {
                Text("Get Started")
                    .font(.headline)
                    .foregroundStyle(.black)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 16)
                    .background(.white)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
            }
            .padding(.horizontal, 32)
            .padding(.bottom, 48)
        }
    }

    private func featureRow(icon: String, title: String, subtitle: String) -> some View {
        HStack(spacing: 16) {
            Image(systemName: icon)
                .font(.title2)
                .foregroundStyle(.white)
                .frame(width: 44, height: 44)
                .background(.white.opacity(0.15))
                .clipShape(RoundedRectangle(cornerRadius: 12))

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.white)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.7))
            }

            Spacer()
        }
    }

    // MARK: - Complete Screen

    private var completeScreen: some View {
        VStack(spacing: 32) {
            Spacer()

            VStack(spacing: 16) {
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 72))
                    .foregroundStyle(.green)
                    .shadow(color: .green.opacity(0.3), radius: 20)

                Text("You're All Set!")
                    .font(.system(size: 32, weight: .bold, design: .rounded))
                    .foregroundStyle(.white)

                if let name = userName, !name.isEmpty {
                    Text("Welcome, \(name)")
                        .font(.title3)
                        .foregroundStyle(.white.opacity(0.8))
                } else {
                    Text("Your AI is ready to help")
                        .font(.title3)
                        .foregroundStyle(.white.opacity(0.8))
                }
            }

            Spacer()

            VStack(spacing: 12) {
                Text("Your preferences have been saved.")
                    .font(.subheadline)
                    .foregroundStyle(.white.opacity(0.7))
                Text("You can update them anytime in Settings.")
                    .font(.subheadline)
                    .foregroundStyle(.white.opacity(0.7))
            }

            Spacer()

            Button {
                onComplete?()
                isPresented = false
            } label: {
                Text("Let's Go")
                    .font(.headline)
                    .foregroundStyle(.black)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 16)
                    .background(.white)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
            }
            .padding(.horizontal, 32)
            .padding(.bottom, 48)
        }
    }

    // MARK: - Background

    private var backgroundGradient: some View {
        LinearGradient(
            colors: [
                Color(red: 0.1, green: 0.1, blue: 0.3),
                Color(red: 0.05, green: 0.05, blue: 0.15),
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
        .ignoresSafeArea()
    }
}
