import SwiftUI

// MARK: - Channel Style Helpers

/// Shared channel icon/color/label helpers used by card views and thread views.
/// Centralizes the channel → visual mapping that was duplicated in ContentView
/// and MessageThreadView.

/// SF Symbol name for a channel.
func channelIcon(for channel: String) -> String {
    switch channel.lowercased() {
    case "telegram": return "paperplane.fill"
    case "whatsapp": return "phone.fill"
    case "slack": return "number"
    case "email": return "envelope.fill"
    default: return "bubble.left.fill"
    }
}

/// Tint color for a channel header banner.
func channelColor(for channel: String) -> Color {
    switch channel.lowercased() {
    case "telegram": return Color(red: 0.35, green: 0.53, blue: 0.87)
    case "whatsapp": return Color(red: 0.15, green: 0.68, blue: 0.38)
    case "slack": return Color(red: 0.44, green: 0.19, blue: 0.58)
    case "email": return Color(red: 0.35, green: 0.35, blue: 0.42)
    default: return .accentColor
    }
}

/// Human-readable label for a card's channel context.
func channelLabel(for card: ApprovalCard) -> String {
    let channel = card.channel.lowercased()
    switch channel {
    case "email":
        return card.conversationId
    default:
        return "via \(card.channel.capitalized)"
    }
}
