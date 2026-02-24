import UIKit
import WebKit
import Tauri
import SwiftRs
import StoreKit

// MARK: - Argument types

class GetProductsArgs: Decodable {
    let productIds: [String]
}

class PurchaseArgs: Decodable {
    let productId: String
    let appAccountToken: String?
}

class GetSubscriptionStatusArgs: Decodable {
    let productId: String
}

// MARK: - StoreKit 2 Plugin (iOS 15+)

@MainActor
@available(iOS 15.0, *)
class IapPlugin: Plugin {
    // Simulator StoreKit 2 purchase calls have been unstable in some Xcode/iOS
    // runtime combinations. Provide a crash-safe simulator mock path by default.
    // Set DIARYX_IAP_SIMULATOR_REAL=1 in the Xcode scheme to force real StoreKit.
    private var simulatorEntitlements: [String: String] = [:]

    private var useSimulatorMock: Bool {
        // Only mock on the simulator. Real devices (including DEBUG builds)
        // always use real StoreKit so sandbox testing works.
        #if targetEnvironment(simulator)
        return ProcessInfo.processInfo.environment["DIARYX_IAP_SIMULATOR_REAL"] != "1"
        #else
        return false
        #endif
    }

    private struct MockJwtHeader: Codable {
        let alg = "none"
        let typ = "JWT"
    }

    private struct MockTransactionPayload: Codable {
        let originalTransactionId: String
        let productId: String
        let appAccountToken: String?
        let expiresDate: UInt64
        let revocationDate: UInt64?
        let bundleId: String
        let environment: String
    }

    private func base64UrlEncode(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    private func buildMockSignedTransaction(
        productId: String,
        appAccountToken: String?,
        originalTransactionId: String
    ) throws -> String {
        let header = try JSONEncoder().encode(MockJwtHeader())
        let nowMs = UInt64(Date().timeIntervalSince1970 * 1000)
        let payload = MockTransactionPayload(
            originalTransactionId: originalTransactionId,
            productId: productId,
            appAccountToken: appAccountToken,
            expiresDate: nowMs + (30 * 24 * 60 * 60 * 1000),
            revocationDate: nil,
            bundleId: Bundle.main.bundleIdentifier ?? "org.diaryx.desktop",
            environment: "Sandbox"
        )
        let payloadData = try JSONEncoder().encode(payload)
        return "\(base64UrlEncode(header)).\(base64UrlEncode(payloadData)).simulator"
    }

    // Avoid ad-hoc `Task { ... }` wrappers in command handlers because we have
    // seen Swift task allocator crashes when commands are invoked repeatedly.
    // Use native async plugin handlers instead.

    @objc public func getProducts(_ invoke: Invoke) async throws {
        let args = try invoke.parseArgs(GetProductsArgs.self)
        guard !args.productIds.isEmpty else {
            invoke.reject("Missing or empty productIds")
            return
        }

        do {
            let products = try await Product.products(for: Set(args.productIds))
            let result: [[String: Any]] = products.map { product in
                [
                    "id": product.id,
                    "title": product.displayName,
                    "description": product.description,
                    "price": product.displayPrice,
                    "priceLocale": product.priceFormatStyle.locale.identifier,
                ]
            }
            invoke.resolve(["value": result])
        } catch {
            invoke.reject("Failed to fetch products: \(error.localizedDescription)")
        }
    }

    @objc public func purchase(_ invoke: Invoke) async throws {
        let args = try invoke.parseArgs(PurchaseArgs.self)
        guard !args.productId.isEmpty else {
            invoke.reject("Missing productId")
            return
        }

        if useSimulatorMock {
            do {
                let txId = "sim-\(UUID().uuidString.lowercased())"
                let signed = try buildMockSignedTransaction(
                    productId: args.productId,
                    appAccountToken: args.appAccountToken,
                    originalTransactionId: txId
                )
                simulatorEntitlements[args.productId] = signed
                invoke.resolve([
                    "transactionId": txId,
                    "originalTransactionId": txId,
                    "productId": args.productId,
                    "signedTransaction": signed,
                ])
            } catch {
                invoke.reject("Failed to create simulator mock purchase: \(error.localizedDescription)")
            }
            return
        }

        do {
            let bundleId = Bundle.main.bundleIdentifier ?? "nil"
            NSLog("[IAP] useSimulatorMock=\(useSimulatorMock), bundleId=\(bundleId), requesting productId=\(args.productId)")
            let products = try await Product.products(for: [args.productId])
            NSLog("[IAP] Product.products returned \(products.count) product(s): \(products.map { $0.id })")
            guard let product = products.first else {
                invoke.reject("Product not found: \(args.productId)")
                return
            }

            var options: Set<Product.PurchaseOption> = []
            if let tokenString = args.appAccountToken {
                guard let uuid = UUID(uuidString: tokenString) else {
                    invoke.reject("Invalid appAccountToken: must be a UUID")
                    return
                }
                options.insert(.appAccountToken(uuid))
            }

            let result = options.isEmpty
                ? try await product.purchase()
                : try await product.purchase(options: options)

            switch result {
            case .success(let verification):
                let transaction = try checkVerification(verification)
                await transaction.finish()

                let response: [String: Any] = [
                    "transactionId": String(transaction.id),
                    "originalTransactionId": String(transaction.originalID),
                    "productId": transaction.productID,
                    "signedTransaction": verification.jwsRepresentation,
                ]
                invoke.resolve(response)

            case .userCancelled:
                invoke.reject("Purchase cancelled by user")

            case .pending:
                invoke.reject("Purchase is pending approval")

            @unknown default:
                invoke.reject("Unknown purchase result")
            }
        } catch {
            invoke.reject("Purchase failed: \(error.localizedDescription)")
        }
    }

    @objc public func restorePurchases(_ invoke: Invoke) async throws {
        if useSimulatorMock {
            invoke.resolve(["value": Array(simulatorEntitlements.values)])
            return
        }

        do {
            var jwsStrings: [String] = []
            for await verification in Transaction.currentEntitlements {
                if case .verified(let transaction) = verification {
                    // Only include non-revoked transactions
                    if transaction.revocationDate == nil {
                        jwsStrings.append(verification.jwsRepresentation)
                    }
                }
            }
            invoke.resolve(["value": jwsStrings])
        } catch {
            invoke.reject("Failed to restore purchases: \(error.localizedDescription)")
        }
    }

    @objc public func getSubscriptionStatus(_ invoke: Invoke) async throws {
        let args = try invoke.parseArgs(GetSubscriptionStatusArgs.self)
        guard !args.productId.isEmpty else {
            invoke.reject("Missing productId")
            return
        }

        if useSimulatorMock {
            invoke.resolve(["isSubscribed": simulatorEntitlements[args.productId] != nil])
            return
        }

        var isSubscribed = false
        for await verification in Transaction.currentEntitlements {
            if case .verified(let transaction) = verification {
                if transaction.productID == args.productId &&
                   transaction.revocationDate == nil {
                    isSubscribed = true
                    break
                }
            }
        }
        invoke.resolve(["isSubscribed": isSubscribed])
    }

    // MARK: - Helpers

    private func checkVerification<T>(_ result: VerificationResult<T>) throws -> T {
        switch result {
        case .unverified(_, let error):
            throw error
        case .verified(let value):
            return value
        }
    }
}

// MARK: - Dummy plugin for iOS < 15

class IapPluginDummy: Plugin {
    @objc public func getProducts(_ invoke: Invoke) {
        invoke.reject("StoreKit 2 requires iOS 15.0 or later")
    }
    @objc public func purchase(_ invoke: Invoke) {
        invoke.reject("StoreKit 2 requires iOS 15.0 or later")
    }
    @objc public func restorePurchases(_ invoke: Invoke) {
        invoke.reject("StoreKit 2 requires iOS 15.0 or later")
    }
    @objc public func getSubscriptionStatus(_ invoke: Invoke) {
        invoke.reject("StoreKit 2 requires iOS 15.0 or later")
    }
}

// MARK: - Plugin entry point

@_cdecl("init_plugin_iap")
func initPlugin() -> Plugin {
    if #available(iOS 15.0, *) {
        return IapPlugin()
    } else {
        return IapPluginDummy()
    }
}
