export { getBillingProvider, type BillingProvider } from "./platform";
export { openStripeUrl, pollForPlusUpgrade } from "./stripeRedirect";
export {
  fetchProducts,
  purchasePlus,
  restoreIapPurchases,
  checkSubscriptionStatus,
  getPlusProductId,
  type IapProduct,
  type IapPurchaseResult,
} from "./iapService";
