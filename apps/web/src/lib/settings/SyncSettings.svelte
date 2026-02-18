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
  import { Server, Wifi, WifiOff, Loader2, RefreshCw, Database } from "@lucide/svelte";
  import {
    getAuthState,
    getStorageUsage,
    getWorkspaces,
    initAuth,
    refreshUserStorageUsage,
  } from "$lib/auth";
  import { getStorageUsageState, getUsageSummary } from "./syncSettingsLogic";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
  import { onMount } from "svelte";

  interface Props {
    /** Callback to open the sync setup wizard */
    onOpenWizard?: () => void;
  }

  let { onOpenWizard }: Props = $props();

  // Get auth state reactively
  let authState = $derived(getAuthState());
  let serverWorkspaces = $derived(getWorkspaces());

  // Get collaboration state
  let syncStatus = $derived(collaborationStore.syncStatus);
  let isEnabled = $derived(collaborationStore.collaborationEnabled);
  let storageUsage = $derived(getStorageUsage());
  let storageUsageState = $derived(getStorageUsageState(storageUsage));
  let usageSummary = $derived(getUsageSummary(storageUsage, formatBytes));
  let isRefreshingUsage = $state(false);

  // Get server URL for display
  let serverUrl = $derived(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx_sync_server_url") || ""
      : ""
  );

  // Initialize auth on mount
  onMount(() => {
    initAuth();
    refreshUserStorageUsage();
  });

  async function handleRefreshStorageUsage() {
    isRefreshingUsage = true;
    try {
      await refreshUserStorageUsage();
    } finally {
      isRefreshingUsage = false;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const index = Math.floor(Math.log(bytes) / Math.log(1024));
    const value = bytes / Math.pow(1024, index);
    return `${value.toFixed(value < 10 && index > 0 ? 1 : 0)} ${units[index]}`;
  }
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
        <span class="text-sm text-muted-foreground">Sync not configured on this device</span>
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
        {#if serverWorkspaces.length > 0}
          You have {serverWorkspaces.length} workspace{serverWorkspaces.length === 1 ? '' : 's'} on the server. Click "Set Up Sync" to download and sync on this device, or switch workspaces from the selector in the sidebar.
        {:else}
          You're signed in. Set up sync to access your notes across devices.
        {/if}
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

  {#if authState.isAuthenticated}
    <div class="rounded-md border p-3 space-y-3">
      <div class="flex items-center justify-between gap-2">
        <h4 class="text-sm font-medium flex items-center gap-2">
          <Database class="size-4" />
          Synced Storage
        </h4>
        <Button
          variant="ghost"
          size="icon"
          class="size-7"
          onclick={handleRefreshStorageUsage}
          disabled={isRefreshingUsage}
          aria-label="Refresh storage usage"
        >
          <RefreshCw class="size-3.5 {isRefreshingUsage ? 'animate-spin' : ''}" />
        </Button>
      </div>

      {#if storageUsage}
        <div class="grid grid-cols-2 gap-2 text-xs">
          <div class="rounded-md bg-muted/50 p-2">
            <div class="text-muted-foreground">Used</div>
            <div class="font-medium">{formatBytes(storageUsage.used_bytes)}</div>
          </div>
          <div class="rounded-md bg-muted/50 p-2">
            <div class="text-muted-foreground">Blobs</div>
            <div class="font-medium">{storageUsage.blob_count}</div>
          </div>
        </div>
        {#if usageSummary}
          <div class="text-xs text-muted-foreground">
            {usageSummary}
          </div>
        {/if}
        {#if storageUsageState === "over_limit"}
          <p class="text-xs text-destructive">
            Storage limit exceeded. New attachment uploads are blocked.
          </p>
        {:else if storageUsageState === "warning"}
          <p class="text-xs text-amber-600 dark:text-amber-400">
            Approaching storage limit.
          </p>
        {:else}
          <p class="text-xs text-muted-foreground">
            Includes synced attachment blobs.
          </p>
        {/if}
      {:else}
        <p class="text-xs text-muted-foreground">
          Storage usage unavailable.
        </p>
      {/if}
    </div>
  {/if}
</div>
