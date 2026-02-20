#if canImport(UIKit)
import SwiftUI
import UIKit

/// A touch-passthrough UIView that lets its gesture recognizers observe touches
/// without blocking the views underneath.
///
/// `hitTest` returns `nil` so UIKit doesn't consider this view the "hit" target —
/// touches fall through to the ScrollView (or whatever is below). But gesture
/// recognizers attached to this view still get to observe the touch sequence because
/// UIKit delivers touches to recognizers on the entire chain, not just the hit view.
private class PassthroughView: UIView {
    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        return nil
    }
}

/// A transparent UIKit overlay that captures horizontal pan gestures with zero lag.
///
/// This solves the fundamental problem: SwiftUI's `DragGesture` on a view containing
/// a `ScrollView` always has delay because the ScrollView's built-in pan recognizer
/// gets priority and SwiftUI must disambiguate. By using a UIKit `UIPanGestureRecognizer`
/// with a custom delegate, we claim horizontal gestures immediately at the UIKit level,
/// before SwiftUI's gesture system even sees them.
///
/// Vertical gestures pass through to the ScrollView normally because:
/// 1. `PassthroughView.hitTest` returns nil — touches reach the ScrollView
/// 2. Our pan recognizer runs simultaneously via delegate
/// 3. On vertical movement we cancel our recognizer — ScrollView keeps full control
struct HorizontalSwipeGesture: UIViewRepresentable {
    /// Called on every pan movement with the horizontal translation (points).
    var onChanged: (CGFloat) -> Void
    /// Called when the gesture ends with (final translation, predicted velocity.x).
    var onEnded: (CGFloat, CGFloat) -> Void

    func makeUIView(context: Context) -> UIView {
        let view = PassthroughView()
        view.backgroundColor = .clear

        let pan = UIPanGestureRecognizer(
            target: context.coordinator,
            action: #selector(Coordinator.handlePan(_:))
        )
        pan.delegate = context.coordinator
        // Cancel touches in view = NO means the ScrollView still gets them
        pan.cancelsTouchesInView = false
        // Delay touches = NO means the ScrollView gets touches immediately
        pan.delaysTouchesBegan = false
        pan.delaysTouchesEnded = false
        view.addGestureRecognizer(pan)

        return view
    }

    func updateUIView(_ uiView: UIView, context: Context) {
        context.coordinator.onChanged = onChanged
        context.coordinator.onEnded = onEnded
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(onChanged: onChanged, onEnded: onEnded)
    }

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        var onChanged: (CGFloat) -> Void
        var onEnded: (CGFloat, CGFloat) -> Void
        private var isLockedHorizontal = false

        init(onChanged: @escaping (CGFloat) -> Void, onEnded: @escaping (CGFloat, CGFloat) -> Void) {
            self.onChanged = onChanged
            self.onEnded = onEnded
        }

        @objc func handlePan(_ gesture: UIPanGestureRecognizer) {
            guard let view = gesture.view else { return }
            let translation = gesture.translation(in: view)

            switch gesture.state {
            case .began:
                isLockedHorizontal = false

            case .changed:
                // Lock direction on first significant movement
                if !isLockedHorizontal {
                    let absX = abs(translation.x)
                    let absY = abs(translation.y)
                    guard absX > 6 || absY > 6 else { return }

                    if absX > absY {
                        isLockedHorizontal = true
                    } else {
                        // Vertical — cancel this recognizer, let ScrollView keep full control
                        gesture.state = .cancelled
                        return
                    }
                }

                onChanged(translation.x)

            case .ended, .cancelled:
                if isLockedHorizontal {
                    let velocity = gesture.velocity(in: view)
                    onEnded(translation.x, velocity.x)
                }
                isLockedHorizontal = false

            default:
                break
            }
        }

        // MARK: - UIGestureRecognizerDelegate

        /// Allow this gesture to recognize simultaneously with the ScrollView's pan.
        /// Both recognizers track the touch; direction lock determines which one "wins."
        func gestureRecognizer(
            _ gestureRecognizer: UIGestureRecognizer,
            shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
        ) -> Bool {
            return true
        }

        /// Only begin when initial velocity is more horizontal than vertical.
        /// This prevents the recognizer from even starting on clearly vertical swipes.
        func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
            guard let pan = gestureRecognizer as? UIPanGestureRecognizer,
                  let view = pan.view else { return true }
            let velocity = pan.velocity(in: view)
            // Allow if horizontal velocity dominates, or if not moving yet (let direction lock decide)
            return abs(velocity.x) >= abs(velocity.y) || (velocity.x == 0 && velocity.y == 0)
        }
    }
}
#endif
