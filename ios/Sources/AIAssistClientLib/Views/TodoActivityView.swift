import SwiftUI

/// Scrolling activity log for a single todo's agent work stream.
///
/// Shows a timeline of events: thinking indicators, tool executions with
/// success/failure badges, agent text responses, and terminal banners.
/// Auto-scrolls to bottom on new messages. Dark theme consistent with the app.
public struct TodoActivityView: View {
    let todo: TodoItem
    @State private var activitySocket: TodoActivitySocket

    public init(todo: TodoItem) {
        self.todo = todo
        self._activitySocket = State(initialValue: TodoActivitySocket(todoId: todo.id))
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

            if activitySocket.messages.isEmpty {
                emptyState
            } else {
                messageList
            }
        }
        .navigationTitle("Activity")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) {
                VStack(spacing: 1) {
                    Text(todo.title)
                        .font(.subheadline)
                        .fontWeight(.semibold)
                        .lineLimit(1)
                    connectionBadge
                }
            }
        }
        #endif
        .onAppear {
            activitySocket.connect()
        }
        .onDisappear {
            activitySocket.disconnect()
        }
    }

    // MARK: - Connection Badge

    private var connectionBadge: some View {
        HStack(spacing: 4) {
            Circle()
                .fill(activitySocket.isConnected ? .green : .red)
                .frame(width: 6, height: 6)
            Text(activitySocket.isConnected
                 ? (activitySocket.isFinished ? "Finished" : "Live")
                 : "Disconnected")
                .font(.system(size: 10))
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Message List

    private var messageList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    ForEach(activitySocket.messages) { message in
                        activityRow(message)
                            .id(message.id)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
            }
            #if os(iOS)
            .scrollDismissesKeyboard(.interactively)
            #endif
            .onChange(of: activitySocket.messages.count) { _, _ in
                if let last = activitySocket.messages.last {
                    withAnimation(.easeOut(duration: 0.3)) {
                        proxy.scrollTo(last.id, anchor: .bottom)
                    }
                }
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
            toolCompletedRow(toolName: toolName, success: success, summary: summary)
        case .agentResponse(_, let content):
            agentResponseRow(content: content)
        case .completed(_, let summary):
            completedBanner(summary: summary)
        case .failed(_, let error):
            failedBanner(error: error)
        }
    }

    // MARK: - Started

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

    // MARK: - Thinking

    private func thinkingRow(iteration: UInt32) -> some View {
        HStack(spacing: 8) {
            ProgressView()
                .controlSize(.small)
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

    // MARK: - Tool Started

    private func toolStartedRow(toolName: String) -> some View {
        HStack(spacing: 8) {
            ProgressView()
                .controlSize(.small)
            toolBadge(toolName)
            Text("running...")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
    }

    // MARK: - Tool Completed

    private func toolCompletedRow(toolName: String, success: Bool, summary: String) -> some View {
        ToolCompletedRowView(toolName: toolName, success: success, summary: summary)
    }

    // MARK: - Agent Response

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

    // MARK: - Completed Banner

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

    // MARK: - Failed Banner

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

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 16) {
            if activitySocket.isConnected {
                ProgressView()
                    .controlSize(.large)
                Text("Waiting for agent to start...")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else {
                Image(systemName: "wifi.slash")
                    .font(.system(size: 36))
                    .foregroundStyle(.secondary)
                Text("Connecting to activity stream...")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - Tool Completed Row (collapsible detail)

/// A separate view to hold collapsible state for tool completion detail.
private struct ToolCompletedRowView: View {
    let toolName: String
    let success: Bool
    let summary: String

    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            // Header row
            HStack(spacing: 8) {
                Image(systemName: success ? "checkmark.circle.fill" : "xmark.circle.fill")
                    .font(.system(size: 14))
                    .foregroundStyle(success ? .green : .red)

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

            // Collapsible detail
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
