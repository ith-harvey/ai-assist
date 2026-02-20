// COMMENTED OUT: Preserved for future use. Replaced by full-screen swipe in ContentView.
/*
import SwiftUI

/// Displays a single reply suggestion card.
public struct CardView: View {
    let card: ReplyCard
    let dragOffset: CGFloat

    public init(card: ReplyCard, dragOffset: CGFloat) {
        self.card = card
        self.dragOffset = dragOffset
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header: sender + channel icon
            HStack {
                Text(card.sourceSender)
                    .font(.headline)
                Spacer()
                Image(systemName: channelIcon)
                    .font(.title3)
                    .foregroundStyle(channelColor)
            }

            // Source message
            Text(card.sourceMessage)
                .font(.body)
                .foregroundStyle(.secondary)
                .lineLimit(3)

            Divider()

            // Suggested reply
            Text(card.suggestedReply)
                .font(.body)
                .fontWeight(.medium)

            // Confidence indicator (compact)
            HStack(spacing: 4) {
                Circle()
                    .fill(confidenceColor)
                    .frame(width: 6, height: 6)
                Text("\(Int(card.confidence * 100))%")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                    .monospacedDigit()
            }
        }
        .padding(20)
        .background(
            .ultraThinMaterial,
            in: RoundedRectangle(cornerRadius: 16)
        )
        .shadow(color: .black.opacity(0.15), radius: 12, y: 6)
        .overlay(
            RoundedRectangle(cornerRadius: 16)
                .fill(swipeTintColor)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16)
                .stroke(borderColor, lineWidth: borderWidth)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16)
                .stroke(.white.opacity(0.3), lineWidth: 0.5)
        )
        .overlay(swipeLabel)
        .clipShape(RoundedRectangle(cornerRadius: 16))
    }

    // MARK: - Swipe Overlays

    @ViewBuilder
    private var swipeLabel: some View {
        if dragOffset > 30 {
            Text("APPROVE")
                .font(.system(size: 36, weight: .black))
                .foregroundStyle(.green)
                .rotationEffect(.degrees(-15))
                .opacity(Double(min(1, (dragOffset - 30) / 70)))
                .padding(8)
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(.green, lineWidth: 3)
                        .opacity(Double(min(1, (dragOffset - 30) / 70)))
                )
        } else if dragOffset < -30 {
            Text("REJECT")
                .font(.system(size: 36, weight: .black))
                .foregroundStyle(.red)
                .rotationEffect(.degrees(15))
                .opacity(Double(min(1, (abs(dragOffset) - 30) / 70)))
                .padding(8)
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(.red, lineWidth: 3)
                        .opacity(Double(min(1, (abs(dragOffset) - 30) / 70)))
                )
        }
    }

    private var swipeTintColor: Color {
        if dragOffset > 30 {
            return .green.opacity(Double(min(0.15, (dragOffset - 30) / 500)))
        } else if dragOffset < -30 {
            return .red.opacity(Double(min(0.15, (abs(dragOffset) - 30) / 500)))
        }
        return .clear
    }

    private var channelIcon: String {
        switch card.channel.lowercased() {
        case "telegram": return "paperplane.fill"
        case "whatsapp": return "phone.fill"
        case "slack": return "number"
        case "email": return "envelope.fill"
        default: return "bubble.left.fill"
        }
    }

    private var channelColor: Color {
        switch card.channel.lowercased() {
        case "telegram": return .blue
        case "whatsapp": return .green
        case "slack": return .purple
        case "email": return .gray
        default: return .secondary
        }
    }

    private var confidenceColor: Color {
        if card.confidence >= 0.8 { return .green }
        if card.confidence >= 0.5 { return .orange }
        return .red
    }

    private var borderColor: Color {
        if dragOffset > 30 { return .green.opacity(0.5) }
        if dragOffset < -30 { return .red.opacity(0.5) }
        return .clear
    }

    private var borderWidth: CGFloat {
        abs(dragOffset) > 30 ? 2 : 0
    }
}
*/
