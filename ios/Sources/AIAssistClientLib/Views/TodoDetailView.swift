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

/// Tracks the bottom anchor's Y position in global coordinates.
private struct BottomAnchorGlobalKey: PreferenceKey {
    static var defaultValue: CGFloat = .infinity
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

/// Tracks the scroll view's visible frame height in global coordinates.
private struct ScrollViewFrameKey: PreferenceKey {
    static var defaultValue: CGRect = .zero
    static func reduce(value: inout CGRect, nextValue: () -> CGRect) {
        value = nextValue()
    }
}

public struct TodoDetailView: View {
    let todo: TodoItem
    let cardSocket: CardWebSocket
    @State private var activitySocket: TodoActivitySocket
    @State private var isDescriptionExpanded = false
    @State private var isHeaderCollapsed = false
    @State private var isActivityExpanded: Bool
    @State private var approvalCard: ApprovalCard?
    /// Whether the user is near the bottom of the scroll view (for auto-scroll).
    @State private var isNearBottom: Bool = true
    /// Bottom edge of the scroll view's visible frame (global Y).
    @State private var scrollViewMaxY: CGFloat = 0
    /// Documents fetched via REST for completed todos.
    @State private var deliverables: [Document] = []
    /// Text input for follow-up messages.
    @State private var inputText = ""
    /// Todo fetched via REST — source of truth for current status.
    @State private var fetchedTodo: TodoItem?

    public init(todo: TodoItem, cardSocket: CardWebSocket) {
        self.todo = todo
        self.cardSocket = cardSocket
        self._activitySocket = State(initialValue: TodoActivitySocket(todoId: todo.id))
        // Collapse activity by default when completed/readyForReview
        let isFinished = todo.status == .completed || todo.status == .readyForReview
        self._isActivityExpanded = State(initialValue: !isFinished)
    }

    public var body: some View {
        VStack(spacing: 0) {
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

                    // ── Content (layout varies by completion state) ─
                    if isCompletedState {
                        // Completed layout: banner → documents → collapsed activity
                        completionBannerFromActivity
                            .padding(.horizontal, 20)
                            .padding(.bottom, 12)

                        DocumentListSection(todoId: todo.id, host: cardSocket.host, port: cardSocket.port)
                            .padding(.horizontal, 20)
                            .padding(.bottom, 12)

                        if todo.bucket == .agentStartable {
                            collapsibleActivitySection
                                .padding(.top, 4)
                        }
                    } else {
                        // In-progress layout: documents → divider → live activity
                        DocumentListSection(todoId: todo.id, host: cardSocket.host, port: cardSocket.port)
                            .padding(.horizontal, 20)
                            .padding(.bottom, 8)

                        if todo.bucket == .agentStartable {
                            Rectangle()
                                .fill(Color.gray.opacity(0.2))
                                .frame(height: 1)
                                .padding(.horizontal, 20)

                            activitySection
                                .padding(.top, 12)
                        }
                    }

                    // Invisible bottom anchor for scroll-to-bottom
                    Color.clear
                        .frame(height: 1)
                        .id("activityBottom")
                        .background(
                            GeometryReader { geo in
                                Color.clear.preference(
                                    key: BottomAnchorGlobalKey.self,
                                    value: geo.frame(in: .global).maxY
                                )
                            }
                        )
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
            .background(
                GeometryReader { geo in
                    Color.clear.preference(
                        key: ScrollViewFrameKey.self,
                        value: geo.frame(in: .global)
                    )
                }
            )
            .onChange(of: activitySocket.hasCompletedInitialLoad) { _, loaded in
                // One-shot scroll to bottom after history replay finishes
                if loaded && !isCompletedState {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
                        proxy.scrollTo("activityBottom", anchor: .bottom)
                    }
                }
            }
            .onChange(of: activitySocket.messages.count) { _, _ in
                // Auto-scroll for live messages (after initial load), only if user
                // hasn't scrolled up and the todo is still in progress.
                if !isCompletedState && activitySocket.hasCompletedInitialLoad && isNearBottom {
                    withAnimation(.easeOut(duration: 0.15)) {
                        proxy.scrollTo("activityBottom", anchor: .bottom)
                    }
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
        .onPreferenceChange(ScrollViewFrameKey.self) { frame in
            scrollViewMaxY = frame.maxY
        }
        .onPreferenceChange(BottomAnchorGlobalKey.self) { bottomY in
            // User is "near bottom" when the bottom anchor is within 150pt
            // of the scroll view's visible bottom edge.
            isNearBottom = bottomY <= scrollViewMaxY + 150
        }
        #if os(iOS)
        .scrollDismissesKeyboard(.interactively)
        #endif
        .secondaryBackground()
        .navigationTitle("")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                connectionBadge
            }
        }
        #endif
        .task {
            let api = TodoAPI(host: cardSocket.host, port: cardSocket.port)
            if let detail = try? await api.fetchTodoDetail(id: todo.id) {
                fetchedTodo = detail.todo
            }
        }
        .onAppear {
            if todo.bucket == .agentStartable {
                activitySocket.connect()
            }
        }
        .onDisappear {
            activitySocket.disconnect()
        }
        .onChange(of: isActivityExpanded) { _, expanded in
            if expanded && !activitySocket.isConnected {
                activitySocket.connect()
            }
        }
        .onChange(of: fetchedTodo?.status) { _, newStatus in
            if newStatus == .completed || newStatus == .readyForReview {
                withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                    isActivityExpanded = false
                }
            }
        }
        .onChange(of: activitySocket.isFinished) { _, finished in
            if finished {
                withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                    isActivityExpanded = false
                    fetchedTodo?.status = .completed
                }
            }
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

        if todo.bucket == .agentStartable {
            inputBar
        }
        } // VStack
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
                            todo.todoType.tag()

                            if let priorityTag = todo.priorityTag() {
                                priorityTag
                            }

                            todo.bucket.tag()
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

            if !activitySocket.messages.isEmpty {
                // Always show full activity history
                ForEach(activitySocket.messages) { msg in
                    let isLast = msg.id == activitySocket.messages.last?.id
                    let showSpinner = isLast && !activitySocket.isFinished && !msg.isTerminal

                    HStack(alignment: .top, spacing: 8) {
                        if showSpinner {
                            ProgressView()
                                .controlSize(.mini)
                                .padding(.top, 4)
                        }
                        activityRow(msg)
                    }
                    .padding(.horizontal, 20)
                }
            } else {
                activityEmptyState
                    .padding(.horizontal, 20)
            }
        }
    }

    // MARK: - Completion State Helpers

    /// Whether the todo is in a completed/review state (prefers API-fetched status).
    private var isCompletedState: Bool {
        let status = fetchedTodo?.status ?? todo.status
        return status == .completed || status == .readyForReview
    }

    /// Completion banner driven by todo status from the API.
    @ViewBuilder
    private var completionBannerFromActivity: some View {
        let status = fetchedTodo?.status ?? todo.status
        if status == .completed {
            completedBanner(summary: "")
        } else if status == .readyForReview {
            completedBanner(summary: "Ready for your review")
        }
    }

    // MARK: - Collapsible Activity

    /// Activity feed wrapped in a collapsible disclosure section.
    private var collapsibleActivitySection: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Tappable header
            Button {
                withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                    isActivityExpanded.toggle()
                }
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "waveform.path.ecg")
                        .font(.system(size: 13))
                    Text("Agent Activity")
                        .font(.system(size: 13, weight: .semibold))

                    let stepCount = activitySocket.messages.count
                    if stepCount > 0 {
                        Text("(\(stepCount) steps)")
                            .font(.system(size: 12))
                            .foregroundStyle(.tertiary)
                    }

                    Spacer()

                    Image(systemName: "chevron.right")
                        .font(.system(size: 12, weight: .semibold))
                        .rotationEffect(.degrees(isActivityExpanded ? 90 : 0))
                }
                .foregroundStyle(.secondary)
                .padding(.horizontal, 20)
                .padding(.vertical, 10)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if isActivityExpanded {
                // Filter out the terminal banner (already shown above)
                let nonTerminal = activitySocket.messages.filter { !$0.isTerminal }
                ForEach(nonTerminal) { msg in
                    activityRow(msg)
                        .padding(.horizontal, 20)
                }
                .transition(.opacity.combined(with: .move(edge: .top)))
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
        case .userMessage(_, let content):
            userMessageRow(content: content)
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

            MarkdownBodyView(content: content)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .cardBackground(cornerRadius: 12, shadowRadius: 4, shadowY: 2)
        }
        .padding(.vertical, 2)
    }

    private func completedBanner(summary: String) -> some View {
        StatusBannerView(
            icon: "checkmark.circle.fill",
            title: "Completed",
            summary: summary,
            color: .green,
            summaryLineLimit: 3
        )
    }

    private func failedBanner(error: String) -> some View {
        StatusBannerView(
            icon: "xmark.circle.fill",
            title: "Failed",
            summary: error,
            color: .red,
            summaryLineLimit: nil
        )
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
        .cardBackground(cornerRadius: 12, shadowRadius: 0, shadowY: 0)
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

    // MARK: - User Message Row

    private func userMessageRow(content: String) -> some View {
        VStack(alignment: .trailing, spacing: 4) {
            HStack(spacing: 6) {
                Spacer()
                Text("You")
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(.blue)
            }
            Text(content)
                .font(.subheadline)
                .foregroundStyle(.white)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(.blue)
                .clipShape(RoundedRectangle(cornerRadius: 16))
        }
        .frame(maxWidth: .infinity, alignment: .trailing)
        .padding(.vertical, 2)
    }

    // MARK: - Input Bar

    private var inputBar: some View {
        SharedInputBar(
            text: $inputText,
            placeholder: "Send instructions...",
            font: .subheadline,
            sendIconSize: 28,
            onSend: {
                let text = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !text.isEmpty else { return }

                // Optimistic: add user message to local activity feed
                let msg = ActivityMessage.userMessage(todoId: todo.id, content: text)
                activitySocket.messages.append(msg)

                // Send over WebSocket
                activitySocket.send(text: text)

                // Transition UI back to in-progress if completed
                if isCompletedState {
                    transitionToInProgress()
                }

                // Clear input
                inputText = ""
            },
            onVoiceTranscript: { transcript in
                let msg = ActivityMessage.userMessage(todoId: todo.id, content: transcript)
                activitySocket.messages.append(msg)
                activitySocket.send(text: transcript)
                if isCompletedState {
                    transitionToInProgress()
                }
            }
        )
    }

    /// Optimistically flip the UI from completed → in-progress so the live
    /// activity feed is shown immediately after a follow-up message.
    private func transitionToInProgress() {
        withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
            if fetchedTodo == nil {
                var t = todo
                t.status = .agentWorking
                fetchedTodo = t
            } else {
                fetchedTodo?.status = .agentWorking
            }
            isActivityExpanded = true
        }
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

    // Badge color now comes from todo.todoType.badgeColor

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
                    .secondaryFill()
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
                    .secondaryFill()
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
