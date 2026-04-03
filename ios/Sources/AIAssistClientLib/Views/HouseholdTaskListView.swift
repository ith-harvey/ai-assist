import SwiftUI

/// Filter mode for the household task list.
private enum HouseholdFilter: Hashable {
    case all
    case category(HouseholdTaskCategory)
    case assignee(String) // user_id
}

/// Tab for active vs completed.
private enum HouseholdTab: String, CaseIterable {
    case active = "Active"
    case completed = "Completed"
}

/// Main household task list with category grouping and assignee filtering.
///
/// - Segmented control: Active | Completed
/// - Filter chips: All, Chores, Errands, Meals, Other, plus assignee avatars
/// - Tasks grouped by category with section headers
/// - Swipe right → complete, swipe left → delete
/// - Tap → push detail view
/// - Pull to refresh
/// - Quick-add FAB
public struct HouseholdTaskListView: View {
    let socket: HouseholdWebSocket
    @State private var selectedTab: HouseholdTab = .active
    @State private var filter: HouseholdFilter = .all
    @State private var selectedTask: HouseholdTask?
    @State private var showAddSheet = false

    public init(socket: HouseholdWebSocket) {
        self.socket = socket
    }

    public var body: some View {
        ZStack(alignment: .bottomTrailing) {
            if socket.tasks.isEmpty {
                emptyState
            } else {
                VStack(spacing: 0) {
                    filterBar
                    taskList
                }
            }

            addButton
        }
        .secondaryBackground()
        .navigationTitle("Household")
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("Filter", selection: $selectedTab) {
                    ForEach(HouseholdTab.allCases, id: \.self) { tab in
                        Text(tab.rawValue).tag(tab)
                    }
                }
                .pickerStyle(.segmented)
                .frame(maxWidth: 220)
            }
        }
        .navigationDestination(item: $selectedTask) { task in
            HouseholdTaskDetailView(task: task, socket: socket)
        }
        .sheet(isPresented: $showAddSheet) {
            QuickAddTaskSheet(socket: socket)
        }
    }

    // MARK: - Filter Bar

    private var filterBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                filterChip("All", icon: "line.3.horizontal.decrease.circle", isSelected: filter == .all) {
                    filter = .all
                }

                ForEach(HouseholdTaskCategory.allCases) { category in
                    filterChip(category.label, icon: category.icon, color: category.color, isSelected: filter == .category(category)) {
                        filter = .category(category)
                    }
                }

                if !socket.members.isEmpty {
                    Divider()
                        .frame(height: 24)

                    ForEach(socket.members) { member in
                        filterChip(member.displayName, emoji: member.emoji, isSelected: filter == .assignee(member.userId)) {
                            filter = .assignee(member.userId)
                        }
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
        }
    }

    private func filterChip(_ label: String, icon: String? = nil, emoji: String? = nil, color: Color = .primary, isSelected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 4) {
                if let emoji {
                    Text(emoji)
                        .font(.system(size: 14))
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
        let base = socket.activeTasks
        switch filter {
        case .all: return base
        case .category(let cat): return base.filter { $0.category == cat }
        case .assignee(let uid): return base.filter { $0.assignedTo == uid }
        }
    }

    private var filteredCompletedTasks: [HouseholdTask] {
        let base = socket.completedTasks
        switch filter {
        case .all: return base
        case .category(let cat): return base.filter { $0.category == cat }
        case .assignee(let uid): return base.filter { $0.assignedTo == uid }
        }
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
                        subtitle: filter == .all ? "No active tasks" : "No tasks match this filter"
                    )
                    .plainCardListRow()
                } else {
                    let grouped = Dictionary(grouping: tasks, by: \.category)
                    let orderedCategories = HouseholdTaskCategory.allCases.filter { grouped[$0] != nil }

                    ForEach(orderedCategories) { category in
                        Section {
                            ForEach(grouped[category] ?? []) { task in
                                householdTaskCard(task)
                                    .plainCardListRow()
                            }
                        } header: {
                            categoryHeader(category, count: grouped[category]?.count ?? 0)
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
        #if os(iOS)
        .refreshable {
            socket.connect()
        }
        .scrollDismissesKeyboard(.interactively)
        #endif
    }

    // MARK: - Category Header

    private func categoryHeader(_ category: HouseholdTaskCategory, count: Int) -> some View {
        HStack(spacing: 6) {
            Image(systemName: category.icon)
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(category.color)
            Text(category.label)
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
        EmptyStateView(
            icon: "house.fill",
            title: "No household tasks yet",
            subtitle: "Tap + to add your first task and start organizing your household together"
        )
    }
}

// MARK: - Household Task Card

/// A card-style household task row with category color stripe and swipe actions.
struct HouseholdTaskCardView: View {
    let task: HouseholdTask
    let members: [HouseholdMember]
    var onTap: () -> Void
    var onComplete: (() -> Void)? = nil
    var onDelete: (() -> Void)? = nil

    var body: some View {
        HStack(spacing: 0) {
            // Category color stripe
            RoundedRectangle(cornerRadius: 2)
                .fill(task.category.color)
                .frame(width: 4)
                .padding(.vertical, 6)

            HStack(spacing: 12) {
                // Completion toggle
                Button {
                    onComplete?()
                } label: {
                    Image(systemName: task.status.icon)
                        .font(.system(size: 22))
                        .foregroundStyle(task.status.color)
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
                        // Priority badge (if high or urgent)
                        if task.priority == .high || task.priority == .urgent {
                            HStack(spacing: 2) {
                                Image(systemName: task.priority == .urgent ? "exclamationmark.2" : "exclamationmark")
                                    .font(.system(size: 9, weight: .bold))
                                Text(task.priority.label)
                                    .font(.system(size: 11, weight: .semibold))
                            }
                            .foregroundStyle(task.priority.color)
                        }

                        // Assignee
                        if let name = task.assigneeName(in: members) {
                            HStack(spacing: 2) {
                                Image(systemName: "person.fill")
                                    .font(.system(size: 9))
                                Text(name)
                                    .font(.system(size: 11))
                            }
                            .foregroundStyle(.secondary)
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
