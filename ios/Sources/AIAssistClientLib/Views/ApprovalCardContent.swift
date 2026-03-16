import SwiftUI

// MARK: - Approval Card Content Protocol

/// Defines the contract for card-type-specific content rendered inside `ApprovalQueueView`.
///
/// Each child type provides its unique body, declares whether it supports refinement,
/// and whether right-swipe (approve) is disabled (e.g. multiple-choice cards).
protocol ApprovalCardContent: View {
    /// Whether this card type shows the refine input bar (reply/compose only).
    var supportsRefine: Bool { get }

    /// Whether right-swipe approve is disabled (multiple-choice only).
    var approveDisabled: Bool { get }
}

extension ApprovalCardContent {
    var supportsRefine: Bool { false }
    var approveDisabled: Bool { false }
}

// MARK: - Message Draft Approval Card

/// Card content for `.reply` and `.compose` payloads.
/// Shows the card body, divider, refine input bar, and refining indicator.
struct MessageDraftApprovalCard: ApprovalCardContent {
    let card: ApprovalCard
    let cardSocket: CardWebSocket
    @Binding var refineText: String

    var supportsRefine: Bool { true }

    var body: some View {
        CardBodyView(card: card)

        Divider()

        refineInputBar

        if cardSocket.isRefining {
            refiningBar
        }
    }

    // MARK: - Refine Input Bar

    private var refineInputBar: some View {
        SharedInputBar(
            text: $refineText,
            placeholder: refinePlaceholder,
            lineLimit: 1...3,
            showBackground: false,
            onSend: {
                let text = refineText.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !text.isEmpty else { return }
                cardSocket.refine(cardId: card.id, instruction: text)
                refineText = ""
            },
            onVoiceTranscript: { transcript in
                cardSocket.refine(cardId: card.id, instruction: transcript)
            }
        )
    }

    private var refinePlaceholder: String {
        if case .compose = card.payload {
            return "Refine this draft..."
        }
        return "Refine this reply..."
    }

    // MARK: - Refining Bar

    private var refiningBar: some View {
        HStack(spacing: 8) {
            ProgressView()
                .controlSize(.small)
                .tint(.orange)
            Text("Refining...")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.orange)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
        .background(Color.orange.opacity(0.08))
    }
}

// MARK: - Action Approval Card

/// Card content for `.action` and `.decision` payloads.
/// Shows the card body only — no refine bar.
struct ActionApprovalCard: ApprovalCardContent {
    let card: ApprovalCard

    var body: some View {
        CardBodyView(card: card)
    }
}

// MARK: - Multiple Choice Approval Card

/// Card content for `.multipleChoice` payloads.
/// Renders swipeable option rows. Right-swipe on the container is disabled.
struct MultipleChoiceApprovalCard: ApprovalCardContent {
    let card: ApprovalCard
    let socket: CardWebSocket

    var approveDisabled: Bool { true }

    var body: some View {
        MultipleChoiceCardBody(card: card, socket: socket)
    }
}
