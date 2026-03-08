import SwiftUI

/// Reusable section header with uppercase label, optional count, and optional chevron.
/// Used in TodoListView, TodoDetailView, DocumentListSection.
struct SectionHeaderView: View {
    let label: String
    var count: Int? = nil
    var isExpanded: Bool? = nil
    var onTap: (() -> Void)? = nil

    var body: some View {
        HStack {
            Text(label)
                .font(.caption)
                .textCase(.uppercase)
                .foregroundStyle(.secondary)
            Spacer()
            if let count {
                Text("\(count)")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            if let isExpanded {
                Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.horizontal, 6)
        .contentShape(Rectangle())
        .onTapGesture {
            onTap?()
        }
    }
}
