import Foundation
import Observation
import StoreKit

/// Product identifiers matching App Store Connect configuration.
public enum SubscriptionProduct: String, CaseIterable {
    case monthly = "com.aiassist.subscription.monthly"
    case annual = "com.aiassist.subscription.annual"

    public var displayName: String {
        switch self {
        case .monthly: "Monthly"
        case .annual: "Annual"
        }
    }
}

/// Represents the user's current subscription state.
public enum SubscriptionStatus: Equatable {
    case notSubscribed
    case subscribed(productId: String, expiresDate: Date?, isInTrial: Bool)
    case expired
    case revoked

    public var isActive: Bool {
        if case .subscribed = self { return true }
        return false
    }
}

/// Manages StoreKit 2 subscription lifecycle: product loading, purchasing,
/// entitlement checking, and transaction observation.
@Observable
public final class SubscriptionService: @unchecked Sendable {
    // MARK: - Observable state

    public var products: [Product] = []
    public var subscriptionStatus: SubscriptionStatus = .notSubscribed
    public var isLoading = false
    public var purchaseError: String?

    /// Currently active product ID (for UI binding).
    public var activeProductId: String? {
        if case .subscribed(let productId, _, _) = subscriptionStatus {
            return productId
        }
        return nil
    }

    /// Whether the user has an active premium entitlement.
    public var isPremium: Bool {
        subscriptionStatus.isActive
    }

    // MARK: - Server configuration

    private var host: String
    private var port: Int

    // MARK: - Private

    private var transactionListener: Task<Void, Never>?

    public init(
        host: String = UserDefaults.standard.string(forKey: "ai_assist_host") ?? "localhost",
        port: Int = UserDefaults.standard.object(forKey: "ai_assist_port") as? Int ?? 8080
    ) {
        self.host = host
        self.port = port
        transactionListener = listenForTransactions()
        Task { await loadProducts() }
        Task { await refreshEntitlements() }
    }

    deinit {
        transactionListener?.cancel()
    }

    // MARK: - Server updates

    public func updateServer(host: String, port: Int) {
        self.host = host
        self.port = port
    }

    // MARK: - Product loading

    @MainActor
    public func loadProducts() async {
        isLoading = true
        defer { isLoading = false }

        do {
            let productIds = SubscriptionProduct.allCases.map(\.rawValue)
            let storeProducts = try await Product.products(for: Set(productIds))
            products = storeProducts.sorted { $0.price < $1.price }
        } catch {
            purchaseError = "Failed to load products: \(error.localizedDescription)"
        }
    }

    // MARK: - Purchasing

    @MainActor
    public func purchase(_ product: Product) async -> Bool {
        purchaseError = nil

        do {
            let result = try await product.purchase()

            switch result {
            case .success(let verification):
                let transaction = try checkVerified(verification)
                await transaction.finish()
                await refreshEntitlements()
                await verifyReceiptWithServer(transaction)
                return true

            case .userCancelled:
                return false

            case .pending:
                purchaseError = "Purchase is pending approval."
                return false

            @unknown default:
                purchaseError = "An unexpected purchase result occurred."
                return false
            }
        } catch {
            purchaseError = "Purchase failed: \(error.localizedDescription)"
            return false
        }
    }

    // MARK: - Restore purchases

    @MainActor
    public func restorePurchases() async {
        isLoading = true
        defer { isLoading = false }

        do {
            try await AppStore.sync()
            await refreshEntitlements()
        } catch {
            purchaseError = "Restore failed: \(error.localizedDescription)"
        }
    }

    // MARK: - Entitlement checking

    @MainActor
    public func refreshEntitlements() async {
        var foundActive = false

        for await result in Transaction.currentEntitlements {
            guard let transaction = try? checkVerified(result) else { continue }

            if transaction.revocationDate != nil {
                subscriptionStatus = .revoked
                return
            }

            if let expirationDate = transaction.expirationDate, expirationDate < Date() {
                subscriptionStatus = .expired
                return
            }

            let isInTrial = transaction.offerType == .introductory
            subscriptionStatus = .subscribed(
                productId: transaction.productID,
                expiresDate: transaction.expirationDate,
                isInTrial: isInTrial
            )
            foundActive = true
            break
        }

        if !foundActive {
            subscriptionStatus = .notSubscribed
        }
    }

    // MARK: - Transaction listener

    private func listenForTransactions() -> Task<Void, Never> {
        Task.detached { [weak self] in
            for await result in Transaction.updates {
                guard let self else { return }
                if let transaction = try? self.checkVerified(result) {
                    await transaction.finish()
                    await MainActor.run {
                        Task { await self.refreshEntitlements() }
                    }
                }
            }
        }
    }

    // MARK: - Verification

    private func checkVerified<T>(_ result: VerificationResult<T>) throws -> T {
        switch result {
        case .unverified(_, let error):
            throw error
        case .verified(let value):
            return value
        }
    }

    // MARK: - Server receipt verification

    private func verifyReceiptWithServer(_ transaction: Transaction) async {
        guard let url = URL(string: "http://\(host):\(port)/api/subscriptions/verify-receipt") else {
            return
        }

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        let body: [String: Any] = [
            "transactionId": String(transaction.id),
            "originalTransactionId": String(transaction.originalID),
            "productId": transaction.productID,
            "purchaseDate": ISO8601DateFormatter().string(from: transaction.purchaseDate),
            "expiresDate": transaction.expirationDate.map { ISO8601DateFormatter().string(from: $0) } as Any,
        ]

        guard let httpBody = try? JSONSerialization.data(withJSONObject: body) else { return }
        request.httpBody = httpBody

        _ = try? await URLSession.shared.data(for: request)
    }
}
