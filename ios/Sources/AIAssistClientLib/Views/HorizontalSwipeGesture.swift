#if canImport(UIKit)
import SwiftUI
import UIKit

/// Injects a horizontal `UIPanGestureRecognizer` into the **hosting view's window**
/// and wires it so the ScrollView's built-in pan must wait for ours to fail on
/// horizontal movement. This is the Tinder/Hinge pattern for "swipe whole screen
/// while content scrolls vertically."
///
/// How it works:
/// 1. On `didMoveToWindow`, we walk the view hierarchy to find all `UIScrollView`s.
/// 2. For each ScrollView's `panGestureRecognizer`, we call
///    `require(toFail: ourPan)` — the ScrollView won't start tracking until our
///    recognizer fails (i.e., the gesture is vertical, not horizontal).
/// 3. Our recognizer uses `gestureRecognizerShouldBegin` to ONLY begin when the
///    initial velocity is clearly horizontal. Vertical gestures → we fail immediately
///    → ScrollView takes over with zero delay.
/// 4. `shouldRecognizeSimultaneouslyWith` returns false — exactly one recognizer
///    wins, no fighting.
///
/// Result: horizontal swipes are captured instantly (1:1 tracking), vertical scrolls
/// feel completely native with no lag or dead zones.
struct HorizontalSwipeGesture: UIViewRepresentable {
    var onChanged: (CGFloat) -> Void
    var onEnded: (CGFloat, CGFloat) -> Void

    func makeUIView(context: Context) -> SwipeHostView {
        let view = SwipeHostView()
        view.backgroundColor = .clear
        // Invisible but participates in hit testing (unlike PassthroughView)
        view.isUserInteractionEnabled = true

        let pan = UIPanGestureRecognizer(
            target: context.coordinator,
            action: #selector(Coordinator.handlePan(_:))
        )
        pan.delegate = context.coordinator
        // We handle the gesture; let ScrollView handle touches independently
        pan.cancelsTouchesInView = false
        pan.delaysTouchesBegan = false
        pan.delaysTouchesEnded = false
        view.addGestureRecognizer(pan)

        view.swipePan = pan
        return view
    }

    func updateUIView(_ uiView: SwipeHostView, context: Context) {
        context.coordinator.onChanged = onChanged
        context.coordinator.onEnded = onEnded
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(onChanged: onChanged, onEnded: onEnded)
    }

    // MARK: - SwipeHostView

    /// A real UIView (not passthrough) that wires failure requirements once it's
    /// in the view hierarchy and can find ScrollViews.
    final class SwipeHostView: UIView {
        var swipePan: UIPanGestureRecognizer?
        private var didWire = false

        override func didMoveToWindow() {
            super.didMoveToWindow()
            guard !didWire, let pan = swipePan, window != nil else { return }
            didWire = true

            // Walk up to find the nearest superview, then search down for ScrollViews
            if let root = window {
                wireScrollViews(in: root, toFail: pan)
            }
        }

        // Also re-wire when layout changes (SwiftUI can reparent views)
        override func layoutSubviews() {
            super.layoutSubviews()
            guard let pan = swipePan, let root = window else { return }
            wireScrollViews(in: root, toFail: pan)
        }

        private func wireScrollViews(in view: UIView, toFail pan: UIPanGestureRecognizer) {
            if let scrollView = view as? UIScrollView {
                // Make the ScrollView's pan wait for ours to fail
                // If ours begins (horizontal) → ScrollView's pan won't start
                // If ours fails (vertical) → ScrollView's pan starts immediately
                let scrollPan = scrollView.panGestureRecognizer
                if !scrollPan.description.contains("_wired") {
                    scrollPan.require(toFail: pan)
                }
            }
            for sub in view.subviews {
                wireScrollViews(in: sub, toFail: pan)
            }
        }
    }

    // MARK: - Coordinator

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        var onChanged: (CGFloat) -> Void
        var onEnded: (CGFloat, CGFloat) -> Void
        private var tracking = false

        init(onChanged: @escaping (CGFloat) -> Void, onEnded: @escaping (CGFloat) -> Void) {
            self.onChanged = onChanged
            self.onEnded = onEnded
        }

        @objc func handlePan(_ gesture: UIPanGestureRecognizer) {
            guard let view = gesture.view else { return }
            let translation = gesture.translation(in: view)

            switch gesture.state {
            case .began:
                tracking = true

            case .changed:
                if tracking {
                    onChanged(translation.x)
                }

            case .ended:
                if tracking {
                    let velocity = gesture.velocity(in: view)
                    onEnded(translation.x, velocity.x)
                }
                tracking = false

            case .cancelled, .failed:
                if tracking {
                    onEnded(0, 0) // snap back
                }
                tracking = false

            default:
                break
            }
        }

        // MARK: - UIGestureRecognizerDelegate

        /// Only begin when initial movement is clearly horizontal.
        /// This is the key gatekeeper — if we return false, we "fail" immediately,
        /// and the ScrollView's pan (which required us to fail) starts instantly.
        /// No lag on vertical scrolling.
        func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
            guard let pan = gestureRecognizer as? UIPanGestureRecognizer,
                  let view = pan.view else { return false }
            let velocity = pan.velocity(in: view)
            // Horizontal must be at least 1.2x vertical to claim. This gives vertical
            // a slight edge so scrolling never feels sticky.
            return abs(velocity.x) > abs(velocity.y) * 1.2
        }

        /// Do NOT recognize simultaneously — we want exactly one winner.
        /// Our `require(toFail:)` wiring handles the arbitration.
        func gestureRecognizer(
            _ gestureRecognizer: UIGestureRecognizer,
            shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
        ) -> Bool {
            return false
        }
    }
}
#endif
