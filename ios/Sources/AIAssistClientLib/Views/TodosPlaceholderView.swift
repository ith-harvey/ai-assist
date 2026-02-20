import SwiftUI

/// Placeholder view for the Todos tab.
public struct TodosPlaceholderView: View {
    public init() {}

    public var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "checklist")
                .font(.system(size: 64))
                .foregroundStyle(.secondary)
            Text("Coming soon")
                .font(.title3)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
