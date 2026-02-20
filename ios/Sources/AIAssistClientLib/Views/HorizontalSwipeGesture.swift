#if canImport(UIKit)
import SwiftUI
import UIKit

/// A transparent UIKit overlay that captures horizontal pan gestures with zero lag.
///
/// This solves the fundamental problem: SwiftUI's `DragGesture` on a view containing
/// a `ScrollView` always has delay because the ScrollView's built-in pan recognizer
/// gets priority and SwiftUI must disambiguate. By using a UIKit `UIPanGestureRecognizer`
/// with a custom delegate, we claim horizontal gestures immediately at the UIKit level,
/// before SwiftUI's gesture system even sees them.
///
/// Vertical gestures pass through to the ScrollView normally.
struct HorizontalSwipeGesture: UIViewRepresentable {
    /// Called on every pan movement with the horizontal translation (points).
    var onChanged: (CGFloat) -> Void
    /// Called when the gesture ends with (final translation, predicted velocity.x).
    var onEnded: (CGFloat, CGFloat) -> Void

    func makeUIView(context: Context) -> UIView {
        let view = UIView()
        view.backgroundColor = .clear

        let pan = UIPanGestureRecognizer(
            target: context.coordinator,
            action: #selector(Coordinator.handlePan(_:))
        )
        pan.delegate = context.coordinator
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
                        // Vertical — cancel this recognizer, let ScrollView take over
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
        /// During the direction-lock phase both run; once we lock horizontal we'll
        /// keep claiming events while the ScrollView sees no vertical movement.
        func gestureRecognizer(
            _ gestureRecognizer: UIGestureRecognizer,
            shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
        ) -> Bool {
            return true
        }

        /// Begin recognizing immediately — don't wait for failure of other recognizers.
        func gestureRecognizer(
            _ gestureRecognizer: UIGestureRecognizer,
            shouldBeRequiredToFailBy other: UIGestureRecognizer
        ) -> Bool {
            return false
        }
    }
}
#endif
