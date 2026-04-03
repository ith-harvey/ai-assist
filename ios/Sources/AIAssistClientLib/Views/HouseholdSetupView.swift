import SwiftUI

/// Onboarding screen for creating or joining a household.
///
/// Shown when the user has no household yet. Two paths:
/// 1. Create a new household (name only — creator becomes Owner)
/// 2. Join an existing household by entering a household ID
///
/// After creation/join, the HouseholdWebSocket reconnects to the new household.
public struct HouseholdSetupView: View {
    let socket: HouseholdWebSocket

    @State private var mode: SetupMode = .none
    @State private var householdName = ""
    @State private var joinId = ""
    @State private var displayName = ""
    @State private var isLoading = false
    @State private var errorMessage: String?

    private var api: HouseholdAPI {
        HouseholdAPI(host: socket.host, port: socket.port)
    }

    private enum SetupMode {
        case none
        case create
        case join
    }

    public var body: some View {
        VStack(spacing: 24) {
            Spacer()

            // Hero icon
            VStack(spacing: 16) {
                ZStack {
                    Circle()
                        .fill(Color.accentColor.opacity(0.1))
                        .frame(width: 100, height: 100)
                    Image(systemName: "house.fill")
                        .font(.system(size: 44))
                        .foregroundStyle(.accentColor)
                }

                Text("Set Up Your Household")
                    .font(.title2.bold())

                Text("Create a household to share tasks with your family, or join one that already exists.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 32)
            }

            Spacer()

            // Action area
            VStack(spacing: 12) {
                switch mode {
                case .none:
                    modeButtons
                case .create:
                    createForm
                case .join:
                    joinForm
                }
            }
            .padding(.horizontal, 20)

            if let errorMessage {
                Text(errorMessage)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .padding(.horizontal, 20)
            }

            Spacer()
        }
        .secondaryBackground()
    }

    // MARK: - Mode Selection

    private var modeButtons: some View {
        VStack(spacing: 12) {
            Button {
                withAnimation(.spring(response: 0.3)) { mode = .create }
            } label: {
                HStack {
                    Image(systemName: "plus.circle.fill")
                        .font(.system(size: 20))
                    Text("Create a Household")
                        .fontWeight(.semibold)
                    Spacer()
                    Image(systemName: "chevron.right")
                        .font(.system(size: 14))
                }
                .padding(16)
                .background(Color.accentColor)
                .foregroundStyle(.white)
                .clipShape(RoundedRectangle(cornerRadius: 14))
            }
            .buttonStyle(.plain)

            Button {
                withAnimation(.spring(response: 0.3)) { mode = .join }
            } label: {
                HStack {
                    Image(systemName: "person.2.circle")
                        .font(.system(size: 20))
                    Text("Join a Household")
                        .fontWeight(.semibold)
                    Spacer()
                    Image(systemName: "chevron.right")
                        .font(.system(size: 14))
                }
                .padding(16)
                .overlay(
                    RoundedRectangle(cornerRadius: 14)
                        .strokeBorder(Color.accentColor, lineWidth: 1.5)
                )
                .foregroundStyle(.accentColor)
            }
            .buttonStyle(.plain)
        }
    }

    // MARK: - Create Form

    private var createForm: some View {
        VStack(spacing: 16) {
            TextField("Household Name (e.g. Smith Family)", text: $householdName)
                .font(.body)
                #if os(iOS)
                .textInputAutocapitalization(.words)
                #endif
                .padding(14)
                .cardBackground()

            HStack(spacing: 12) {
                Button {
                    withAnimation(.spring(response: 0.3)) { mode = .none }
                } label: {
                    Text("Back")
                        .frame(maxWidth: .infinity)
                        .padding(14)
                        .overlay(
                            RoundedRectangle(cornerRadius: 12)
                                .strokeBorder(Color.secondary.opacity(0.3), lineWidth: 1)
                        )
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)

                Button {
                    createHousehold()
                } label: {
                    HStack {
                        if isLoading {
                            ProgressView()
                                .tint(.white)
                        }
                        Text("Create")
                    }
                    .frame(maxWidth: .infinity)
                    .padding(14)
                    .background(Color.accentColor)
                    .foregroundStyle(.white)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                }
                .buttonStyle(.plain)
                .disabled(householdName.trimmingCharacters(in: .whitespaces).isEmpty || isLoading)
            }
        }
    }

    // MARK: - Join Form

    private var joinForm: some View {
        VStack(spacing: 16) {
            TextField("Household ID", text: $joinId)
                .font(.body)
                #if os(iOS)
                .textInputAutocapitalization(.never)
                #endif
                .autocorrectionDisabled()
                .padding(14)
                .cardBackground()

            TextField("Your Display Name", text: $displayName)
                .font(.body)
                #if os(iOS)
                .textInputAutocapitalization(.words)
                #endif
                .padding(14)
                .cardBackground()

            HStack(spacing: 12) {
                Button {
                    withAnimation(.spring(response: 0.3)) { mode = .none }
                } label: {
                    Text("Back")
                        .frame(maxWidth: .infinity)
                        .padding(14)
                        .overlay(
                            RoundedRectangle(cornerRadius: 12)
                                .strokeBorder(Color.secondary.opacity(0.3), lineWidth: 1)
                        )
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)

                Button {
                    joinHousehold()
                } label: {
                    HStack {
                        if isLoading {
                            ProgressView()
                                .tint(.white)
                        }
                        Text("Join")
                    }
                    .frame(maxWidth: .infinity)
                    .padding(14)
                    .background(Color.accentColor)
                    .foregroundStyle(.white)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                }
                .buttonStyle(.plain)
                .disabled(joinId.trimmingCharacters(in: .whitespaces).isEmpty || displayName.trimmingCharacters(in: .whitespaces).isEmpty || isLoading)
            }
        }
    }

    // MARK: - Actions

    private func createHousehold() {
        let name = householdName.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        isLoading = true
        errorMessage = nil

        Task {
            do {
                let household = try await api.createHousehold(name: name)
                await MainActor.run {
                    isLoading = false
                    socket.household = household
                    socket.connect(householdId: household.id)
                }
            } catch {
                await MainActor.run {
                    isLoading = false
                    errorMessage = "Failed to create household. Check your connection."
                }
            }
        }
    }

    private func joinHousehold() {
        let idStr = joinId.trimmingCharacters(in: .whitespaces)
        let name = displayName.trimmingCharacters(in: .whitespaces)
        guard !idStr.isEmpty, !name.isEmpty, let householdUUID = UUID(uuidString: idStr) else {
            errorMessage = "Enter a valid household ID."
            return
        }
        isLoading = true
        errorMessage = nil

        Task {
            do {
                // Add self as a member, then connect
                _ = try await api.addMember(householdId: householdUUID, userId: name.lowercased().replacingOccurrences(of: " ", with: "_"), displayName: name)
                let household = try await api.getHousehold(id: householdUUID)
                await MainActor.run {
                    isLoading = false
                    socket.household = household
                    socket.connect(householdId: household.id)
                }
            } catch {
                await MainActor.run {
                    isLoading = false
                    errorMessage = "Failed to join household. Check the ID and try again."
                }
            }
        }
    }
}
