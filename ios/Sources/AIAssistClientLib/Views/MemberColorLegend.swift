import SwiftUI

/// Horizontal color legend showing household members and their assigned colors.
struct MemberColorLegend: View {
    let members: [HouseholdMember]

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 12) {
                ForEach(members) { member in
                    HStack(spacing: 5) {
                        Circle()
                            .fill(member.color)
                            .frame(width: 8, height: 8)
                        Text(member.name)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 6)
    }
}

#Preview {
    MemberColorLegend(members: HouseholdMember.samples)
}
