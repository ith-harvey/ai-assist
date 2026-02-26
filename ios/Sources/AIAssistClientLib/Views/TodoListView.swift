import SwiftUI

/// Main to-do list view with swipeable rows and inline expanded detail.
///
/// Swipe right → complete. Swipe left → delete.
/// Tap a row to expand inline detail with description, metadata, and an input bar.
/// Sections: Active (sorted by priority), Completed (collapsed by default).
/// Approval badge in nav bar shows items needing attention.
///
/// No persistent bottom input bar — input lives inside expanded todo rows only.
public struct TodoListView: View {
    @State private var todoSocket = TodoWebSocket()
    @State private var showCompleted = false
    @State private var expandedTodoId: UUID?
    @State private var activityTodo: TodoItem?

    private let title: String

    public init(title: String = "To-Dos") {
        self.title = title
    }

    public var body: some View {
        ZStack {
            #if os(iOS)
            Color(uiColor: .secondarySystemBackground)
                .ignoresSafeArea()
            #else
            Color.gray.opacity(0.08)
                .ignoresSafeArea()
            #endif

            if todoSocket.activeTodos.isEmpty && todoSocket.completedTodos.isEmpty {
                emptyState
            } else {
                todoList
            }
        }
        .navigationTitle(title)
        #if os(iOS)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                ApprovalBellBadge(count: todoSocket.approvalCount)
            }
        }
        #endif
        .navigationDestination(item: $activityTodo) { todo in
            TodoActivityView(todo: todo)
        }
        .onAppear {
            todoSocket.connect()
        }
        .onDisappear {
            todoSocket.disconnect()
        }
    }

    // MARK: - Todo List

    private var todoList: some View {
        List {
            // Active section
            if !todoSocket.activeTodos.isEmpty {
                Section {
                    ForEach(todoSocket.activeTodos) { todo in
                        todoCard(todo)
                            .listRowSeparator(.hidden)
                            .listRowBackground(Color.clear)
                            .listRowInsets(EdgeInsets(top: 5, leading: 14, bottom: 5, trailing: 14))
                    }
                } header: {
                    sectionHeader("Active")
                }
            }

            // Snoozed section
            if !todoSocket.snoozedTodos.isEmpty {
                Section {
                    ForEach(todoSocket.snoozedTodos) { todo in
                        todoCard(todo)
                            .listRowSeparator(.hidden)
                            .listRowBackground(Color.clear)
                            .listRowInsets(EdgeInsets(top: 5, leading: 14, bottom: 5, trailing: 14))
                    }
                } header: {
                    sectionHeader("Snoozed")
                }
            }

            // Completed section (collapsible)
            if !todoSocket.completedTodos.isEmpty {
                Section {
                    if showCompleted {
                        ForEach(todoSocket.completedTodos) { todo in
                            todoCard(todo)
                                .listRowSeparator(.hidden)
                                .listRowBackground(Color.clear)
                                .listRowInsets(EdgeInsets(top: 5, leading: 14, bottom: 5, trailing: 14))
                        }
                    }
                } header: {
                    completedSectionHeader
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        #if os(iOS)
        .scrollDismissesKeyboard(.interactively)
        #endif
        .animation(.spring(response: 0.35, dampingFraction: 0.8), value: expandedTodoId)
    }

    // MARK: - Section Headers

    private func sectionHeader(_ title: String) -> some View {
        HStack {
            Text(title)
                .font(.caption)
                .textCase(.uppercase)
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 6)
    }

    private var completedSectionHeader: some View {
        HStack {
            Text("Completed")
                .font(.caption)
                .textCase(.uppercase)
                .foregroundStyle(.secondary)
            Spacer()
            Text("\(todoSocket.completedTodos.count)")
                .font(.caption)
                .foregroundStyle(.tertiary)
            Image(systemName: showCompleted ? "chevron.up" : "chevron.down")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 6)
        .contentShape(Rectangle())
        .onTapGesture {
            withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                showCompleted.toggle()
            }
        }
    }

    // MARK: - Todo Card (matches approval card width/style)

    private func todoCard(_ todo: TodoItem) -> some View {
        let isExpanded = expandedTodoId == todo.id

        return VStack(alignment: .leading, spacing: 0) {
            TodoRowView(todo: todo, isExpanded: false)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)

            if isExpanded {
                TodoExpandedDetail(todo: todo)
                    .padding(.horizontal, 14)
                    .padding(.top, 4)
                    .padding(.bottom, 4)

                // Activity view link for agent-startable todos
                if todo.bucket == .agentStartable {
                    Button {
                        activityTodo = todo
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "waveform.path.ecg")
                                .font(.system(size: 13))
                            Text("View Activity")
                                .font(.system(size: 13, weight: .medium))
                        }
                        .foregroundStyle(.blue)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                        #if os(iOS)
                        .background(Color(uiColor: .systemGray6))
                        #else
                        .background(Color.gray.opacity(0.1))
                        #endif
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                    }
                    .buttonStyle(.plain)
                    .padding(.horizontal, 14)
                    .padding(.top, 2)
                }

                TodoInlineInputBar()
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
            }
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
        .shadow(color: .black.opacity(0.1), radius: 12, y: 4)
        .contentShape(Rectangle())
        .onTapGesture {
            let isCollapsing = expandedTodoId == todo.id
            withAnimation(isCollapsing
                ? .easeIn(duration: 0.2)
                : .spring(response: 0.35, dampingFraction: 0.85)
            ) {
                expandedTodoId = isCollapsing ? nil : todo.id
            }
        }
        // Right swipe — complete
        .swipeActions(edge: .leading) {
            Button {
                // TODO: Mark todo complete via WebSocket
            } label: {
                Label("Complete", systemImage: "checkmark.circle.fill")
            }
            .tint(.green)
        }
        // Left swipe — delete
        .swipeActions(edge: .trailing) {
            Button(role: .destructive) {
                // TODO: Delete todo via WebSocket
            } label: {
                Label("Delete", systemImage: "trash.fill")
            }
        }
    }

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "checklist")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No to-dos yet")
                .font(.title3)
                .foregroundStyle(.secondary)
            #if os(iOS)
            Text("Use the Brain tab to create todos with your voice")
                .font(.subheadline)
                .foregroundStyle(.tertiary)
                .multilineTextAlignment(.center)
            #endif
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - Inline Input Bar (inside expanded todo)

/// Telegram-style text + mic/send input bar shown inside expanded todo rows.
/// Self-contained state — each expanded todo gets its own input.
struct TodoInlineInputBar: View {
    @State private var inputText = ""

    var body: some View {
        HStack(spacing: 8) {
            TextField("Add a note...", text: $inputText, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.subheadline)
                .lineLimit(1...3)
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
                #if os(iOS)
                .background(Color(uiColor: .systemGray6))
                #else
                .background(Color.gray.opacity(0.12))
                #endif
                .clipShape(RoundedRectangle(cornerRadius: 16))
                .onSubmit {
                    submitInput()
                }

            ZStack {
                if !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    Button {
                        submitInput()
                    } label: {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.system(size: 28))
                            .foregroundStyle(.blue)
                    }
                    .transition(.scale.combined(with: .opacity))
                } else {
                    #if os(iOS)
                    VoiceMicButton(shouldSuppress: false) { transcript in
                        inputText = transcript
                        submitInput()
                    }
                    .zIndex(1)
                    .transition(.scale.combined(with: .opacity))
                    #else
                    Button {} label: {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.system(size: 28))
                            .foregroundStyle(.gray.opacity(0.4))
                    }
                    .disabled(true)
                    #endif
                }
            }
            .animation(.spring(response: 0.3, dampingFraction: 0.7),
                        value: !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
    }

    private func submitInput() {
        let text = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        // TODO: Send note/update to backend via WebSocket
        inputText = ""
    }
}

// MARK: - Expanded Detail (extracted from TodoRowView)

/// The metadata section shown when a todo is expanded.
struct TodoExpandedDetail: View {
    let todo: TodoItem

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            // Metadata first — immediately under title

            // Status
            detailRow(label: "Status", icon: todo.status.iconName) {
                Text(todo.status.label)
                    .foregroundStyle(.primary)
            }

            // Bucket
            detailRow(
                label: "Bucket",
                icon: todo.bucket == .agentStartable ? "cpu" : "person.fill"
            ) {
                Text(todo.bucket == .agentStartable ? "🤖 AI can start this" : "👤 Waiting on you")
                    .foregroundStyle(.primary)
            }

            // Created
            detailRow(label: "Created", icon: "clock.arrow.circlepath") {
                Text(formatCreatedDate(todo.createdAt))
                    .foregroundStyle(.primary)
            }

            // Due date (full format)
            if let due = todo.dueDate {
                detailRow(label: "Due", icon: "calendar") {
                    HStack(spacing: 4) {
                        Text(formatFullDate(due))
                            .foregroundStyle(.primary)
                        if todo.isOverdue {
                            Text("Overdue")
                                .font(.system(size: 11, weight: .semibold))
                                .foregroundStyle(.red)
                        }
                    }
                }
            }

            // Description
            if let description = todo.description, !description.isEmpty {
                Text(description)
                    .font(.subheadline)
                    .foregroundStyle(.primary)
                    .padding(.bottom, 2)
            }

            // Source card
            if todo.sourceCardId != nil {
                detailRow(label: "Source", icon: "doc.on.doc") {
                    Text("From approval card")
                        .foregroundStyle(.primary)
                }
            }

        }
        .font(.subheadline)
        .transition(.asymmetric(
            insertion: .opacity.animation(.easeOut(duration: 0.2)),
            removal: .opacity.animation(.easeIn(duration: 0.15))
        ))
    }

    // MARK: - Detail Row Helper

    private func detailRow<Content: View>(
        label: String,
        icon: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        HStack(spacing: 8) {
            Label(label, systemImage: icon)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.secondary)
                .frame(width: 90, alignment: .leading)

            content()
                .font(.subheadline)
        }
    }

    // MARK: - Formatting

    private func formatFullDate(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }

    private func formatCreatedDate(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d, yyyy"
        return formatter.string(from: date)
    }
}

// MARK: - Todo Row (compact only — no longer owns expanded detail)

/// A single compact row showing status, title, due date, and type badge.
struct TodoRowView: View {
    let todo: TodoItem
    var isExpanded: Bool = false

    var body: some View {
        HStack(spacing: 12) {
            // Status icon
            Image(systemName: todo.status.iconName)
                .font(.system(size: 18))
                .foregroundStyle(statusColor)
                .frame(width: 24)

            // Content
            VStack(alignment: .leading, spacing: 3) {
                Text(todo.title)
                    .font(.body)
                    .foregroundStyle(todo.status == .completed ? .secondary : .primary)
                    .strikethrough(todo.status == .completed)
                    .lineLimit(2)

                HStack(spacing: 6) {
                    // Type badge
                    Text(todo.todoType.label)
                        .font(.system(size: 10, weight: .medium))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(badgeColor.opacity(0.15))
                        .foregroundStyle(badgeColor)
                        .clipShape(Capsule())

                    // Due date
                    if let due = todo.dueDate {
                        HStack(spacing: 2) {
                            Image(systemName: "clock")
                                .font(.system(size: 9))
                            Text(formatDueDate(due))
                                .font(.system(size: 11))
                        }
                        .foregroundStyle(todo.isOverdue ? .red : .secondary)
                    }

                    // Agent badge
                    if todo.bucket == .agentStartable {
                        HStack(spacing: 2) {
                            Image(systemName: "cpu")
                                .font(.system(size: 9))
                            Text("Agent")
                                .font(.system(size: 10))
                        }
                        .foregroundStyle(.blue.opacity(0.7))
                    }
                }
            }

            Spacer()

            // Priority indicator
            if todo.priority <= 2 && todo.status != .completed {
                Image(systemName: "exclamationmark.circle.fill")
                    .font(.system(size: 14))
                    .foregroundStyle(todo.priority == 1 ? .red : .orange)
            }
        }
    }

    // MARK: - Colors

    private var statusColor: Color {
        switch todo.status {
        case .created: .blue
        case .agentWorking: .orange
        case .readyForReview: .green
        case .waitingOnYou: .purple
        case .snoozed: .gray
        case .completed: .green
        }
    }

    private var badgeColor: Color {
        switch todo.todoType {
        case .deliverable: .blue
        case .research: .purple
        case .errand: .orange
        case .learning: .green
        case .administrative: .gray
        case .creative: .pink
        case .review: .yellow
        }
    }

    // MARK: - Formatting

    private func formatDueDate(_ date: Date) -> String {
        let calendar = Calendar.current
        if calendar.isDateInToday(date) {
            let formatter = DateFormatter()
            formatter.dateFormat = "h:mm a"
            return "Today \(formatter.string(from: date))"
        } else if calendar.isDateInTomorrow(date) {
            let formatter = DateFormatter()
            formatter.dateFormat = "h:mm a"
            return "Tomorrow \(formatter.string(from: date))"
        } else if calendar.isDateInYesterday(date) {
            return "Yesterday"
        } else {
            let formatter = DateFormatter()
            formatter.dateFormat = "MMM d"
            return formatter.string(from: date)
        }
    }
}
