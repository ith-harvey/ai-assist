import SwiftUI

/// Shared bell icon with red count badge for the toolbar.
/// Used across all tabs to show pending approval count.
///
/// The badge is inset so it doesn't clip outside the toolbar area.
/// Pass `count: 0` to show just the bell with no badge.
public struct ApprovalBellBadge: View {
    let count: Int

    public init(count: Int = 0) {
        self.count = count
    }

    public var body: some View {
        Button {
            // Placeholder — will open approval overlay
        } label: {
            ZStack(alignment: .topTrailing) {
                Image(systemName: "bell.fill")
                    .font(.system(size: 18))
                    .foregroundStyle(.primary)

                if count > 0 {
                    Text("\(count)")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(.white)
                        .frame(minWidth: 18, minHeight: 18)
                        .background(Color.red)
                        .clipShape(Circle())
                        .offset(x: 8, y: -6)
                }
            }
        }
    }
}
