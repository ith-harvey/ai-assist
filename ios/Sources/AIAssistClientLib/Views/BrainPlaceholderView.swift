import SwiftUI

/// Placeholder view for the Brain tab.
public struct BrainPlaceholderView: View {
    public init() {}

    public var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "brain.head.profile")
                .font(.system(size: 64))
                .foregroundStyle(.secondary)
            Text("Coming soon")
                .font(.title3)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
