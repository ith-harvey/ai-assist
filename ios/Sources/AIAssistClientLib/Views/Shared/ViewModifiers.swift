import SwiftUI

// MARK: - Card Background Modifier

/// Platform-aware card background with rounded corners and shadow.
/// Replaces repeated RoundedRectangle + fill + clipShape + shadow patterns.
struct CardBackgroundModifier: ViewModifier {
    var cornerRadius: CGFloat = 20
    var shadowRadius: CGFloat = 12
    var shadowY: CGFloat = 4

    func body(content: Content) -> some View {
        content
            .background(
                RoundedRectangle(cornerRadius: cornerRadius)
                    #if os(iOS)
                    .fill(Color(uiColor: .systemBackground))
                    #else
                    .fill(Color.white)
                    #endif
            )
            .clipShape(RoundedRectangle(cornerRadius: cornerRadius))
            .shadow(color: .black.opacity(0.1), radius: shadowRadius, y: shadowY)
    }
}

extension View {
    func cardBackground(
        cornerRadius: CGFloat = 20,
        shadowRadius: CGFloat = 12,
        shadowY: CGFloat = 4
    ) -> some View {
        modifier(CardBackgroundModifier(
            cornerRadius: cornerRadius,
            shadowRadius: shadowRadius,
            shadowY: shadowY
        ))
    }
}

// MARK: - Plain Card List Row Modifier

/// Standardizes list row appearance for card-style lists.
/// Hides separators, clears background, and applies consistent insets.
struct PlainCardListRowModifier: ViewModifier {
    func body(content: Content) -> some View {
        content
            .listRowSeparator(.hidden)
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets(top: 5, leading: 14, bottom: 5, trailing: 14))
    }
}

extension View {
    func plainCardListRow() -> some View {
        modifier(PlainCardListRowModifier())
    }
}

// MARK: - Secondary System Background

/// Platform-aware secondary background that ignores safe area.
struct SecondaryBackgroundModifier: ViewModifier {
    func body(content: Content) -> some View {
        content
            .background {
                #if os(iOS)
                Color(uiColor: .secondarySystemBackground)
                    .ignoresSafeArea()
                #else
                Color.gray.opacity(0.08)
                    .ignoresSafeArea()
                #endif
            }
    }
}

extension View {
    func secondaryBackground() -> some View {
        modifier(SecondaryBackgroundModifier())
    }
}

// MARK: - Secondary System Background (inline, non-ignoring)

/// Platform-aware secondary background color for inline use (no ignoresSafeArea).
struct SecondaryFillModifier: ViewModifier {
    func body(content: Content) -> some View {
        content
            #if os(iOS)
            .background(Color(uiColor: .systemGray6))
            #else
            .background(Color.gray.opacity(0.12))
            #endif
    }
}

extension View {
    func secondaryFill() -> some View {
        modifier(SecondaryFillModifier())
    }
}
