import SwiftUI

/// Reusable horizontal member picker for selecting a task assignee.
///
/// Shows an "Anyone" option (nil selection) followed by each household member
/// with their initials in a circular avatar. Supports single selection.
public struct MemberPickerView: View {
    let members: [HouseholdMember]
    @Binding var selectedUserId: String?
    var label: String = "ASSIGN TO"

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(label)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    memberButton(displayName: "Anyone", initials: "?", userId: nil)
                    ForEach(members) { member in
                        memberButton(displayName: member.displayName, initials: member.initials, userId: member.userId)
                    }
                }
            }
        }
    }

    private func memberButton(displayName: String, initials: String, userId: String?) -> some View {
        let isSelected = selectedUserId == userId
        return Button {
            selectedUserId = userId
        } label: {
            VStack(spacing: 4) {
                ZStack {
                    Circle()
                        .fill(isSelected ? Color.accentColor.opacity(0.15) : Color.secondary.opacity(0.1))
                        .frame(width: 44, height: 44)
                    Circle()
                        .strokeBorder(isSelected ? Color.accentColor : Color.clear, lineWidth: 2)
                        .frame(width: 44, height: 44)
                    Text(initials)
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(isSelected ? .accentColor : .secondary)
                }
                Text(displayName)
                    .font(.system(size: 11, weight: isSelected ? .semibold : .regular))
                    .foregroundStyle(isSelected ? .accentColor : .secondary)
                    .lineLimit(1)
            }
            .frame(width: 56)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Assign to \(displayName)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

/// Inline member badge showing initials + name for a task card row.
public struct MemberBadgeView: View {
    let member: HouseholdMember?
    let userId: String?

    public init(member: HouseholdMember? = nil, userId: String? = nil) {
        self.member = member
        self.userId = userId
    }

    public var body: some View {
        if let member {
            HStack(spacing: 4) {
                ZStack {
                    Circle()
                        .fill(Color.accentColor.opacity(0.12))
                        .frame(width: 18, height: 18)
                    Text(member.initials)
                        .font(.system(size: 8, weight: .bold))
                        .foregroundStyle(.accentColor)
                }
                Text(member.displayName)
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }
        } else if let userId {
            HStack(spacing: 2) {
                Image(systemName: "person.fill")
                    .font(.system(size: 9))
                Text(userId)
                    .font(.system(size: 11))
            }
            .foregroundStyle(.secondary)
        }
    }
}
