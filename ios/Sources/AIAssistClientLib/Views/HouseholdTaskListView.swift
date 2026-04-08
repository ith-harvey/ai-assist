import SwiftUI

/// Scope filter: My Tasks vs All Household tasks.
private enum HouseholdScope: String, CaseIterable {
    case all = "All"
    case mine = "My Tasks"
}

/// Filter mode for the household task list.
private enum HouseholdFilter: Hashable {
    case all
    case priority(HouseholdTaskPriority)
    case assignee(String) // userId
}

/// Tab for active vs completed.
private enum HouseholdTab: String, CaseIterable {
    case active = "Active"
    case completed = "Completed"
}

/// Main household task list with priority grouping, assignee filtering, and My Tasks toggle.
///
/// - Scope toggle: My Tasks | All
/// - Segmented control: Active | Completed
/// - Filter chips: All, Low, Medium, High, Urgent, plus assignee avatars
/// - Tasks grouped by priority with section headers
/// - Swipe right -> complete, swipe left -> delete
/// - Tap -> push detail view
/// - Quick-add FAB
/// - Toolbar button to manage household
public struct HouseholdTaskListView: View {
    let socket: HouseholdWebSocket
    @State private var selectedTab: HouseholdTab = .active
    @State private var scope: HouseholdScope = .all
    @State private var filter: HouseholdFilter = .all
    @State private var selectedTask: HouseholdTask?
    @State private var showAddSheet = false
    @State private var showHouseholdSettings = false

    public init(socket: HouseholdWebSocket) {
        self.socket = socket
    }

    /// Whether the user has a household set up.
    private var hasHousehold: Bool {
        socket.household != nil
    }

    public var body: some View {
        Group {
            if hasHousehold {
                taskContent
            } else {
                HouseholdSetupView(socket: socket)
            }
        }
        .navigationTitle("Household")
        .toolbar {
            if hasHousehold {
                ToolbarItem(placement: .principal) {
                    Picker("Filter", selection: $selectedTab) {
                        ForEach(HouseholdTab.allCases, id: \.self) { tab in
                            Text(tab.rawValue).tag(tab)
                        }
                    }
                    .pickerStyle(.segmented)
                    .frame(maxWidth: 220)
                }

                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        showHouseholdSettings = true
                    } label: {
                        Image(systemName: "person.2.circle")
                            .font(.system(size: 16))
                    }
                    .accessibilityLabel("Household settings")
                }
            }
        }
        .navigationDestination(item: $selectedTask) { task in
            HouseholdTaskDetailView(task: task, socket: socket)
        }
        .sheet(isPresented: $showAddSheet) {
            QuickAddTaskSheet(socket: socket)
        }
        .navigationDestination(isPresented: $showHouseholdSettings) {
            HouseholdView(socket: socket)
        }
    }

    // MARK: - Task Content

    private var taskContent: some View {
        ZStack(alignment: .bottomTrailing) {
            if socket.tasks.isEmpty {
                emptyState
            } else {
                VStack(spacing: 0) {
                    scopeToggle
                    filterBar
                    taskList
                }
            }
            addButton
        }
        .secondaryBackground()
    }

    // MARK: - Scope Toggle

    private var scopeToggle: some View {
        HStack(spacing: 0) {
            ForEach(HouseholdScope.allCases, id: \.self) { s in
                Button {
                    withAnimation(.spring(response: 0.25)) { scope = s }
                } label: {
                    Text(s.rawValue)
                        .font(.system(size: 13, weight: scope == s ? .semibold : .regular))
                        .foregroundStyle(scope == s ? .accentColor : .secondary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                        .background(
                            scope == s ? Color.accentColor.opacity(0.1) : Color.clear
                        )
                }
                .buttonStyle(.plain)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.secondary.opacity(0.15), lineWidth: 1))
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
    }

    // MARK: - Filter Bar

    private var filterBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                filterChip("All", icon: "line.3.horizontal.decrease.circle", isSelected: filter == .all) {
                    filter = .all
                }

                ForEach(HouseholdTaskPriority.allCases) { priority in
                    filterChip(priority.label, icon: priority.icon, color: priority.color, isSelected: filter == .priority(priority)) {
                        filter = .priority(priority)
                    }
                }

                if !socket.members.isEmpty {
                    Divider()
                        .frame(height: 24)

                    ForEach(socket.members) { member in
                        filterChip(member.displayName, initials: member.initials, isSelected: filter == .assignee(member.userId)) {
                            filter = .assignee(member.userId)
                        }
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
        }
    }

    private func filterChip(_ label: String, icon: String? = nil, initials: String? = nil, color: Color = .primary, isSelected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 4) {
                if let initials {
                    ZStack {
                        Circle()
                            .fill(isSelected ? color.opacity(0.15) : Color.secondary.opacity(0.1))
                            .frame(width: 20, height: 20)
                        Text(initials)
                            .font(.system(size: 9, weight: .bold))
                            .foregroundStyle(isSelected ? color : .secondary)
                    }
                } else if let icon {
                    Image(systemName: icon)
                        .font(.system(size: 12))
                }
                Text(label)
                    .font(.system(size: 13, weight: isSelected ? .semibold : .regular))
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(
                Capsule()
                    .fill(isSelected ? color.opacity(0.15) : Color.clear)
            )
            .overlay(
                Capsule()
                    .strokeBorder(isSelected ? color.opacity(0.4) : Color.secondary.opacity(0.2), lineWidth: 1)
            )
            .foregroundStyle(isSelected ? color : .secondary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(label) filter")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    // MARK: - Task List

    private var filteredActiveTasks: [HouseholdTask] {
        applyFilters(to: socket.activeTasks)
    }

    private var filteredCompletedTasks: [HouseholdTask] {
        applyFilters(to: socket.completedTasks)
    }

    private func applyFilters(to tasks: [HouseholdTask]) -> [HouseholdTask] {
        var result = tasks

        // Scope filter
        if scope == .mine {
            // Show only tasks assigned to "me" — for now use all members as a fallback
            // In a real app, the current user ID would come from auth
            // For sample data, filter unassigned tasks out
            result = result.filter { $0.assignedTo != nil }
        }

        // Detail filter
        switch filter {
        case .all: break
        case .priority(let p): result = result.filter { $0.priority == p }
        case .assignee(let userId): result = result.filter { $0.assignedTo == userId }
        }

        return result
    }

    private var taskList: some View {
        List {
            switch selectedTab {
            case .active:
                let tasks = filteredActiveTasks
                if tasks.isEmpty {
                    EmptyStateView(
                        icon: "checkmark.circle",
                        title: "All done!",
                        subtitle: filter == .all && scope == .all ? "No active tasks" : "No tasks match this filter"
                    )
                    .plainCardListRow()
                } else {
                    // Group by priority (urgent first)
                    let grouped = Dictionary(grouping: tasks, by: \.priority)
                    let orderedPriorities: [HouseholdTaskPriority] = [.urgent, .high, .medium, .low]
                    let activePriorities = orderedPriorities.filter { grouped[$0] != nil }

                    ForEach(activePriorities) { priority in
                        Section {
                            ForEach(grouped[priority] ?? []) { task in
                                householdTaskCard(task)
                                    .plainCardListRow()
                            }
                        } header: {
                            priorityHeader(priority, count: grouped[priority]?.count ?? 0)
                        }
                    }
                }

            case .completed:
                let tasks = filteredCompletedTasks
                if tasks.isEmpty {
                    EmptyStateView(icon: "tray", title: "No completed tasks yet")
                        .plainCardListRow()
                } else {
                    ForEach(tasks) { task in
                        householdTaskCard(task)
                            .plainCardListRow()
                    }
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .animation(.default, value: selectedTab)
        .animation(.default, value: filter)
        .animation(.default, value: scope)
        #if os(iOS)
        .refreshable {
            socket.connect()
        }
        .scrollDismissesKeyboard(.interactively)
        #endif
    }

    // MARK: - Priority Header

    private func priorityHeader(_ priority: HouseholdTaskPriority, count: Int) -> some View {
        HStack(spacing: 6) {
            Image(systemName: priority.icon)
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(priority.color)
            Text(priority.label)
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(.primary)
            Text("(\(count))")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 4)
        .padding(.top, 8)
        .padding(.bottom, 2)
        .textCase(nil)
        .listRowInsets(EdgeInsets(top: 0, leading: 14, bottom: 0, trailing: 14))
    }

    // MARK: - Task Card

    private func householdTaskCard(_ task: HouseholdTask) -> some View {
        HouseholdTaskCardView(
            task: task,
            members: socket.members,
            onTap: { selectedTask = task },
            onComplete: {
                if task.isCompleted {
                    socket.uncomplete(taskId: task.id)
                } else {
                    socket.complete(taskId: task.id)
                }
            },
            onDelete: { socket.delete(taskId: task.id) }
        )
    }

    // MARK: - Quick Add Button

    private var addButton: some View {
        Button {
            showAddSheet = true
        } label: {
            Image(systemName: "plus")
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 56, height: 56)
                .background(Circle().fill(Color.accentColor))
                .shadow(color: .accentColor.opacity(0.35), radius: 8, y: 4)
        }
        .padding(.trailing, 20)
        .padding(.bottom, 90)
        .accessibilityLabel("Add new task")
    }

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 20) {
            EmptyStateView(
                icon: "house.fill",
                title: "No household tasks yet",
                subtitle: "Tap + to add your first task and start organizing your household together"
            )
        }
    }
}

// MARK: - Household Task Card

/// A card-style household task row with priority color stripe, assignee badge, and swipe actions.
struct HouseholdTaskCardView: View {
    let task: HouseholdTask
    let members: [HouseholdMember]
    var onTap: () -> Void
    var onComplete: (() -> Void)? = nil
    var onDelete: (() -> Void)? = nil

    var body: some View {
        HStack(spacing: 0) {
            // Priority color stripe
            RoundedRectangle(cornerRadius: 2)
                .fill(task.priority.color)
                .frame(width: 4)
                .padding(.vertical, 6)

            HStack(spacing: 12) {
                // Completion toggle
                Button {
                    onComplete?()
                } label: {
                    Image(systemName: task.isCompleted ? "checkmark.circle.fill" : "circle")
                        .font(.system(size: 22))
                        .foregroundStyle(task.isCompleted ? .green : .secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(task.isCompleted ? "Mark incomplete" : "Mark complete")

                // Content
                VStack(alignment: .leading, spacing: 3) {
                    Text(task.title)
                        .font(.body)
                        .foregroundStyle(task.isCompleted ? .secondary : .primary)
                        .strikethrough(task.isCompleted)
                        .lineLimit(2)

                    HStack(spacing: 6) {
                        // Assignee badge
                        if let userId = task.assignedTo {
                            let member = members.first(where: { $0.userId == userId })
                            MemberBadgeView(member: member, userId: userId)
                        }

                        // Due date
                        if let due = task.dueDate {
                            HStack(spacing: 2) {
                                Image(systemName: "clock")
                                    .font(.system(size: 9))
                                Text(formatDueDate(due))
                                    .font(.system(size: 11))
                            }
                            .foregroundStyle(task.isOverdue ? .red : .secondary)
                        }

                        // Recurrence badge
                        if let recurrence = task.recurrence {
                            HStack(spacing: 2) {
                                Image(systemName: "repeat")
                                    .font(.system(size: 9))
                                Text(recurrence.label)
                                    .font(.system(size: 11))
                            }
                            .foregroundStyle(.blue)
                        }
                    }
                }

                Spacer()

                Image(systemName: "chevron.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
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
                .strokeBorder(task.isOverdue ? .red.opacity(0.4) : .clear, lineWidth: 1)
        )
        .shadow(color: .black.opacity(0.1), radius: 12, y: 4)
        .contentShape(Rectangle())
        .onTapGesture { onTap() }
        .swipeActions(edge: .leading) {
            if let onComplete {
                Button {
                    onComplete()
                } label: {
                    Label(task.isCompleted ? "Undo" : "Complete", systemImage: task.isCompleted ? "arrow.uturn.backward" : "checkmark.circle.fill")
                }
                .tint(task.isCompleted ? .orange : .green)
            }
        }
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

    private func formatDueDate(_ date: Date) -> String {
        let calendar = Calendar.current
        if calendar.isDateInToday(date) {
            let formatter = DateFormatter()
            formatter.dateFormat = "h:mm a"
            return "Today \(formatter.string(from: date))"
        } else if calendar.isDateInTomorrow(date) {
            return "Tomorrow"
        } else if calendar.isDateInYesterday(date) {
            return "Yesterday"
        } else {
            let formatter = DateFormatter()
            formatter.dateFormat = "MMM d"
            return formatter.string(from: date)
        }
    }
}
