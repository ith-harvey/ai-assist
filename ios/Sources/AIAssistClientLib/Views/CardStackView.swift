// COMMENTED OUT: Preserved for future use. Replaced by full-screen swipe in ContentView.
/*
import SwiftUI

/// Displays a fanned-deck stack of cards.
/// Front card is fully focused and interactive; cards behind fan out at slight angles.
public struct CardStackView: View {
    @Bindable var socket: CardWebSocket
    @State private var dragOffset: CGSize = .zero
    @State private var showEditSheet = false
    @State private var editText = ""
    @State private var editCardId: UUID?

    private let swipeThreshold: CGFloat = 100

    public init(socket: CardWebSocket) {
        self.socket = socket
    }

    public var body: some View {
        ZStack {
            if socket.cards.isEmpty {
                emptyState
            } else {
                cardStack
            }
        }
        .sheet(isPresented: $showEditSheet) {
            editSheet
        }
    }

    // MARK: - Card Stack

    private var cardStack: some View {
        let visibleCards = Array(socket.cards.prefix(5).enumerated())

        return ZStack {
            ForEach(visibleCards, id: \.element.id) { index, card in
                let isTop = index == 0

                CardView(card: card, dragOffset: isTop ? dragOffset.width : 0)
                    .scaleEffect(isTop ? 1.0 : 1.0 - Double(index) * 0.04)
                    .blur(radius: isTop ? 0 : Double(index) * 5)
                    .opacity(isTop ? 1.0 : max(0.5, 1.0 - Double(index) * 0.12))
                    .offset(x: isTop ? dragOffset.width : CGFloat(index) * 6,
                            y: isTop ? 0 : CGFloat(index) * 10)
                    .rotationEffect(.degrees(
                        isTop ? Double(dragOffset.width) / 20 : Double(index) * 5
                    ))
                    .zIndex(Double(socket.cards.count - index))
                    .gesture(isTop ? dragGesture(for: card) : nil)
                    .onTapGesture {
                        if isTop {
                            editCardId = card.id
                            editText = card.suggestedReply
                            showEditSheet = true
                        }
                    }
                    .animation(.spring(response: 0.3, dampingFraction: 0.7), value: dragOffset)
                    .allowsHitTesting(isTop)
            }
        }
        .padding(.horizontal, 20)
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "tray")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No pending cards")
                .font(.title3)
                .foregroundStyle(.secondary)
            Text("New reply suggestions will appear here")
                .font(.subheadline)
                .foregroundStyle(.tertiary)
        }
    }

    // MARK: - Edit Sheet

    private var editSheet: some View {
        NavigationStack {
            Form {
                Section("Edit Reply") {
                    TextEditor(text: $editText)
                        .frame(minHeight: 100)
                }
            }
            .navigationTitle("Edit & Send")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        showEditSheet = false
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") {
                        if let cardId = editCardId {
                            socket.edit(cardId: cardId, newText: editText)
                        }
                        showEditSheet = false
                    }
                    .fontWeight(.semibold)
                }
            }
        }
        .presentationDetents([.medium])
    }

    // MARK: - Gestures

    private func dragGesture(for card: ApprovalCard) -> some Gesture {
        DragGesture()
            .onChanged { value in
                dragOffset = value.translation
            }
            .onEnded { value in
                let width = value.translation.width
                if width > swipeThreshold {
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                        dragOffset = CGSize(width: 500, height: 0)
                    }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
                        withAnimation(.spring(response: 0.45, dampingFraction: 0.8)) {
                            socket.approve(cardId: card.id)
                            dragOffset = .zero
                        }
                    }
                } else if width < -swipeThreshold {
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                        dragOffset = CGSize(width: -500, height: 0)
                    }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
                        withAnimation(.spring(response: 0.45, dampingFraction: 0.8)) {
                            socket.dismiss(cardId: card.id)
                            dragOffset = .zero
                        }
                    }
                } else {
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                        dragOffset = .zero
                    }
                }
            }
    }
}
*/
