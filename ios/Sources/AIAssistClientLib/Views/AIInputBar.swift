import SwiftUI

/// Global AI input bar with Telegram-style mic/send swap.
///
/// Floats at the bottom of every tab. Wraps `SharedInputBar` with
/// a Brain-specific status indicator showing tool use, thinking, etc.
/// Wired to a shared `ChatWebSocket` for conversation continuity.
public struct AIInputBar: View {
    let chatSocket: ChatWebSocket
    @Binding var inputText: String

    public init(chatSocket: ChatWebSocket, inputText: Binding<String>) {
        self.chatSocket = chatSocket
        self._inputText = inputText
    }

    private var canSend: Bool {
        !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && chatSocket.currentStatus == nil
    }

    public var body: some View {
        VStack(spacing: 0) {
            statusIndicator

            SharedInputBar(
                text: $inputText,
                placeholder: "Message your AI...",
                font: .system(.body, design: .monospaced),
                canSend: canSend,
                onSend: {
                    guard canSend else { return }
                    chatSocket.send(text: inputText)
                    inputText = ""
                },
                onVoiceTranscript: { transcript in
                    chatSocket.send(text: transcript)
                }
            )
        }
    }

    // MARK: - Status Indicator

    @ViewBuilder
    private var statusIndicator: some View {
        if let status = chatSocket.currentStatus {
            HStack(spacing: 6) {
                statusIcon(for: status)
                statusText(for: status)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            .transition(.opacity)
        }
    }

    @ViewBuilder
    private func statusIcon(for status: StatusEvent) -> some View {
        switch status.kind {
        case .thinking:
            ProgressView()
                .controlSize(.small)
        case .toolStarted:
            Image(systemName: "wrench.and.screwdriver")
                .font(.caption)
                .foregroundStyle(.orange)
        case .toolCompleted(_, let success):
            Image(systemName: success ? "checkmark.circle" : "xmark.circle")
                .font(.caption)
                .foregroundStyle(success ? .green : .red)
        case .toolResult:
            Image(systemName: "doc.text")
                .font(.caption)
                .foregroundStyle(.blue)
        case .error:
            Image(systemName: "exclamationmark.triangle")
                .font(.caption)
                .foregroundStyle(.red)
        case .status:
            ProgressView()
                .controlSize(.small)
        }
    }

    private func statusText(for status: StatusEvent) -> Text {
        switch status.kind {
        case .thinking(let msg):
            Text(msg.isEmpty ? "thinking..." : msg)
        case .toolStarted(let name):
            Text("running \(name)...")
        case .toolCompleted(let name, let success):
            Text("\(name) \(success ? "done" : "failed")")
        case .toolResult(let name, let preview):
            Text("\(name): \(preview)")
        case .error(let msg):
            Text(msg)
        case .status(let msg):
            Text(msg)
        }
    }
}
