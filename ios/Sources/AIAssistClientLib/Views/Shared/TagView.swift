import SwiftUI

/// Reusable tag component with two styles:
///
/// - **Capsule** (default): colored background pill, e.g. "Deliverable", "Research"
/// - **Inline**: icon + text with shared foreground color, e.g. "🔴 High", "🤖 Agent"
///
/// Usage:
/// ```
/// TagView.capsule("Deliverable", color: .blue)
/// TagView.inline("High", icon: "exclamationmark.circle.fill", color: .red)
/// TagView.inline("Agent", icon: "cpu", color: .blue)
/// ```
struct TagView: View {
    let label: String
    let color: Color
    var icon: String? = nil
    var style: Style = .capsule
    var fontSize: CGFloat = 11

    enum Style {
        case capsule
        case inline
    }

    var body: some View {
        switch style {
        case .capsule:
            capsuleBody
        case .inline:
            inlineBody
        }
    }

    private var capsuleBody: some View {
        HStack(spacing: 3) {
            if let icon {
                Image(systemName: icon)
                    .font(.system(size: fontSize))
            }
            Text(label)
                .font(.system(size: fontSize, weight: .medium))
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(color.opacity(0.15))
        .foregroundStyle(color)
        .clipShape(Capsule())
    }

    private var inlineBody: some View {
        HStack(spacing: 2) {
            if let icon {
                Image(systemName: icon)
                    .font(.system(size: fontSize - 2))
            }
            Text(label)
                .font(.system(size: fontSize, weight: .medium))
        }
        .foregroundStyle(color)
    }

    // MARK: - Factory Methods

    static func capsule(_ label: String, color: Color, icon: String? = nil, fontSize: CGFloat = 11) -> TagView {
        TagView(label: label, color: color, icon: icon, style: .capsule, fontSize: fontSize)
    }

    static func inline(_ label: String, icon: String? = nil, color: Color, fontSize: CGFloat = 11) -> TagView {
        TagView(label: label, color: color, icon: icon, style: .inline, fontSize: fontSize)
    }
}

// MARK: - Model Convenience Extensions

extension TodoType {
    /// Standard capsule tag for this todo type.
    func tag(fontSize: CGFloat = 11) -> TagView {
        .capsule(label, color: badgeColor, fontSize: fontSize)
    }
}

extension TodoBucket {
    /// Inline icon+text tag for agent/human bucket.
    func tag(fontSize: CGFloat = 11) -> TagView {
        switch self {
        case .agentStartable:
            .inline("Agent", icon: "cpu", color: .blue, fontSize: fontSize)
        case .humanOnly:
            .inline("Human", icon: "person.fill", color: .purple, fontSize: fontSize)
        }
    }
}

extension TodoItem {
    /// Inline priority tag (only for priority 1-2).
    func priorityTag(fontSize: CGFloat = 11) -> TagView? {
        switch priority {
        case 1: .inline("High", icon: "exclamationmark.circle.fill", color: .red, fontSize: fontSize)
        case 2: .inline("Medium", icon: "exclamationmark.circle.fill", color: .orange, fontSize: fontSize)
        default: nil
        }
    }
}
