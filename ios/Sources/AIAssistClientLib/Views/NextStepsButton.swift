import SwiftUI

/// Reusable orange-outlined button showing pending approval count.
/// Transparent fill, orange border and text. Sized to match todo card rows.
public struct NextStepsButton: View {
    let count: Int
    let action: () -> Void

    public init(count: Int, action: @escaping () -> Void) {
        self.count = count
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack {
                Text("Next Steps")
                    .font(.body)
                    .fontWeight(.medium)
                Spacer()
                Text("\(count)")
                    .font(.body)
                    .fontWeight(.semibold)
            }
            .foregroundStyle(.orange)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
        }
        .buttonStyle(.plain)
        .background(
            RoundedRectangle(cornerRadius: 20)
                .strokeBorder(.orange, lineWidth: 1.5)
        )
        .contentShape(RoundedRectangle(cornerRadius: 20))
    }
}
