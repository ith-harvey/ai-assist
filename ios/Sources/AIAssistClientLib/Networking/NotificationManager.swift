import Foundation
import Observation
import UserNotifications
#if canImport(UIKit)
import UIKit
#endif

/// Notification payload types the server can send.
public enum NotificationDestination: Equatable, Sendable {
    case todo(id: UUID)
    case calendar(id: UUID)
    case card(id: UUID)
    case chat
    case unknown
}

/// Manages push notification permissions, token registration, and incoming notification handling.
/// Uses @Observable so SwiftUI views reactively update when authorization status changes.
/// All mutable state is isolated to @MainActor to prevent data races.
@MainActor @Observable
public final class NotificationManager: NSObject {

    // MARK: - Observable state

    /// Current authorization status — drives UI for permission prompts and settings.
    public var authorizationStatus: UNAuthorizationStatus = .notDetermined

    /// The hex-encoded device token, set after successful APNS registration.
    public var deviceToken: String?

    /// Set when a notification tap should navigate the user somewhere.
    public var pendingDestination: NotificationDestination?

    // MARK: - Private

    private let center = UNUserNotificationCenter.current()
    private var tokenAPI: DeviceTokenAPI?

    /// Whether the server connection uses HTTP (local dev) or HTTPS (production).
    private var useSecureTransport: Bool = true

    // MARK: - Init

    public override init() {
        super.init()
        center.delegate = self
        Task { await refreshAuthorizationStatus() }
    }

    // MARK: - Permission flow

    /// Request notification authorization. Call on first launch or when user enables notifications.
    public func requestAuthorization() async {
        do {
            let granted = try await center.requestAuthorization(options: [.alert, .badge, .sound])
            await refreshAuthorizationStatus()
            if granted {
                registerForRemoteNotifications()
            }
        } catch {
            print("[NotificationManager] Authorization request failed: \(error)")
        }
    }

    /// Refresh the cached authorization status from the system.
    public func refreshAuthorizationStatus() async {
        let settings = await center.notificationSettings()
        authorizationStatus = settings.authorizationStatus
    }

    /// Register with APNS.
    private func registerForRemoteNotifications() {
        #if canImport(UIKit) && !targetEnvironment(simulator)
        UIApplication.shared.registerForRemoteNotifications()
        #endif
    }

    // MARK: - Token management

    /// Called by AppDelegate when APNS registration succeeds.
    public func didRegisterForRemoteNotifications(deviceToken data: Data) {
        let token = data.map { String(format: "%02x", $0) }.joined()
        self.deviceToken = token
        UserDefaults.standard.set(token, forKey: "ai_assist_device_token")

        Task {
            do {
                try await tokenAPI?.register(token: token)
            } catch {
                print("[NotificationManager] Device token registration failed: \(error)")
            }
        }
    }

    /// Called by AppDelegate when APNS registration fails.
    public func didFailToRegisterForRemoteNotifications(error: Error) {
        print("[NotificationManager] APNS registration failed: \(error)")
        self.deviceToken = nil
    }

    /// Update the server connection and re-register the token if we have one.
    /// Pass `useSecureTransport: false` for local development servers using HTTP.
    public func updateServer(host: String, port: Int, useSecureTransport: Bool = true) {
        self.useSecureTransport = useSecureTransport
        tokenAPI = DeviceTokenAPI(host: host, port: port, useSecureTransport: useSecureTransport)
        if let token = deviceToken {
            Task {
                do {
                    try await tokenAPI?.register(token: token)
                } catch {
                    print("[NotificationManager] Device token re-registration failed: \(error)")
                }
            }
        }
    }

    // MARK: - Badge management

    /// Reset the app badge count to zero.
    public func clearBadge() {
        #if canImport(UIKit)
        UIApplication.shared.applicationIconBadgeNumber = 0
        #endif
    }

    // MARK: - Deep link parsing

    /// Parse a notification payload into a navigation destination.
    private nonisolated func parseDestination(from userInfo: [AnyHashable: Any]) -> NotificationDestination {
        guard let type = userInfo["type"] as? String else { return .unknown }

        switch type {
        case "todo":
            if let idStr = userInfo["todo_id"] as? String, let id = UUID(uuidString: idStr) {
                return .todo(id: id)
            }
        case "calendar":
            if let idStr = userInfo["event_id"] as? String, let id = UUID(uuidString: idStr) {
                return .calendar(id: id)
            }
        case "card":
            if let idStr = userInfo["card_id"] as? String, let id = UUID(uuidString: idStr) {
                return .card(id: id)
            }
        case "chat":
            return .chat
        default:
            break
        }
        return .unknown
    }
}

// MARK: - UNUserNotificationCenterDelegate

extension NotificationManager: @preconcurrency UNUserNotificationCenterDelegate {

    /// Foreground notification — show the banner even when app is active.
    nonisolated public func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .badge, .sound]
    }

    /// Notification tap — parse the payload and set the pending destination for navigation.
    nonisolated public func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse
    ) async {
        let userInfo = response.notification.request.content.userInfo
        let destination = parseDestination(from: userInfo)
        await MainActor.run {
            self.pendingDestination = destination
        }
    }
}
