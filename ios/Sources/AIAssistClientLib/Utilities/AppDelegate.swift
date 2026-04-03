#if canImport(UIKit)
import UIKit

/// UIApplicationDelegate adapter for receiving remote notification registration callbacks.
/// Wire this into the SwiftUI app via `@UIApplicationDelegateAdaptor`.
/// Buffers the token if it arrives before notificationManager is set, preventing silent drops.
public final class AppDelegate: NSObject, UIApplicationDelegate {

    /// Set by the SwiftUI app so callbacks can forward to the shared manager.
    public var notificationManager: NotificationManager? {
        didSet {
            guard let notificationManager, let pending = pendingToken else { return }
            pendingToken = nil
            Task { @MainActor in
                notificationManager.didRegisterForRemoteNotifications(deviceToken: pending)
            }
        }
    }

    /// Buffered token in case APNS responds before notificationManager is wired.
    private var pendingToken: Data?

    public func application(
        _ application: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        if let notificationManager {
            Task { @MainActor in
                notificationManager.didRegisterForRemoteNotifications(deviceToken: deviceToken)
            }
        } else {
            pendingToken = deviceToken
        }
    }

    public func application(
        _ application: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: Error
    ) {
        if let notificationManager {
            Task { @MainActor in
                notificationManager.didFailToRegisterForRemoteNotifications(error: error)
            }
        } else {
            print("[AppDelegate] APNS registration failed before manager was set: \(error)")
        }
    }
}
#endif
