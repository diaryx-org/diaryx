<script lang="ts">
  import type { Component } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Loader2, Sparkles } from "@lucide/svelte";
  import {
    getAuthState,
    createCheckoutSession,
    verifyAppleTransaction,
    refreshUserInfo,
    restoreApplePurchases,
  } from "$lib/auth";
  import {
    getBillingProvider,
    openStripeUrl,
    purchasePlus,
    restoreIapPurchases,
    getPlusProductId,
    openExternalUrl,
    pollForPlusUpgrade,
  } from "$lib/billing";
  import { isTauri } from "$lib/backend/interface";

  interface Props {
    feature?: string;
    description: string;
    icon?: Component;
    onUpgraded?: () => void;
  }

  let { description, icon, onUpgraded }: Props = $props();

  const authState = $derived(getAuthState());
  const billingProvider = getBillingProvider();
  let isUpgrading = $state(false);
  let upgradeError = $state<string | null>(null);

  async function handleApplePurchase() {
    isUpgrading = true;
    upgradeError = null;
    try {
      const userId = getAuthState().user?.id;
      if (!userId) throw new Error("Not signed in");
      const result = await purchasePlus(userId);
      if (!result) return;
      await verifyAppleTransaction(result.signedTransaction, getPlusProductId());
      await refreshUserInfo();
      onUpgraded?.();
    } catch (e) {
      upgradeError = e instanceof Error ? e.message : String(e);
    } finally {
      isUpgrading = false;
    }
  }

  async function handleAppleRestore() {
    isUpgrading = true;
    upgradeError = null;
    try {
      const transactions = await restoreIapPurchases();
      if (transactions.length === 0) {
        upgradeError = "No purchases found to restore.";
        return;
      }
      const result = await restoreApplePurchases(transactions);
      if (result.restored_count === 0) {
        upgradeError = "No active subscriptions found.";
        return;
      }
      await refreshUserInfo();
      onUpgraded?.();
    } catch (e) {
      upgradeError = e instanceof Error ? e.message : "Failed to restore purchases";
    } finally {
      isUpgrading = false;
    }
  }

  async function handleStripeCheckout() {
    isUpgrading = true;
    upgradeError = null;
    try {
      const url = await createCheckoutSession();
      await openStripeUrl(url);
      if (isTauri()) {
        await pollForPlusUpgrade();
        onUpgraded?.();
      }
    } catch (e) {
      upgradeError = e instanceof Error ? e.message : String(e);
    } finally {
      isUpgrading = false;
    }
  }
</script>

<div class="text-center space-y-3 py-6">
  {#if icon}
    {@const Icon = icon}
    <Icon class="size-8 mx-auto text-muted-foreground" />
  {:else}
    <Sparkles class="size-8 mx-auto text-muted-foreground" />
  {/if}
  <h3 class="font-medium text-sm">Upgrade to Plus</h3>
  <p class="text-xs text-muted-foreground">{description}</p>
  {#if upgradeError}
    <p class="text-xs text-destructive">{upgradeError}</p>
  {/if}
  {#if !authState.isAuthenticated}
    <p class="text-xs text-muted-foreground">Sign in to upgrade to Plus.</p>
  {:else if billingProvider === "apple_iap"}
    <Button
      variant="default"
      size="sm"
      onclick={handleApplePurchase}
      disabled={isUpgrading}
    >
      {#if isUpgrading}
        <Loader2 class="size-4 mr-2 animate-spin" />
        Loading...
      {:else}
        Upgrade to Plus — $5/month
      {/if}
    </Button>
    <button
      type="button"
      class="text-xs text-muted-foreground hover:text-foreground transition-colors"
      onclick={handleAppleRestore}
      disabled={isUpgrading}
    >
      Restore Purchases
    </button>
    <p class="text-[10px] text-muted-foreground/70 text-center leading-tight">
      $4.99/month. Auto-renews monthly. Cancel anytime in Settings &gt; Apple&nbsp;ID &gt; Subscriptions.
      <button type="button" class="underline" onclick={() => openExternalUrl('https://diaryx.org/terms')}>Terms</button> &
      <button type="button" class="underline" onclick={() => openExternalUrl('https://diaryx.org/privacy')}>Privacy</button>.
    </p>
  {:else}
    <Button
      variant="default"
      size="sm"
      onclick={handleStripeCheckout}
      disabled={isUpgrading}
    >
      {#if isUpgrading}
        <Loader2 class="size-4 mr-2 animate-spin" />
        Loading...
      {:else}
        Upgrade to Plus — $5/month
      {/if}
    </Button>
    <p class="text-[10px] text-muted-foreground/70 text-center leading-tight">
      <button type="button" class="underline" onclick={() => openExternalUrl('https://diaryx.org/terms')}>Terms</button> &
      <button type="button" class="underline" onclick={() => openExternalUrl('https://diaryx.org/privacy')}>Privacy</button>
    </p>
  {/if}
</div>
