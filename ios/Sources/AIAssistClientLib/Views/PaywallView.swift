import StoreKit
import SwiftUI

/// Paywall screen showing subscription tiers, feature comparison, and purchase buttons.
public struct PaywallView: View {
    @Bindable var subscriptionService: SubscriptionService
    @Environment(\.dismiss) private var dismiss

    @State private var selectedProductId: String?
    @State private var isPurchasing = false

    public init(subscriptionService: SubscriptionService) {
        self.subscriptionService = subscriptionService
    }

    public var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 28) {
                    headerSection
                    featureList
                    productCards
                    purchaseButton
                    restoreButton
                    legalFooter
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 24)
            }
            .background(Color(.systemGroupedBackground))
            .navigationTitle("Go Premium")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") { dismiss() }
                }
            }
            .alert("Purchase Error", isPresented: .init(
                get: { subscriptionService.purchaseError != nil },
                set: { if !$0 { subscriptionService.purchaseError = nil } }
            )) {
                Button("OK", role: .cancel) {}
            } message: {
                if let error = subscriptionService.purchaseError {
                    Text(error)
                }
            }
            .onAppear {
                if selectedProductId == nil {
                    selectedProductId = subscriptionService.products.last?.id
                }
            }
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        VStack(spacing: 12) {
            Image(systemName: "crown.fill")
                .font(.system(size: 48))
                .foregroundStyle(.yellow.gradient)

            Text("Unlock AI Assist Premium")
                .font(.title2.bold())
                .multilineTextAlignment(.center)

            Text("Get the most out of your household AI assistant")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .padding(.top, 8)
    }

    // MARK: - Features

    private var featureList: some View {
        VStack(alignment: .leading, spacing: 14) {
            featureRow(icon: "house.fill", title: "Shared Households", description: "Coordinate tasks with family members")
            featureRow(icon: "bell.fill", title: "Push Notifications", description: "Never miss an important update")
            featureRow(icon: "brain.head.profile", title: "Unlimited AI Chat", description: "Ask your assistant anything, anytime")
            featureRow(icon: "calendar", title: "Calendar Integration", description: "Smart scheduling and reminders")
            featureRow(icon: "chart.bar.fill", title: "Insights & Analytics", description: "Track household productivity")
        }
        .padding(20)
        .background(.background, in: RoundedRectangle(cornerRadius: 16))
    }

    private func featureRow(icon: String, title: String, description: String) -> some View {
        HStack(spacing: 14) {
            Image(systemName: icon)
                .font(.body)
                .foregroundStyle(.tint)
                .frame(width: 28)

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.subheadline.weight(.semibold))
                Text(description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    // MARK: - Product Cards

    private var productCards: some View {
        VStack(spacing: 12) {
            if subscriptionService.isLoading {
                ProgressView("Loading plans...")
                    .padding()
            } else if subscriptionService.products.isEmpty {
                Text("Unable to load subscription plans.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .padding()
            } else {
                ForEach(subscriptionService.products, id: \.id) { product in
                    productCard(for: product)
                }
            }
        }
    }

    private func productCard(for product: Product) -> some View {
        let isSelected = selectedProductId == product.id
        let isAnnual = product.id == SubscriptionProduct.annual.rawValue

        return Button {
            withAnimation(.spring(response: 0.3)) {
                selectedProductId = product.id
            }
        } label: {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 8) {
                        Text(product.displayName)
                            .font(.headline)

                        if isAnnual {
                            Text("Best Value")
                                .font(.caption2.weight(.bold))
                                .foregroundStyle(.white)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 3)
                                .background(.green.gradient, in: Capsule())
                        }
                    }

                    Text(product.description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer()

                VStack(alignment: .trailing, spacing: 2) {
                    Text(product.displayPrice)
                        .font(.title3.weight(.bold))

                    if let subscription = product.subscription {
                        Text(periodLabel(subscription.subscriptionPeriod))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding(16)
            .background(
                RoundedRectangle(cornerRadius: 14)
                    .fill(isSelected ? Color.accentColor.opacity(0.08) : Color(.secondarySystemGroupedBackground))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 14)
                    .stroke(isSelected ? Color.accentColor : Color.clear, lineWidth: 2)
            )
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(product.displayName), \(product.displayPrice)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    private func periodLabel(_ period: Product.SubscriptionPeriod) -> String {
        switch period.unit {
        case .month: period.value == 1 ? "per month" : "per \(period.value) months"
        case .year: period.value == 1 ? "per year" : "per \(period.value) years"
        case .week: period.value == 1 ? "per week" : "per \(period.value) weeks"
        case .day: period.value == 1 ? "per day" : "per \(period.value) days"
        @unknown default: ""
        }
    }

    // MARK: - Purchase Button

    private var purchaseButton: some View {
        Button {
            Task { await purchaseSelected() }
        } label: {
            Group {
                if isPurchasing {
                    ProgressView()
                        .controlSize(.small)
                        .tint(.white)
                } else {
                    Text("Subscribe Now")
                        .fontWeight(.semibold)
                }
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 4)
        }
        .buttonStyle(.borderedProminent)
        .controlSize(.large)
        .disabled(selectedProductId == nil || isPurchasing || subscriptionService.products.isEmpty)
    }

    private func purchaseSelected() async {
        guard let selectedProductId,
              let product = subscriptionService.products.first(where: { $0.id == selectedProductId })
        else { return }

        isPurchasing = true
        let success = await subscriptionService.purchase(product)
        isPurchasing = false

        if success {
            dismiss()
        }
    }

    // MARK: - Restore

    private var restoreButton: some View {
        Button("Restore Purchases") {
            Task { await subscriptionService.restorePurchases() }
        }
        .font(.subheadline)
        .foregroundStyle(.secondary)
    }

    // MARK: - Legal

    private var legalFooter: some View {
        Text("Payment will be charged to your Apple ID account. Subscription automatically renews unless cancelled at least 24 hours before the end of the current period.")
            .font(.caption2)
            .foregroundStyle(.tertiary)
            .multilineTextAlignment(.center)
            .padding(.horizontal, 8)
    }
}
