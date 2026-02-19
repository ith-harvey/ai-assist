import SwiftUI

/// Shows the email conversation thread as iMessage-style chat bubbles.
///
/// When the card has thread context (previous emails), renders each message
/// as a bubble — incoming left-aligned (gray), outgoing right-aligned (blue).
/// Auto-scrolls to the bottom (newest messages). The AI suggested reply appears
/// as a faded/dashed "Draft" bubble at the end.
///
/// Falls back to the simple source_message → suggestedReply view when no
/// thread context is available.
struct MessageThreadView: View {
    let card: ReplyCard?

    var body: some View {
        if let card {
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(spacing: 12) {
                        threadHeader(card: card)

                        if card.thread.isEmpty {
                            // Fallback: no thread context
                            incomingBubble(
                                sender: card.sourceSender,
                                content: card.sourceMessage,
                                timestamp: nil
                            )
                            draftBubble(reply: card.suggestedReply)
                        } else {
                            // Full thread view
                            ForEach(card.thread) { msg in
                                if msg.isOutgoing {
                                    outgoingBubble(content: msg.content, timestamp: msg.timestamp)
                                } else {
                                    incomingBubble(
                                        sender: msg.sender,
                                        content: msg.content,
                                        timestamp: msg.timestamp
                                    )
                                }
                            }
                            draftBubble(reply: card.suggestedReply)
                                .id("draft")
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 8)
                    .padding(.bottom, 16)
                }
                .onAppear {
                    if !card.thread.isEmpty {
                        proxy.scrollTo("draft", anchor: .bottom)
                    }
                }
            }
        } else {
            VStack(spacing: 8) {
                Image(systemName: "bubble.left.and.bubble.right")
                    .font(.system(size: 32))
                    .foregroundStyle(.quaternary)
                Text("No active conversation")
                    .font(.subheadline)
                    .foregroundStyle(.tertiary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    // MARK: - Thread Header

    private func threadHeader(card: ReplyCard) -> some View {
        HStack(spacing: 8) {
            Image(systemName: channelIcon(for: card.channel))
                .font(.body)
                .foregroundStyle(channelColor(for: card.channel))
            Text(card.sourceSender)
                .font(.headline)
            Spacer()
            HStack(spacing: 4) {
                Circle()
                    .fill(confidenceColor(for: card.confidence))
                    .frame(width: 6, height: 6)
                Text("\(Int(card.confidence * 100))%")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                    .monospacedDigit()
            }
        }
        .padding(.horizontal, 4)
    }

    // MARK: - Bubbles

    private func incomingBubble(sender: String, content: String, timestamp: String?) -> some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 4) {
                Text(sender)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(content)
                    .font(.body)
                if let timestamp {
                    Text(formatTimestamp(timestamp))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
            .padding(12)
            .background(Color.gray.opacity(0.15))
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .frame(maxWidth: 280, alignment: .leading)

            Spacer(minLength: 40)
        }
    }

    private func outgoingBubble(content: String, timestamp: String?) -> some View {
        HStack(alignment: .top) {
            Spacer(minLength: 40)

            VStack(alignment: .trailing, spacing: 4) {
                Text(content)
                    .font(.body)
                    .foregroundStyle(.white)
                if let timestamp {
                    Text(formatTimestamp(timestamp))
                        .font(.caption2)
                        .foregroundStyle(.white.opacity(0.6))
                }
            }
            .padding(12)
            .background(Color.blue)
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .frame(maxWidth: 280, alignment: .trailing)
        }
    }

    private func draftBubble(reply: String) -> some View {
        HStack(alignment: .top) {
            Spacer(minLength: 40)

            VStack(alignment: .trailing, spacing: 4) {
                Text("AI Suggestion")
                    .font(.caption)
                    .foregroundStyle(.blue.opacity(0.7))
                Text(reply)
                    .font(.body)
                    .foregroundStyle(.primary.opacity(0.7))
            }
            .padding(12)
            .background(Color.blue.opacity(0.08))
            .overlay(
                RoundedRectangle(cornerRadius: 16)
                    .stroke(style: StrokeStyle(lineWidth: 1, dash: [6, 3]))
                    .foregroundStyle(.blue.opacity(0.4))
            )
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .frame(maxWidth: 280, alignment: .trailing)
        }
    }

    // MARK: - Helpers

    private func formatTimestamp(_ iso: String) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = formatter.date(from: iso) {
            let relative = RelativeDateTimeFormatter()
            relative.unitsStyle = .short
            return relative.localizedString(for: date, relativeTo: Date())
        }
        // Try without fractional seconds
        formatter.formatOptions = [.withInternetDateTime]
        if let date = formatter.date(from: iso) {
            let relative = RelativeDateTimeFormatter()
            relative.unitsStyle = .short
            return relative.localizedString(for: date, relativeTo: Date())
        }
        return iso
    }

    private func channelIcon(for channel: String) -> String {
        switch channel.lowercased() {
        case "telegram": return "paperplane.fill"
        case "whatsapp": return "phone.fill"
        case "slack": return "number"
        case "email": return "envelope.fill"
        default: return "bubble.left.fill"
        }
    }

    private func channelColor(for channel: String) -> Color {
        switch channel.lowercased() {
        case "telegram": return .blue
        case "whatsapp": return .green
        case "slack": return .purple
        case "email": return .gray
        default: return .secondary
        }
    }

    private func confidenceColor(for confidence: Float) -> Color {
        if confidence >= 0.8 { return .green }
        if confidence >= 0.5 { return .orange }
        return .red
    }
}
