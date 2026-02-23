import SwiftUI

/// PreferenceKey that reports how far the user has overscrolled past the bottom.
/// Positive values mean the user is pulling down past the end of content (rubber-band).
/// Zero or negative means normal scrolling (not past bottom).
///
/// Fallback for iOS < 18. On iOS 18+ we use `onScrollGeometryChange` instead.
public struct OverscrollDistanceKey: PreferenceKey {
    public static let defaultValue: CGFloat = 0
    public static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

/// PreferenceKey to capture the scroll viewport height from an overlay on the ScrollView.
///
/// Fallback for iOS < 18.
public struct ViewportHeightKey: PreferenceKey {
    public static let defaultValue: CGFloat = 0
    public static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

/// ViewModifier that uses iOS 18+ `onScrollGeometryChange` and `onScrollPhaseChange`
/// to report overscroll distance and user interaction state. Falls back to a no-op
/// on iOS < 18 (where PreferenceKey-based reporting is used instead).
///
/// Shared between MessageThreadView (card refine) and BrainChatView (voice-to-chat).
public struct ScrollOverscrollModifier: ViewModifier {
    @Binding var overscrollDistance: CGFloat
    @Binding var isUserInteracting: Bool

    public init(overscrollDistance: Binding<CGFloat>, isUserInteracting: Binding<Bool>) {
        self._overscrollDistance = overscrollDistance
        self._isUserInteracting = isUserInteracting
    }

    public func body(content: Content) -> some View {
        if #available(iOS 18.0, macOS 15.0, *) {
            content
                .onScrollGeometryChange(for: CGFloat.self) { geo in
                    // Raw overscroll past the bottom of content.
                    // iOS rubber-band dampens finger movement ~4-6x,
                    // so we amplify to approximate actual finger travel.
                    let scrolledTo = geo.contentOffset.y + geo.containerSize.height
                    let contentEnd = geo.contentSize.height + geo.contentInsets.bottom
                    return max(0, scrolledTo - contentEnd) * 6.0
                } action: { _, newOverscroll in
                    overscrollDistance = newOverscroll
                }
                .onScrollPhaseChange { _, newPhase in
                    isUserInteracting = (newPhase == .interacting)
                }
        } else {
            // iOS < 18: no-op. PreferenceKey path handles overscroll reporting.
            content
        }
    }
}
