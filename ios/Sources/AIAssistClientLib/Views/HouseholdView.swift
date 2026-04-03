import SwiftUI

/// Household management screen — shows household name, member list, and management actions.
///
/// Accessible from the Household tab toolbar. Displays:
/// - Household name (editable by owner/admin)
/// - Member list with roles and initials avatars
/// - Add member form
/// - Leave / delete household actions
public struct HouseholdView: View {
    let socket: HouseholdWebSocket

    @State private var isEditingName = false
    @State private var editedName = ""
    @State private var showAddMember = false
    @State private var newMemberName = ""
    @State private var newMemberUserId = ""
    @State private var newMemberRole: HouseholdRole = .member
    @State private var isLoading = false
    @State private var errorMessage: String?

    @Environment(\.dismiss) private var dismiss

    private var api: HouseholdAPI {
        HouseholdAPI(host: socket.host, port: socket.port)
    }

    public var body: some View {
        ScrollView {
            VStack(spacing: 20) {
                householdHeader
                membersSection
                actionsSection
            }
            .padding(20)
        }
        .secondaryBackground()
        .navigationTitle("Household")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .alert("Error", isPresented: Binding(
            get: { errorMessage != nil },
            set: { if !$0 { errorMessage = nil } }
        )) {
            Button("OK") { errorMessage = nil }
        } message: {
            Text(errorMessage ?? "")
        }
        .sheet(isPresented: $showAddMember) {
            addMemberSheet
        }
    }

    // MARK: - Household Header

    private var householdHeader: some View {
        VStack(spacing: 12) {
            // Household icon
            ZStack {
                Circle()
                    .fill(Color.accentColor.opacity(0.12))
                    .frame(width: 72, height: 72)
                Image(systemName: "house.fill")
                    .font(.system(size: 32))
                    .foregroundStyle(.accentColor)
            }

            if isEditingName {
                HStack {
                    TextField("Household name", text: $editedName)
                        .font(.title2.bold())
                        .multilineTextAlignment(.center)
                        #if os(iOS)
                        .textInputAutocapitalization(.words)
                        #endif
                    Button("Save") {
                        saveHouseholdName()
                    }
                    .fontWeight(.semibold)
                    .disabled(editedName.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                .padding(.horizontal, 20)
            } else {
                Text(socket.household?.name ?? "My Household")
                    .font(.title2.bold())
                    .onTapGesture {
                        editedName = socket.household?.name ?? ""
                        isEditingName = true
                    }
            }

            Text("\(socket.members.count) member\(socket.members.count == 1 ? "" : "s")")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 20)
        .cardBackground()
    }

    // MARK: - Members

    private var membersSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Members")
                    .font(.headline)
                Spacer()
                Button {
                    showAddMember = true
                } label: {
                    Image(systemName: "person.badge.plus")
                        .font(.system(size: 16))
                }
                .accessibilityLabel("Add member")
            }

            VStack(spacing: 0) {
                ForEach(socket.members) { member in
                    memberRow(member)
                    if member.id != socket.members.last?.id {
                        Divider()
                            .padding(.leading, 52)
                    }
                }
            }
            .cardBackground()
        }
    }

    private func memberRow(_ member: HouseholdMember) -> some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(roleColor(member.role).opacity(0.12))
                    .frame(width: 40, height: 40)
                Text(member.initials)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(roleColor(member.role))
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(member.displayName)
                    .font(.body)
                Text(member.role.label)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            if member.role != .owner {
                Button {
                    removeMember(member)
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Remove \(member.displayName)")
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private func roleColor(_ role: HouseholdRole) -> Color {
        switch role {
        case .owner: .orange
        case .admin: .blue
        case .member: .accentColor
        }
    }

    // MARK: - Actions

    private var actionsSection: some View {
        VStack(spacing: 0) {
            Button(role: .destructive) {
                deleteHousehold()
            } label: {
                HStack {
                    Image(systemName: "trash")
                    Text("Delete Household")
                    Spacer()
                }
                .foregroundStyle(.red)
                .padding(14)
            }
            .buttonStyle(.plain)
        }
        .cardBackground()
    }

    // MARK: - Add Member Sheet

    private var addMemberSheet: some View {
        NavigationStack {
            Form {
                Section("Member Details") {
                    TextField("Display Name", text: $newMemberName)
                        #if os(iOS)
                        .textInputAutocapitalization(.words)
                        #endif
                    TextField("User ID", text: $newMemberUserId)
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        #endif
                        .autocorrectionDisabled()
                    Picker("Role", selection: $newMemberRole) {
                        ForEach([HouseholdRole.member, .admin], id: \.self) { role in
                            Text(role.label).tag(role)
                        }
                    }
                }
            }
            .navigationTitle("Add Member")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { showAddMember = false }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") {
                        addMember()
                    }
                    .fontWeight(.semibold)
                    .disabled(newMemberName.trimmingCharacters(in: .whitespaces).isEmpty || newMemberUserId.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
        .presentationDetents([.medium])
    }

    // MARK: - Actions

    private func saveHouseholdName() {
        guard let householdId = socket.householdId else { return }
        let name = editedName.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        isEditingName = false

        Task {
            do {
                let updated = try await api.updateHousehold(id: householdId, name: name)
                await MainActor.run {
                    socket.household = updated
                }
            } catch {
                await MainActor.run {
                    errorMessage = "Failed to update household name."
                }
            }
        }
    }

    private func addMember() {
        guard let householdId = socket.householdId else { return }
        let name = newMemberName.trimmingCharacters(in: .whitespaces)
        let userId = newMemberUserId.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty, !userId.isEmpty else { return }

        showAddMember = false

        Task {
            do {
                let member = try await api.addMember(householdId: householdId, userId: userId, displayName: name, role: newMemberRole)
                await MainActor.run {
                    socket.members.append(member)
                    newMemberName = ""
                    newMemberUserId = ""
                    newMemberRole = .member
                }
            } catch {
                await MainActor.run {
                    errorMessage = "Failed to add member."
                }
            }
        }
    }

    private func removeMember(_ member: HouseholdMember) {
        guard let householdId = socket.householdId else { return }

        Task {
            do {
                try await api.removeMember(householdId: householdId, userId: member.userId)
                await MainActor.run {
                    socket.members.removeAll { $0.id == member.id }
                }
            } catch {
                await MainActor.run {
                    errorMessage = "Failed to remove member."
                }
            }
        }
    }

    private func deleteHousehold() {
        guard let householdId = socket.householdId else { return }

        Task {
            do {
                try await api.deleteHousehold(id: householdId)
                await MainActor.run {
                    socket.household = nil
                    socket.householdId = nil
                    socket.tasks = []
                    socket.members = []
                    dismiss()
                }
            } catch {
                await MainActor.run {
                    errorMessage = "Failed to delete household."
                }
            }
        }
    }
}
