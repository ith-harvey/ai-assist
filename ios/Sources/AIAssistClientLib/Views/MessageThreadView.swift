import SwiftUI

/// Shows the top card's conversation as a chat-style message thread.
struct MessageThreadView: View {
    let card: ReplyCard?

    var body: some View {
        if let card {
            ScrollView {
                VStack(spacing: 16) {
                    threadHeader(card: card)
                    incomingBubble(card: card)
                    outgoingBubble(card: card)
                }
                .padding(.horizontal, 16)
                .padding(.top, 8)
                .padding(.bottom, 16)
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

    private func incomingBubble(card: ReplyCard) -> some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 4) {
                Text(card.sourceSender)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(card.sourceMessage)
                    .font(.body)
            }
            .padding(12)
            .background(Color.gray.opacity(0.15))
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .frame(maxWidth: 280, alignment: .leading)

            Spacer(minLength: 40)
        }
    }

    private func outgoingBubble(card: ReplyCard) -> some View {
        HStack(alignment: .top) {
            Spacer(minLength: 40)

            VStack(alignment: .trailing, spacing: 4) {
                Text("AI Suggestion")
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.7))
                Text(card.suggestedReply)
                    .font(.body)
                    .foregroundStyle(.white)
            }
            .padding(12)
            .background(Color.blue)
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .frame(maxWidth: 280, alignment: .trailing)
        }
    }

    // MARK: - Helpers

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
