<script lang="ts">
  /**
   * SyncSettings - Multi-device sync status and setup
   *
   * Three states:
   * 1. Not authenticated → prompt to sign in from Account tab
   * 2. Authenticated, sync not enabled → "Set Up Sync" button (opens wizard at options screen)
   * 3. Authenticated, sync enabled → sync status display
   */
  import { Button } from "$lib/components/ui/button";
  import { Server, Wifi, WifiOff, Loader2 } from "@lucide/svelte";
  import { getAuthState, initAuth } from "$lib/auth";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
  import { onMount } from "svelte";

  interface Props {
    /** Callback to open the sync setup wizard */
    onOpenWizard?: () => void;
  }

  let { onOpenWizard }: Props = $props();

  // Get auth state reactively
  let authState = $derived(getAuthState());

  // Get collaboration state
  let syncStatus = $derived(collaborationStore.syncStatus);
  let isEnabled = $derived(collaborationStore.collaborationEnabled);

  // Get server URL for display
  let serverUrl = $derived(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx_sync_server_url") || ""
      : ""
  );

  // Initialize auth on mount
  onMount(() => {
    initAuth();
  });
</script>

<div class="space-y-4">
  <!-- Header -->
  <h3 class="font-medium flex items-center gap-2">
    <Server class="size-4" />
    Multi-Device Sync
    <span class="text-[10px] font-semibold uppercase px-1.5 py-0.5 rounded-full bg-blue-500/15 text-blue-600 dark:text-blue-400">Beta</span>
  </h3>

  <p class="text-xs text-muted-foreground px-1">
    Sync your workspace across devices with our cloud server.
  </p>

  {#if authState.isAuthenticated && isEnabled}
    <!-- State 3: Authenticated + sync enabled → show status -->
    <div class="space-y-3">
      {#if serverUrl}
        <div class="text-xs text-muted-foreground px-1">
          Server: <span class="font-mono">{serverUrl}</span>
        </div>
      {/if}

      <div class="flex items-center gap-2 p-2 bg-muted/50 rounded-md">
        {#if syncStatus === "syncing" || syncStatus === "connecting"}
          <Loader2 class="size-4 text-primary animate-spin" />
          <span class="text-sm">{syncStatus === "connecting" ? "Connecting..." : "Syncing..."}</span>
        {:else if syncStatus === "synced" || syncStatus === "idle"}
          <Wifi class="size-4 text-green-500" />
          <span class="text-sm text-green-600 dark:text-green-400">Connected</span>
        {:else if syncStatus === "error"}
          <WifiOff class="size-4 text-destructive" />
          <span class="text-sm text-destructive">Connection Error</span>
        {:else}
          <WifiOff class="size-4 text-muted-foreground" />
          <span class="text-sm text-muted-foreground">Not Connected</span>
        {/if}
      </div>

      <p class="text-xs text-muted-foreground">
        Your workspace is syncing across all your devices. Manage your account in the Account tab.
      </p>
    </div>
  {:else if authState.isAuthenticated && !isEnabled}
    <!-- State 2: Authenticated but sync not enabled → offer setup -->
    <div class="space-y-3">
      <div class="flex items-center gap-2 p-2 bg-muted/50 rounded-md">
        <WifiOff class="size-4 text-muted-foreground" />
        <span class="text-sm text-muted-foreground">Sync not configured</span>
      </div>

      {#if onOpenWizard}
        <Button
          variant="default"
          size="sm"
          class="w-full"
          onclick={onOpenWizard}
        >
          <Server class="size-4 mr-2" />
          Set Up Sync
        </Button>
      {/if}
      <p class="text-xs text-muted-foreground px-1">
        You're signed in. Set up sync to access your notes across devices.
      </p>
    </div>
  {:else}
    <!-- State 1: Not authenticated → direct to Account tab -->
    <div class="space-y-3">
      <div class="flex items-center gap-2 p-2 bg-muted/50 rounded-md">
        <WifiOff class="size-4 text-muted-foreground" />
        <span class="text-sm text-muted-foreground">Not signed in</span>
      </div>
      <p class="text-xs text-muted-foreground px-1">
        Sign in from the Account tab to enable sync across devices.
      </p>
    </div>
  {/if}
</div>
