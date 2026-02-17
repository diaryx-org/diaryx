<script lang="ts">
  /**
   * BillingSettings - Subscription management
   *
   * Features:
   * - Current plan display (Free / Plus)
   * - Upgrade button (redirects to Stripe Checkout)
   * - Manage Billing button (redirects to Stripe Customer Portal)
   */
  import { Button } from "$lib/components/ui/button";
  import {
    CreditCard,
    Loader2,
    Crown,
    ExternalLink,
    Check,
    AlertCircle,
  } from "@lucide/svelte";
  import {
    getAuthState,
    createCheckoutSession,
    createPortalSession,
  } from "$lib/auth";

  let authState = $derived(getAuthState());
  let isPlus = $derived(authState.tier === "plus");
  let isLoading = $state(false);
  let error = $state<string | null>(null);

  async function handleUpgrade() {
    isLoading = true;
    error = null;
    try {
      const url = await createCheckoutSession();
      window.location.href = url;
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to start checkout";
    } finally {
      isLoading = false;
    }
  }

  async function handleManageBilling() {
    isLoading = true;
    error = null;
    try {
      const url = await createPortalSession();
      window.location.href = url;
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to open billing portal";
    } finally {
      isLoading = false;
    }
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
        {isPlus ? "$5/month" : "Free forever"}
      </p>
    </div>
  </div>

  {#if isPlus}
    <!-- Plus subscriber view -->
    <div class="space-y-2">
      <p class="text-sm text-muted-foreground">
        Thank you for supporting Diaryx!
      </p>
      <Button
        variant="outline"
        size="sm"
        class="w-full justify-start"
        onclick={handleManageBilling}
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
    </div>
  {:else}
    <!-- Free user view -->
    <div class="space-y-3">
      <div class="space-y-2 text-sm">
        <p class="font-medium">Plus includes:</p>
        <ul class="space-y-1 text-muted-foreground">
          <li class="flex items-center gap-2"><Check class="size-3 text-primary" /> 10 workspaces</li>
          <li class="flex items-center gap-2"><Check class="size-3 text-primary" /> 2 GiB attachment storage</li>
          <li class="flex items-center gap-2"><Check class="size-3 text-primary" /> 5 published sites</li>
        </ul>
      </div>
      <Button
        class="w-full"
        onclick={handleUpgrade}
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
    </div>
  {/if}
</div>
