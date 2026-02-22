<script lang="ts">
  /**
   * BillingSettings - Subscription management
   *
   * Features:
   * - Current plan display (Free / Plus)
   * - Stripe: Upgrade button (redirects to Stripe Checkout)
   * - Stripe: Manage Billing button (redirects to Stripe Customer Portal)
   * - Apple IAP: Upgrade via native StoreKit sheet
   * - Apple IAP: Restore Purchases (required by App Store Review)
   * - Apple IAP: Manage Subscription link
   */
  import { Button } from "$lib/components/ui/button";
  import {
    CreditCard,
    Loader2,
    Crown,
    ExternalLink,
    Check,
    AlertCircle,
    RotateCcw,
  } from "@lucide/svelte";
  import {
    getAuthState,
    createCheckoutSession,
    createPortalSession,
    verifyAppleTransaction,
    restoreApplePurchases,
  } from "$lib/auth";
  import {
    getBillingProvider,
    purchasePlus,
    restoreIapPurchases,
    getPlusProductId,
    openStripeUrl,
    pollForPlusUpgrade,
  } from "$lib/billing";
  import { isTauri } from "$lib/backend/interface";

  let authState = $derived(getAuthState());
  let isPlus = $derived(authState.tier === "plus");
  let isLoading = $state(false);
  let isWaitingForCheckout = $state(false);
  let error = $state<string | null>(null);

  const billingProvider = getBillingProvider();
  const iapPrice = "$5/month";

  // ---- Stripe handlers ----

  async function handleStripeUpgrade() {
    isLoading = true;
    error = null;
    try {
      const url = await createCheckoutSession();
      await openStripeUrl(url);
      // In Tauri, poll for the upgrade since the user stays in the app
      if (isTauri()) {
        isLoading = false;
        isWaitingForCheckout = true;
        const upgraded = await pollForPlusUpgrade();
        isWaitingForCheckout = false;
        if (!upgraded) {
          error = "Didn't detect the upgrade yet. If you completed payment, try refreshing.";
        }
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to start checkout";
    } finally {
      isLoading = false;
      isWaitingForCheckout = false;
    }
  }

  async function handleStripeManageBilling() {
    isLoading = true;
    error = null;
    try {
      const url = await createPortalSession();
      await openStripeUrl(url);
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to open billing portal";
    } finally {
      isLoading = false;
    }
  }

  // ---- Apple IAP handlers ----

  async function handleIapUpgrade() {
    isLoading = true;
    error = null;
    try {
      const userId = authState.user?.id;
      if (!userId) throw new Error("Not signed in");

      const result = await purchasePlus(userId);
      if (!result) return; // user cancelled

      // Verify with server
      await verifyAppleTransaction(
        result.signedTransaction,
        getPlusProductId(),
      );
    } catch (e) {
      console.error("[IAP] Purchase error:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  }

  async function handleIapRestore() {
    isLoading = true;
    error = null;
    try {
      const transactions = await restoreIapPurchases();
      if (transactions.length === 0) {
        error = "No purchases found to restore.";
        return;
      }
      const result = await restoreApplePurchases(transactions);
      if (result.restored_count === 0) {
        error = "No active subscriptions found.";
      }
    } catch (e) {
      error =
        e instanceof Error ? e.message : "Failed to restore purchases";
    } finally {
      isLoading = false;
    }
  }

  function handleManageAppleSubscription() {
    window.open("https://apps.apple.com/account/subscriptions", "_blank");
  }
</script>

<div class="space-y-4">
  <h3 class="font-medium flex items-center gap-2">
    <CreditCard class="size-4" />
    Billing
  </h3>

  {#if error}
    <div class="flex items-center gap-2 text-destructive text-sm p-2 bg-destructive/10 rounded-md">
      <AlertCircle class="size-4 shrink-0" />
      <span>{error}</span>
    </div>
  {/if}

  <!-- Current Plan -->
  <div class="flex items-center justify-between p-3 bg-muted/50 rounded-md">
    <div>
      <div class="flex items-center gap-2">
        <span class="font-medium">
          {isPlus ? "Plus" : "Free"}
        </span>
        {#if isPlus}
          <span class="text-[10px] font-semibold uppercase px-1.5 py-0.5 rounded-full bg-amber-500/15 text-amber-600 dark:text-amber-400">
            <Crown class="size-3 inline -mt-0.5" /> Plus
          </span>
        {/if}
      </div>
      <p class="text-xs text-muted-foreground mt-1">
        {#if isPlus}
          {billingProvider === "apple_iap" ? iapPrice : "$5/month"}
        {:else}
          Free forever
        {/if}
      </p>
    </div>
  </div>

  {#if isPlus}
    <!-- Plus subscriber view -->
    <div class="space-y-2">
      <p class="text-sm text-muted-foreground">
        Thank you for supporting Diaryx!
      </p>

      {#if billingProvider === "apple_iap"}
        <!-- Apple IAP: manage via App Store -->
        <Button
          variant="outline"
          size="sm"
          class="w-full justify-start"
          onclick={handleManageAppleSubscription}
        >
          <ExternalLink class="size-4 mr-2" />
          Manage Subscription
        </Button>
        <p class="text-xs text-muted-foreground">
          Manage your subscription in the App Store.
        </p>
      {:else}
        <!-- Stripe: manage via portal -->
        <Button
          variant="outline"
          size="sm"
          class="w-full justify-start"
          onclick={handleStripeManageBilling}
          disabled={isLoading}
        >
          {#if isLoading}
            <Loader2 class="size-4 mr-2 animate-spin" />
          {:else}
            <ExternalLink class="size-4 mr-2" />
          {/if}
          Manage Billing
        </Button>
        <p class="text-xs text-muted-foreground">
          Update payment method, view invoices, or cancel subscription.
        </p>
      {/if}
    </div>
  {:else}
    <!-- Free user view -->
    <div class="space-y-3">
      <div class="space-y-2 text-sm">
        <p class="font-medium">Plus includes:</p>
        <ul class="space-y-1 text-muted-foreground">
          <li class="flex items-center gap-2"><Check class="size-3 text-primary" /> Multi-device sync</li>
          <li class="flex items-center gap-2"><Check class="size-3 text-primary" /> Live collaboration sessions (up to 5 guests)</li>
          <li class="flex items-center gap-2"><Check class="size-3 text-primary" /> Published websites (up to 5, 10 audiences each)</li>
          <li class="flex items-center gap-2"><Check class="size-3 text-primary" /> 10 workspaces</li>
          <li class="flex items-center gap-2"><Check class="size-3 text-primary" /> 2 GiB attachment storage</li>
        </ul>
      </div>

      {#if billingProvider === "apple_iap"}
        <!-- Apple IAP: native StoreKit purchase -->
        <Button
          class="w-full"
          onclick={handleIapUpgrade}
          disabled={isLoading}
        >
          {#if isLoading}
            <Loader2 class="size-4 mr-2 animate-spin" />
            Loading...
          {:else}
            <Crown class="size-4 mr-2" />
            Upgrade to Plus — {iapPrice}
          {/if}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-start text-muted-foreground"
          onclick={handleIapRestore}
          disabled={isLoading}
        >
          <RotateCcw class="size-4 mr-2" />
          Restore Purchases
        </Button>
      {:else if isWaitingForCheckout}
        <!-- Waiting for Stripe checkout to complete (Tauri) -->
        <div class="flex flex-col items-center gap-2 p-4 bg-muted/50 rounded-md text-center">
          <Loader2 class="size-5 animate-spin text-muted-foreground" />
          <p class="text-sm font-medium">Complete checkout in your browser</p>
          <p class="text-xs text-muted-foreground">
            We'll detect your upgrade automatically.
          </p>
          <Button
            variant="ghost"
            size="sm"
            class="mt-1"
            onclick={() => { isWaitingForCheckout = false; }}
          >
            Dismiss
          </Button>
        </div>
      {:else}
        <!-- Stripe: redirect to checkout -->
        <Button
          class="w-full"
          onclick={handleStripeUpgrade}
          disabled={isLoading}
        >
          {#if isLoading}
            <Loader2 class="size-4 mr-2 animate-spin" />
            Loading...
          {:else}
            <Crown class="size-4 mr-2" />
            Upgrade to Plus — $5/month
          {/if}
        </Button>
      {/if}
    </div>
  {/if}
</div>
