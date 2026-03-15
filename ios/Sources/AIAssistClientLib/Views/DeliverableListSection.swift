import SwiftUI

/// A section showing deliverables (documents + message drafts) linked to a todo.
/// Replaces the old `DocumentListSection` with a unified deliverable view.
///
/// Document rows open `DocumentDetailView`. Message rows open `SwipeCardContainer`
/// for approval. Dismissed messages remain visible but dimmed.
public struct DeliverableListSection: View {
    let todoId: UUID
    let cardSocket: CardWebSocket

    @State private var api: DeliverableAPI
    @State private var selectedDocument: Document?
    @State private var selectedCard: ApprovalCard?

    public init(todoId: UUID, cardSocket: CardWebSocket) {
        self.todoId = todoId
        self.cardSocket = cardSocket
        self._api = State(initialValue: DeliverableAPI(host: cardSocket.host, port: cardSocket.port))
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if api.isLoading {
                HStack {
                    ProgressView()
                        .controlSize(.small)
                    Text("Loading deliverables…")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            } else if let error = api.error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            } else if api.deliverables.isEmpty {
                // Show nothing if no deliverables
            } else {
                // Section header
                HStack {
                    Image(systemName: "tray.full.fill")
                        .foregroundStyle(.blue)
                    Text("Deliverables")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.primary)
                    Spacer()
                    Text("\(api.deliverables.count)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(.fill.tertiary, in: Capsule())
                }

                ForEach(api.deliverables) { item in
                    deliverableRow(item)
                }
            }
        }
        .task {
            await api.fetchDeliverables(forTodoId: todoId)
        }
        .sheet(item: $selectedDocument) { doc in
            DocumentDetailView(document: doc)
        }
        .sheet(item: $selectedCard) { card in
            SwipeCardContainer(
                onApprove: {
                    cardSocket.approve(cardId: card.id)
                    selectedCard = nil
                },
                onReject: {
                    cardSocket.dismiss(cardId: card.id)
                    selectedCard = nil
                }
            ) {
                CardBodyView(card: card)
            }
            .presentationDetents([.medium, .large])
        }
    }

    @ViewBuilder
    private func deliverableRow(_ item: DeliverableItem) -> some View {
        switch item {
        case .document(let doc):
            Button {
                selectedDocument = doc
            } label: {
                DeliverableRowView(item: item)
            }
            .buttonStyle(.plain)

        case .message(let card):
            if item.isDismissed || item.isSent {
                // Dismissed/sent messages: visible but dimmed, non-interactive
                DeliverableRowView(item: item)
                    .opacity(0.5)
            } else {
                Button {
                    selectedCard = card
                } label: {
                    DeliverableRowView(item: item)
                }
                .buttonStyle(.plain)
            }
        }
    }
}

/// A single deliverable row — used for both documents and messages.
private struct DeliverableRowView: View {
    let item: DeliverableItem

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: item.iconName)
                .font(.title3)
                .foregroundStyle(item.iconColor)
                .frame(width: 28)

            VStack(alignment: .leading, spacing: 2) {
                Text(item.title)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(1)

                HStack(spacing: 4) {
                    Text(item.subtitle)
                        .font(.caption2)
                        .foregroundStyle(.secondary)

                    if item.isDismissed {
                        Text("· Dismissed")
                            .font(.caption2)
                            .foregroundStyle(.red.opacity(0.7))
                    } else if item.isSent {
                        Text("· Sent")
                            .font(.caption2)
                            .foregroundStyle(.green)
                    }
                }
            }

            Spacer()

            if !item.isDismissed && !item.isSent {
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 6)
        .padding(.horizontal, 10)
        #if os(iOS)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 8))
        #else
        .background(Color.gray.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
        #endif
    }
}
