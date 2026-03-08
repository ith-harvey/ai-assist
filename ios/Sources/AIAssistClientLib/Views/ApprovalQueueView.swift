import SwiftUI

// MARK: - Approval Sheet Mode

/// Distinguishes queue (from NextStepsButton) vs single card (from double-tap).
enum ApprovalSheetMode: Identifiable {
    case queue
    case single(ApprovalCard)

    var id: String {
        switch self {
        case .queue: return "queue"
        case .single(let card): return card.id.uuidString
        }
    }
}

// MARK: - Card Flip Transition

struct FlipModifier: ViewModifier {
    let angle: Double
    func body(content: Content) -> some View {
        content
            .rotation3DEffect(.degrees(angle), axis: (x: 0, y: 1, z: 0))
            .opacity(abs(angle) > 45 ? 0 : 1)
    }
}

extension AnyTransition {
    static var cardFlip: AnyTransition {
        .asymmetric(
            insertion: .modifier(active: FlipModifier(angle: -90), identity: FlipModifier(angle: 0)),
            removal: .identity // SwipeCardContainer handles fly-off
        )
    }
}

// MARK: - Approval Queue View

struct ApprovalQueueView: View {
    let cardSocket: CardWebSocket
    let mode: ApprovalSheetMode
    let onDismiss: () -> Void

    @State private var processedCount: Int = 0
    @State private var initialQueueSize: Int = 0

    private var currentCard: ApprovalCard? {
        switch mode {
        case .queue:
            return cardSocket.cards.first
        case .single(let card):
            return cardSocket.cards.first(where: { $0.id == card.id })
        }
    }

    private var isQueueMode: Bool {
        if case .queue = mode { return true }
        return false
    }

    var body: some View {
        VStack(spacing: 0) {
            // Progress header (queue mode only)
            if isQueueMode {
                progressHeader
            }

            // Card content
            if let card = currentCard {
                SwipeCardContainer(
                    onApprove: { handleAction { cardSocket.approve(cardId: card.id) } },
                    onReject: { handleAction { cardSocket.dismiss(cardId: card.id) } }
                ) {
                    CardBodyView(card: card)
                }
                .id(card.id)
                .transition(.cardFlip)
            }
        }
        .animation(.easeInOut(duration: 0.35), value: currentCard?.id)
        .onAppear {
            if isQueueMode {
                initialQueueSize = cardSocket.cards.count
            }
        }
        .onChange(of: currentCard == nil) { _, isEmpty in
            if isEmpty {
                onDismiss()
            }
        }
    }

    // MARK: - Progress Header

    private var progressHeader: some View {
        VStack(spacing: 8) {
            Text("\(processedCount + 1) of \(initialQueueSize)")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.secondary)

            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 3)
                        .fill(Color.gray.opacity(0.2))

                    RoundedRectangle(cornerRadius: 3)
                        .fill(Color.orange)
                        .frame(width: geo.size.width * progress)
                        .animation(.easeInOut(duration: 0.3), value: progress)
                }
            }
            .frame(height: 6)
        }
        .padding(.horizontal, 20)
        .padding(.top, 16)
        .padding(.bottom, 4)
    }

    private var progress: CGFloat {
        guard initialQueueSize > 0 else { return 0 }
        return CGFloat(processedCount) / CGFloat(initialQueueSize)
    }

    // MARK: - Action Handler

    private func handleAction(_ action: () -> Void) {
        action()
        processedCount += 1

        if !isQueueMode {
            onDismiss()
        }
    }
}
