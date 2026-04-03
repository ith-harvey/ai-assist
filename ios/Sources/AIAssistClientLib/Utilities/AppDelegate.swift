#if canImport(UIKit)
import UIKit

/// UIApplicationDelegate adapter for receiving remote notification registration callbacks.
/// Wire this into the SwiftUI app via `@UIApplicationDelegateAdaptor`.
public final class AppDelegate: NSObject, UIApplicationDelegate {

    /// Set by the SwiftUI app so callbacks can forward to the shared manager.
    public var notificationManager: NotificationManager?

    public func application(
        _ application: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        notificationManager?.didRegisterForRemoteNotifications(deviceToken: deviceToken)
    }

    public func application(
        _ application: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: Error
    ) {
        notificationManager?.didFailToRegisterForRemoteNotifications(error: error)
    }
}
#endif
