import SwiftUI

/// A card-style todo item with status color stripe, approval glow, and swipe actions.
///
/// Wraps `TodoRowView` in a rounded card with:
/// - Left status color stripe
/// - Orange border glow for `awaitingApproval` status
/// - Right swipe → complete, left swipe → delete
/// - Single tap → onTap, double tap → onDoubleTap (for approval cards)
///
/// Used in TodoListView for all todo sections and search results.
struct TodoCardView: View {
    let todo: TodoItem
    var onTap: () -> Void
    var onDoubleTap: (() -> Void)? = nil
    var onComplete: (() -> Void)? = nil
    var onDelete: (() -> Void)? = nil

    var body: some View {
        HStack(spacing: 0) {
            // Status color stripe
            RoundedRectangle(cornerRadius: 2)
                .fill(todo.status.color)
                .frame(width: 4)
                .padding(.vertical, 6)

            TodoRowView(todo: todo)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
        }
        .background(
            RoundedRectangle(cornerRadius: 20)
                #if os(iOS)
                .fill(Color(uiColor: .systemBackground))
                #else
                .fill(Color.white)
                #endif
        )
        .clipShape(RoundedRectangle(cornerRadius: 20))
        .overlay(
            RoundedRectangle(cornerRadius: 20)
                .strokeBorder(.orange.opacity(todo.status == .awaitingApproval ? 0.6 : 0), lineWidth: 1.5)
        )
        .shadow(
            color: todo.status == .awaitingApproval ? .orange.opacity(0.25) : .black.opacity(0.1),
            radius: todo.status == .awaitingApproval ? 8 : 12,
            y: todo.status == .awaitingApproval ? 0 : 4
        )
        .contentShape(Rectangle())
        .onTapGesture(count: 2) {
            onDoubleTap?()
        }
        .onTapGesture(count: 1) {
            onTap()
        }
        // Right swipe — complete (hide on already-completed todos)
        .swipeActions(edge: .leading) {
            if let onComplete, todo.status != .completed {
                Button {
                    onComplete()
                } label: {
                    Label("Complete", systemImage: "checkmark.circle.fill")
                }
                .tint(.green)
            }
        }
        // Left swipe — delete
        .swipeActions(edge: .trailing) {
            if let onDelete {
                Button(role: .destructive) {
                    onDelete()
                } label: {
                    Label("Delete", systemImage: "trash.fill")
                }
            }
        }
    }
}
