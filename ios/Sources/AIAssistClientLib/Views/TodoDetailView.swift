import SwiftUI

/// Full-screen detail view for a single todo, pushed via NavigationStack.
///
/// Layout: header → metadata → description → divider → embedded agent activity feed.
/// The activity feed reuses rendering logic from the old `TodoActivityView`.
/// Connection badge shows live/disconnected status in the toolbar.
/// Preference key for tracking scroll offset within the detail view.
private struct ScrollOffsetKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

/// Preference keys for detecting description text truncation.
private struct FullTextHeightKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}

private struct TruncatedTextHeightKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}

public struct TodoDetailView: View {
    let todo: TodoItem
    let cardSocket: CardWebSocket
    @State private var activitySocket: TodoActivitySocket
    @State private var isDescriptionExpanded = false
    @State private var isHeaderCollapsed = false
    @State private var approvalCard: ApprovalCard?

    public init(todo: TodoItem, cardSocket: CardWebSocket) {
        self.todo = todo
        self.cardSocket = cardSocket
        self._activitySocket = State(initialValue: TodoActivitySocket(todoId: todo.id))
    }

    public var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    // ── Header ──────────────────────────────────────
                    headerSection
                        .padding(.horizontal, 20)
                        .padding(.top, 16)
                        .padding(.bottom, isHeaderCollapsed ? 8 : 12)

                    // ── Metadata (hidden when collapsed) ────────────
                    if !isHeaderCollapsed {
                        metadataSection
                            .padding(.horizontal, 20)
                            .padding(.bottom, 12)
                            .transition(.opacity.combined(with: .move(edge: .top)))
                    }

                    // ── Description (hidden when collapsed) ─────────
                    if !isHeaderCollapsed, let description = todo.description, !description.isEmpty {
                        descriptionSection(description)
                            .padding(.horizontal, 20)
                            .padding(.bottom, 16)
                            .transition(.opacity.combined(with: .move(edge: .top)))
                    }

                    // ── Divider ─────────────────────────────────────
                    if todo.bucket == .agentStartable {
                        Rectangle()
                            .fill(Color.gray.opacity(0.2))
                            .frame(height: 1)
                            .padding(.horizontal, 20)

                        // ── Activity Feed ───────────────────────────────
                        activitySection
                            .padding(.top, 12)
                    }

                    // Invisible bottom anchor for scroll-to-bottom
                    Color.clear
                        .frame(height: 1)
                        .id("activityBottom")
                }
                .padding(.bottom, 20)
                .background(
                    GeometryReader { geo in
                        Color.clear.preference(
                            key: ScrollOffsetKey.self,
                            value: geo.frame(in: .named("detailScroll")).origin.y
                        )
                    }
                )
            }
            .coordinateSpace(name: "detailScroll")
            .onAppear {
                // Scroll to bottom after a brief delay to let content render
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                    proxy.scrollTo("activityBottom", anchor: .bottom)
                }
            }
        }
        .onPreferenceChange(ScrollOffsetKey.self) { offset in
            // Collapse header when user scrolls up (content moves up, offset becomes negative)
            let shouldCollapse = offset < -80
            let shouldExpand = offset >= -40
            if shouldCollapse && !isHeaderCollapsed {
                withAnimation(.easeInOut(duration: 0.2)) {
                    isHeaderCollapsed = true
                    isDescriptionExpanded = false
                }
            } else if shouldExpand && isHeaderCollapsed {
                withAnimation(.easeInOut(duration: 0.2)) {
                    isHeaderCollapsed = false
                }
            }
        }
        #if os(iOS)
        .scrollDismissesKeyboard(.interactively)
        #endif
        #if os(iOS)
        .background(Color(uiColor: .secondarySystemBackground).ignoresSafeArea())
        #else
        .background(Color.gray.opacity(0.08).ignoresSafeArea())
        #endif
        .navigationTitle(todo.title)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                connectionBadge
            }
        }
        #endif
        .onAppear {
            if todo.bucket == .agentStartable {
                activitySocket.connect()
            }
        }
        .onDisappear {
            activitySocket.disconnect()
        }
        .sheet(item: $approvalCard) { card in
            SwipeCardContainer(
                onApprove: {
                    cardSocket.approve(cardId: card.id)
                    approvalCard = nil
                },
                onReject: {
                    cardSocket.dismiss(cardId: card.id)
                    approvalCard = nil
                }
            ) {
                CardBodyView(card: card)
            }
            .presentationDetents([.medium, .large])
        }
    }

    // MARK: - Collapsible Description

    @State private var descriptionIsTruncated = false
    @State private var fullDescriptionHeight: CGFloat = 0
    @State private var truncatedDescriptionHeight: CGFloat = 0

    private func descriptionSection(_ description: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if isDescriptionExpanded {
                Text(description)
                    .font(.subheadline)
                    .foregroundStyle(.primary)

                Text("See less")
                    .font(.subheadline)
                    .foregroundStyle(.blue)
                    .onTapGesture {
                        withAnimation(.easeInOut(duration: 0.2)) {
                            isDescriptionExpanded = false
                        }
                    }
            } else {
                Text(description)
                    .font(.subheadline)
                    .foregroundStyle(.primary)
                    .lineLimit(3)
                    .background(
                        // Measure full-height text vs truncated to detect truncation
                        Text(description)
                            .font(.subheadline)
                            .lineLimit(nil)
                            .fixedSize(horizontal: false, vertical: true)
                            .hidden()
                            .background(
                                GeometryReader { fullSize in
                                    Color.clear.preference(
                                        key: FullTextHeightKey.self,
                                        value: fullSize.size.height
                                    )
                                }
                            )
                    )
                    .overlay(
                        GeometryReader { truncatedSize in
                            Color.clear.preference(
                                key: TruncatedTextHeightKey.self,
                                value: truncatedSize.size.height
                            )
                        }
                    )
                    .onPreferenceChange(FullTextHeightKey.self) { fullHeight in
                        fullDescriptionHeight = fullHeight
                        updateTruncationState()
                    }
                    .onPreferenceChange(TruncatedTextHeightKey.self) { truncatedHeight in
                        truncatedDescriptionHeight = truncatedHeight
                        updateTruncationState()
                    }

                if descriptionIsTruncated {
                    Text("… See more")
                        .font(.subheadline)
                        .foregroundStyle(.blue)
                        .onTapGesture {
                            withAnimation(.easeInOut(duration: 0.2)) {
                                isDescriptionExpanded = true
                            }
                        }
                }
            }
        }
    }

    /// Compare full vs truncated text height to determine if "See more" is needed.
    /// Called whenever either height preference changes.
    private func updateTruncationState() {
        // Both heights must be measured before comparing
        guard fullDescriptionHeight > 0, truncatedDescriptionHeight > 0 else { return }
        descriptionIsTruncated = fullDescriptionHeight > truncatedDescriptionHeight + 1
    }

    // MARK: - Header

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            if isHeaderCollapsed {
                // Compact: title only
                Text(todo.title)
                    .font(.title3)
                    .fontWeight(.semibold)
                    .foregroundStyle(.primary)
                    .lineLimit(2)
            } else {
                // Full header with icon, badges
                HStack(spacing: 10) {
                    // Status icon
                    Image(systemName: todo.status.iconName)
                        .font(.system(size: 24))
                        .foregroundStyle(statusColor)

                    VStack(alignment: .leading, spacing: 3) {
                        Text(todo.title)
                            .font(.title3)
                            .fontWeight(.semibold)
                            .foregroundStyle(.primary)
                            .lineLimit(3)

                        HStack(spacing: 8) {
                            // Type badge
                            Text(todo.todoType.label)
                                .font(.system(size: 11, weight: .medium))
                                .padding(.horizontal, 8)
                                .padding(.vertical, 3)
                                .background(badgeColor.opacity(0.15))
                                .foregroundStyle(badgeColor)
                                .clipShape(Capsule())

                            // Priority
                            if todo.priority <= 2 {
                                HStack(spacing: 2) {
                                    Image(systemName: "exclamationmark.circle.fill")
                                        .font(.system(size: 11))
                                    Text(todo.priority == 1 ? "High" : "Medium")
                                        .font(.system(size: 11, weight: .medium))
                                }
                                .foregroundStyle(todo.priority == 1 ? .red : .orange)
                            }

                            // Bucket
                            HStack(spacing: 3) {
                                Image(systemName: todo.bucket == .agentStartable ? "cpu" : "person.fill")
                                    .font(.system(size: 10))
                                Text(todo.bucket == .agentStartable ? "Agent" : "Human")
                                    .font(.system(size: 11, weight: .medium))
                            }
                            .foregroundStyle(todo.bucket == .agentStartable ? .blue : .purple)
                        }
                    }
                }
            }
        }
    }

    // MARK: - Metadata

    private var metadataSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            // Status
            metadataRow(label: "Status", icon: todo.status.iconName) {
                Text(todo.status.label)
            }

            // Due date
            if let due = todo.dueDate {
                metadataRow(label: "Due", icon: "calendar") {
                    HStack(spacing: 4) {
                        Text(formatFullDate(due))
                        if todo.isOverdue {
                            Text("Overdue")
                                .font(.system(size: 11, weight: .semibold))
                                .foregroundStyle(.red)
                        }
                    }
                }
            }

            // Created
            metadataRow(label: "Created", icon: "clock.arrow.circlepath") {
                Text(formatCreatedDate(todo.createdAt))
            }

            // Source card
            if todo.sourceCardId != nil {
                metadataRow(label: "Source", icon: "doc.on.doc") {
                    Text("From approval card")
                }
            }
        }
        .font(.subheadline)
    }

    private func metadataRow<Content: View>(
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
                .foregroundStyle(.primary)
        }
    }

    // MARK: - Activity Feed (embedded)

    private var activitySection: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Section header
            HStack(spacing: 6) {
                Image(systemName: "waveform.path.ecg")
                    .font(.system(size: 13))
                Text("Agent Activity")
                    .font(.system(size: 13, weight: .semibold))
            }
            .foregroundStyle(.secondary)
            .padding(.horizontal, 20)

            if activitySocket.isFinished {
                // Show all messages when finished (includes failed + transcript)
                ForEach(activitySocket.messages) { msg in
                    activityRow(msg)
                        .padding(.horizontal, 20)
                }
            } else if let latest = activitySocket.latestActivity {
                // Show only the latest event while running
                HStack(alignment: .top, spacing: 8) {
                    if !latest.isTerminal {
                        ProgressView()
                            .controlSize(.mini)
                            .padding(.top, 4)
                    }
                    activityRow(latest)
                }
                    .id(latest.id)
                    .padding(.horizontal, 20)
                    .animation(.easeInOut(duration: 0.2), value: latest.id)
            } else {
                activityEmptyState
                    .padding(.horizontal, 20)
            }
        }
    }

    // MARK: - Activity Row Dispatcher

    @ViewBuilder
    private func activityRow(_ message: ActivityMessage) -> some View {
        switch message {
        case .started:
            startedRow()
        case .thinking(_, let iteration):
            thinkingRow(iteration: iteration)
        case .toolStarted(_, let toolName):
            toolStartedRow(toolName: toolName)
        case .toolCompleted(_, let toolName, let success, let summary):
            ToolCompletedRowView(toolName: toolName, success: success, summary: summary)
        case .reasoning(_, let content):
            reasoningRow(content: content)
        case .agentResponse(_, let content):
            agentResponseRow(content: content)
        case .completed(_, let summary):
            completedBanner(summary: summary)
        case .failed(_, let error):
            failedBanner(error: error)
        case .transcript(_, let messages):
            transcriptView(messages: messages)
        case .approvalNeeded(_, let cardId, let toolName, let description):
            approvalPendingRow(cardId: cardId, toolName: toolName, description: description)
        case .approvalResolved(_, _, let approved):
            approvalResolvedRow(approved: approved)
        }
    }

    // MARK: - Activity Rows

    private func startedRow() -> some View {
        HStack(spacing: 8) {
            Image(systemName: "play.circle.fill")
                .font(.system(size: 16))
                .foregroundStyle(.blue)
            Text("Agent started working")
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(.primary)
        }
        .padding(.vertical, 4)
    }

    private func thinkingRow(iteration: UInt32) -> some View {
        HStack(spacing: 8) {
            Text("Thinking...")
                .font(.subheadline)
                .italic()
                .foregroundStyle(.secondary)
            if iteration > 1 {
                Text("(step \(iteration))")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 2)
    }

    private func toolStartedRow(toolName: String) -> some View {
        HStack(spacing: 8) {
            toolBadge(toolName)
            Text("running...")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
    }

    private func reasoningRow(content: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "brain.head.profile")
                .font(.system(size: 14))
                .foregroundStyle(.purple.opacity(0.7))
            Text(content)
                .font(.subheadline)
                .italic()
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
    }

    private func agentResponseRow(content: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Image(systemName: "bubble.left.fill")
                    .font(.system(size: 12))
                    .foregroundStyle(.blue)
                Text("Agent")
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(.blue)
            }

            Text(content)
                .font(.subheadline)
                .foregroundStyle(.primary)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                #if os(iOS)
                .background(Color(uiColor: .systemBackground))
                #else
                .background(Color.white)
                #endif
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .shadow(color: .black.opacity(0.06), radius: 4, y: 2)
        }
        .padding(.vertical, 2)
    }

    private func completedBanner(summary: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 20))
                    .foregroundStyle(.green)
                Text("Completed")
                    .font(.headline)
                    .foregroundStyle(.green)
            }
            if !summary.isEmpty {
                Text(summary)
                    .font(.subheadline)
                    .foregroundStyle(.primary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(.green.opacity(0.1))
                .overlay(
                    RoundedRectangle(cornerRadius: 14)
                        .strokeBorder(.green.opacity(0.3), lineWidth: 1)
                )
        )
        .padding(.top, 4)
    }

    private func failedBanner(error: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Image(systemName: "xmark.circle.fill")
                    .font(.system(size: 20))
                    .foregroundStyle(.red)
                Text("Failed")
                    .font(.headline)
                    .foregroundStyle(.red)
            }
            if !error.isEmpty {
                Text(error)
                    .font(.subheadline)
                    .foregroundStyle(.primary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(.red.opacity(0.1))
                .overlay(
                    RoundedRectangle(cornerRadius: 14)
                        .strokeBorder(.red.opacity(0.3), lineWidth: 1)
                )
        )
        .padding(.top, 4)
    }

    // MARK: - Transcript View

    private func transcriptView(messages: [TranscriptMessage]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: "doc.text.magnifyingglass")
                    .font(.system(size: 16))
                    .foregroundStyle(.purple)
                Text("Agent Transcript")
                    .font(.subheadline)
                    .fontWeight(.semibold)
                    .foregroundStyle(.purple)
                Spacer()
                Text("\(messages.count) messages")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            ForEach(messages) { msg in
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 4) {
                        Text(transcriptRoleLabel(msg.role))
                            .font(.system(size: 10, weight: .bold, design: .monospaced))
                            .foregroundStyle(transcriptRoleColor(msg.role))
                        if let tool = msg.toolName {
                            Text(tool)
                                .font(.system(size: 10, design: .monospaced))
                                .foregroundStyle(.secondary)
                        }
                    }
                    Text(msg.content)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(.primary)
                        .textSelection(.enabled)
                }
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(
                    RoundedRectangle(cornerRadius: 8)
                        .fill(transcriptBgColor(msg.role))
                )
            }
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(.purple.opacity(0.05))
                .overlay(
                    RoundedRectangle(cornerRadius: 14)
                        .strokeBorder(.purple.opacity(0.2), lineWidth: 1)
                )
        )
        .padding(.top, 4)
    }

    private func transcriptRoleLabel(_ role: String) -> String {
        switch role {
        case "user": return "USER"
        case "assistant": return "ASSISTANT"
        case "system": return "SYSTEM"
        case "tool_start": return "TOOL →"
        case "tool_end": return "TOOL ←"
        case "tool_result": return "RESULT"
        default: return role.uppercased()
        }
    }

    private func transcriptRoleColor(_ role: String) -> Color {
        switch role {
        case "user": return .blue
        case "assistant": return .green
        case "system": return .orange
        case "tool_start", "tool_end", "tool_result": return .purple
        default: return .secondary
        }
    }

    private func transcriptBgColor(_ role: String) -> Color {
        switch role {
        case "user": return .blue.opacity(0.08)
        case "assistant": return .green.opacity(0.08)
        case "tool_result": return .purple.opacity(0.08)
        default: return .gray.opacity(0.06)
        }
    }

    // MARK: - Approval Rows

    private func approvalPendingRow(cardId: UUID, toolName: String, description: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "bolt.circle.fill")
                .font(.system(size: 20))
                .foregroundStyle(.orange)
                .symbolEffect(.pulse, isActive: true)

            VStack(alignment: .leading, spacing: 3) {
                Text(toolName)
                    .font(.subheadline)
                    .fontWeight(.semibold)
                    .foregroundStyle(.primary)

                Text(description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)

                Text("Tap to review")
                    .font(.caption2)
                    .foregroundStyle(.orange)
            }

            Spacer()

            Image(systemName: "chevron.right")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.orange.opacity(0.6))
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 12)
                #if os(iOS)
                .fill(Color(uiColor: .systemBackground))
                #else
                .fill(Color.white)
                #endif
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .strokeBorder(.orange.opacity(0.5), lineWidth: 1.5)
        )
        .contentShape(Rectangle())
        .onTapGesture {
            approvalCard = cardSocket.cards.first(where: { $0.id == cardId })
        }
        .padding(.vertical, 2)
    }

    private func approvalResolvedRow(approved: Bool) -> some View {
        HStack(spacing: 8) {
            Image(systemName: approved ? "checkmark.circle.fill" : "xmark.circle.fill")
                .font(.system(size: 16))
                .foregroundStyle(approved ? .green : .red)
            Text(approved ? "Tool approved" : "Tool rejected")
                .font(.subheadline)
                .foregroundStyle(approved ? .green : .red)
        }
        .padding(.vertical, 4)
    }

    // MARK: - Shared Components

    private func toolBadge(_ name: String) -> some View {
        Text(name)
            .font(.system(size: 11, weight: .semibold, design: .monospaced))
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            #if os(iOS)
            .background(Color(uiColor: .systemGray5))
            #else
            .background(Color.gray.opacity(0.15))
            #endif
            .clipShape(Capsule())
    }

    // MARK: - Connection Badge

    private var connectionBadge: some View {
        HStack(spacing: 4) {
            Circle()
                .fill(activitySocket.isFinished ? .green
                    : activitySocket.isConnected ? .green : .red)
                .frame(width: 6, height: 6)
            Text(activitySocket.isFinished ? "Finished"
                : activitySocket.isConnected ? "Live" : "Disconnected")
                .font(.system(size: 10))
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Activity Empty State

    private var activityEmptyState: some View {
        VStack(spacing: 12) {
            if activitySocket.isConnected {
                ProgressView()
                    .controlSize(.small)
                Text("Waiting for agent to start...")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                Image(systemName: "wifi.slash")
                    .font(.system(size: 24))
                    .foregroundStyle(.tertiary)
                Text("Connecting to activity stream...")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 24)
    }

    // MARK: - Colors

    private var statusColor: Color {
        switch todo.status {
        case .created: .blue
        case .agentWorking: .orange
        case .awaitingApproval: .orange
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

// MARK: - Tool Completed Row (collapsible detail)

/// Collapsible tool completion row — reused from old TodoActivityView.
private struct ToolCompletedRowView: View {
    let toolName: String
    let success: Bool
    let summary: String

    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                Text(toolName)
                    .font(.system(size: 11, weight: .semibold, design: .monospaced))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    #if os(iOS)
                    .background(Color(uiColor: .systemGray5))
                    #else
                    .background(Color.gray.opacity(0.15))
                    #endif
                    .clipShape(Capsule())

                Spacer()

                if !summary.isEmpty {
                    Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                        .font(.system(size: 10))
                        .foregroundStyle(.tertiary)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture {
                guard !summary.isEmpty else { return }
                withAnimation(.spring(response: 0.25, dampingFraction: 0.8)) {
                    isExpanded.toggle()
                }
            }

            if isExpanded && !summary.isEmpty {
                Text(summary)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .padding(10)
                    #if os(iOS)
                    .background(Color(uiColor: .systemGray6))
                    #else
                    .background(Color.gray.opacity(0.08))
                    #endif
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .transition(.asymmetric(
                        insertion: .opacity.combined(with: .move(edge: .top)),
                        removal: .opacity
                    ))
            }
        }
        .padding(.vertical, 2)
    }
}
