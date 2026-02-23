import SwiftUI

/// Shows the email conversation thread as iMessage-style chat bubbles.
///
/// Priority: emailThread (rich headers) > thread (generic) > single message fallback.
/// Auto-scrolls to the bottom (newest messages). The AI suggested reply appears
/// as a faded/dashed "Draft" bubble at the end.
struct MessageThreadView: View {
    let card: ReplyCard?

    var body: some View {
        if let card {
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(spacing: 12) {
                        threadHeader(card: card)

                        if !card.emailThread.isEmpty {
                            // Rich email thread with headers
                            ForEach(card.emailThread) { msg in
                                if msg.isOutgoing {
                                    outgoingEmailBubble(msg: msg)
                                } else {
                                    incomingEmailBubble(msg: msg)
                                }
                            }
                            draftBubble(reply: card.suggestedReply)
                                .id("draft")
                        } else if !card.thread.isEmpty {
                            // Generic thread (no headers)
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
                        } else {
                            // Fallback: no thread context
                            incomingBubble(
                                sender: card.sourceSender,
                                content: card.sourceMessage,
                                timestamp: nil
                            )
                            draftBubble(reply: card.suggestedReply)
                                .id("draft")
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 8)
                    .padding(.bottom, 16)
                }
                .scrollDismissesKeyboard(.interactively)
                .onAppear {
                    if !card.emailThread.isEmpty || !card.thread.isEmpty {
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
        VStack(alignment: .leading, spacing: 6) {
            // Top row: channel icon + subject (bold title) + confidence badge
            HStack(spacing: 8) {
                Image(systemName: channelIcon(for: card.channel))
                    .font(.body)
                    .foregroundStyle(channelColor(for: card.channel))
                Text(card.conversationId)
                    .font(.title3.bold())
                    .foregroundStyle(.primary)
                    .lineLimit(2)
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

            // To/CC from the latest email in the thread
            if let latest = card.emailThread.last {
                VStack(alignment: .leading, spacing: 1) {
                    if !latest.to.isEmpty {
                        Text("To: \(latest.to.joined(separator: ", "))")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                    if !latest.cc.isEmpty {
                        Text("CC: \(latest.cc.joined(separator: ", "))")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                }
            }
        }
        .padding(.horizontal, 4)
    }

    // MARK: - Email Message Header

    @ViewBuilder
    private func messageHeader(from: String, to: [String], cc: [String], outgoing: Bool) -> some View {
        let headerColor: Color = outgoing ? .white.opacity(0.6) : .secondary
        VStack(alignment: .leading, spacing: 1) {
            Text("From: \(from)")
                .font(.caption2)
                .foregroundStyle(headerColor)
                .lineLimit(1)
                .truncationMode(.tail)
            if !to.isEmpty {
                Text("To: \(to.joined(separator: ", "))")
                    .font(.caption2)
                    .foregroundStyle(headerColor)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            if !cc.isEmpty {
                Text("CC: \(cc.joined(separator: ", "))")
                    .font(.caption2)
                    .foregroundStyle(headerColor)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
        }
    }

    // MARK: - Email Bubbles

    private func incomingEmailBubble(msg: EmailMessage) -> some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 4) {
                messageHeader(from: msg.from, to: msg.to, cc: msg.cc, outgoing: false)
                Text(msg.content)
                    .font(.body)
                Text(formatTimestamp(msg.timestamp))
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            .padding(12)
            .background(Color.gray.opacity(0.15))
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .frame(maxWidth: 280, alignment: .leading)

            Spacer(minLength: 40)
        }
    }

    private func outgoingEmailBubble(msg: EmailMessage) -> some View {
        HStack(alignment: .top) {
            Spacer(minLength: 40)

            VStack(alignment: .trailing, spacing: 4) {
                messageHeader(from: "You", to: msg.to, cc: msg.cc, outgoing: true)
                Text(msg.content)
                    .font(.body)
                    .foregroundStyle(.white)
                Text(formatTimestamp(msg.timestamp))
                    .font(.caption2)
                    .foregroundStyle(.white.opacity(0.6))
            }
            .padding(12)
            .background(Color.blue)
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .frame(maxWidth: 280, alignment: .trailing)
        }
    }

    // MARK: - Generic Bubbles

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

    // MARK: - Draft Bubble

    private func draftBubble(reply: String) -> some View {
        HStack(alignment: .top) {
            Spacer(minLength: 40)

            VStack(alignment: .trailing, spacing: 4) {
                Text("AI Suggestion")
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(.blue)
                Text(reply)
                    .font(.body)
                    .foregroundStyle(.primary.opacity(0.85))
            }
            .padding(12)
            .background(Color.blue.opacity(0.18))
            .overlay(
                RoundedRectangle(cornerRadius: 16)
                    .stroke(style: StrokeStyle(lineWidth: 1.5, dash: [6, 3]))
                    .foregroundStyle(.blue.opacity(0.6))
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
