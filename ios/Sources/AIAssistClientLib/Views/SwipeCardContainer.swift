import SwiftUI

// MARK: - Swipe Card Container

/// Generic swipe-to-approve/reject gesture wrapper.
///
/// Wraps any card content view. Handles horizontal drag with direction lock,
/// green/red overlay feedback, fly-off animation, and approve/reject callbacks.
/// Vertical scrolling inside the content is unaffected.
struct SwipeCardContainer<Content: View>: View {
    let onApprove: () -> Void
    let onReject: () -> Void
    let approveDisabled: Bool
    @ViewBuilder let content: () -> Content

    @State private var dragOffset: CGFloat = 0
    @State private var isDraggingHorizontally = false

    private let swipeThreshold: CGFloat = 100
    private let directionLockDistance: CGFloat = 20

    init(
        onApprove: @escaping () -> Void,
        onReject: @escaping () -> Void,
        approveDisabled: Bool = false,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.onApprove = onApprove
        self.onReject = onReject
        self.approveDisabled = approveDisabled
        self.content = content
    }

    var body: some View {
        ZStack {
            // Card chrome: rounded rect + shadow
            VStack(spacing: 0) {
                content()
            }
            .background(
                RoundedRectangle(cornerRadius: 20)
                    #if os(iOS)
                    .fill(Color(uiColor: .systemBackground))
                    #else
                    .fill(Color.white)
                    #endif
            )
            .clipShape(RoundedRectangle(cornerRadius: 20))
            .shadow(color: .black.opacity(0.1), radius: 12, y: 4)
            // Swipe color overlay
            .overlay(
                swipeColorOverlay
                    .clipShape(RoundedRectangle(cornerRadius: 20))
            )
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .offset(x: dragOffset)
        .rotationEffect(.degrees(isDraggingHorizontally ? Double(dragOffset) / 25 : 0))
        .simultaneousGesture(
            DragGesture(minimumDistance: directionLockDistance)
                .onChanged { value in
                    let horizontal = abs(value.translation.width)
                    let vertical = abs(value.translation.height)

                    if !isDraggingHorizontally {
                        if horizontal > vertical && horizontal > directionLockDistance {
                            isDraggingHorizontally = true
                        }
                    }

                    if isDraggingHorizontally {
                        let width = value.translation.width
                        // When approveDisabled, only allow left (negative) swipe
                        dragOffset = approveDisabled ? min(0, width) : width
                    }
                }
                .onEnded { value in
                    if isDraggingHorizontally {
                        let width = value.translation.width
                        let velocityX = value.predictedEndTranslation.width - width
                        let effectiveWidth = width + velocityX * 0.15

                        if effectiveWidth > swipeThreshold {
                            withAnimation(.easeOut(duration: 0.18)) {
                                dragOffset = 500
                            }
                            DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) {
                                onApprove()
                                dragOffset = 0
                                isDraggingHorizontally = false
                            }
                        } else if effectiveWidth < -swipeThreshold {
                            withAnimation(.easeOut(duration: 0.18)) {
                                dragOffset = -500
                            }
                            DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) {
                                onReject()
                                dragOffset = 0
                                isDraggingHorizontally = false
                            }
                        } else {
                            withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                                dragOffset = 0
                            }
                            isDraggingHorizontally = false
                        }
                    } else {
                        isDraggingHorizontally = false
                    }
                }
        )
    }

    // MARK: - Swipe Color Overlay

    @ViewBuilder
    private var swipeColorOverlay: some View {
        let width = dragOffset
        ZStack {
            if width > 10 {
                let intensity = min(0.85, Double(width - 10) / 200)
                Color.green.opacity(intensity)
                VStack(spacing: 8) {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 48, weight: .bold))
                    Text("Approved")
                        .font(.title2.bold())
                }
                .foregroundStyle(.white)
                .opacity(min(1.0, Double(width - 30) / 80))
            } else if width < -10 {
                let intensity = min(0.85, Double(abs(width) - 10) / 200)
                Color.red.opacity(intensity)
                VStack(spacing: 8) {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 48, weight: .bold))
                    Text("Rejected")
                        .font(.title2.bold())
                }
                .foregroundStyle(.white)
                .opacity(min(1.0, Double(abs(width) - 30) / 80))
            }
        }
        .allowsHitTesting(false)
    }
}
