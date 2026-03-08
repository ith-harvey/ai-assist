import SwiftUI

/// A small capsule-shaped badge with a label and color.
/// Used for todo type badges, status badges, silo tags, etc.
struct BadgeView: View {
    let label: String
    let color: Color
    var fontSize: CGFloat = 11

    var body: some View {
        Text(label)
            .font(.system(size: fontSize, weight: .medium))
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(color.opacity(0.15))
            .foregroundStyle(color)
            .clipShape(Capsule())
    }
}
