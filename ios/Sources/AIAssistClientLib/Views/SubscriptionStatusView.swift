import StoreKit
import SwiftUI

/// Displays the user's current subscription plan, expiry date, and manage link.
public struct SubscriptionStatusView: View {
    @Bindable var subscriptionService: SubscriptionService

    public init(subscriptionService: SubscriptionService) {
        self.subscriptionService = subscriptionService
    }

    public var body: some View {
        Group {
            switch subscriptionService.subscriptionStatus {
            case .notSubscribed:
                notSubscribedSection
            case .subscribed(let productId, let expiresDate, let isInTrial):
                subscribedSection(productId: productId, expiresDate: expiresDate, isInTrial: isInTrial)
            case .expired:
                expiredSection
            case .revoked:
                revokedSection
            }
        }
    }

    // MARK: - Not Subscribed

    private var notSubscribedSection: some View {
        Section("Subscription") {
            HStack {
                Label("Plan", systemImage: "crown")
                Spacer()
                Text("Free")
                    .foregroundStyle(.secondary)
            }
        }
    }

    // MARK: - Subscribed

    private func subscribedSection(productId: String, expiresDate: Date?, isInTrial: Bool) -> some View {
        Section("Subscription") {
            HStack {
                Label("Plan", systemImage: "crown.fill")
                    .foregroundStyle(.primary)
                Spacer()
                HStack(spacing: 6) {
                    if isInTrial {
                        Text("Trial")
                            .font(.caption2.weight(.bold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(.blue.gradient, in: Capsule())
                    }
                    Text(planName(for: productId))
                        .foregroundStyle(.secondary)
                }
            }

            if let expiresDate {
                HStack {
                    Label("Renews", systemImage: "arrow.clockwise")
                    Spacer()
                    Text(expiresDate, style: .date)
                        .foregroundStyle(.secondary)
                }
            }

            Button {
                openSubscriptionManagement()
            } label: {
                Label("Manage Subscription", systemImage: "gear")
            }
        }
    }

    // MARK: - Expired

    private var expiredSection: some View {
        Section("Subscription") {
            HStack {
                Label("Plan", systemImage: "crown")
                Spacer()
                Text("Expired")
                    .foregroundStyle(.orange)
            }

            Button {
                openSubscriptionManagement()
            } label: {
                Label("Resubscribe", systemImage: "arrow.clockwise")
            }
        }
    }

    // MARK: - Revoked

    private var revokedSection: some View {
        Section("Subscription") {
            HStack {
                Label("Plan", systemImage: "crown")
                Spacer()
                Text("Revoked")
                    .foregroundStyle(.red)
            }
        }
    }

    // MARK: - Helpers

    private func planName(for productId: String) -> String {
        if let product = SubscriptionProduct(rawValue: productId) {
            return "Premium \(product.displayName)"
        }
        return "Premium"
    }

    private func openSubscriptionManagement() {
        #if os(iOS)
        if let scene = UIApplication.shared.connectedScenes
            .compactMap({ $0 as? UIWindowScene })
            .first {
            Task {
                try? await AppStore.showManageSubscriptions(in: scene)
            }
        }
        #endif
    }
}
