<script lang="ts">
  /**
   * ICloudSettings - iCloud Drive workspace storage settings (iOS only)
   *
   * Shows iCloud availability, a toggle to enable/disable iCloud storage,
   * and sync status when active.
   */
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Button } from "$lib/components/ui/button";
  import { Cloud, CloudOff, RefreshCw, AlertCircle, Check } from "@lucide/svelte";
  import { getBackend } from "$lib/backend";

  let icloudAvailable = $state<boolean | null>(null);
  let icloudActive = $state(false);
  let syncStatus = $state<{
    totalItems: number;
    uploading: number;
    downloading: number;
    upToDate: boolean;
  } | null>(null);
  let error = $state<string | null>(null);
  let migrating = $state(false);
  let showConfirm = $state(false);
  let pendingEnabled = $state(false);

  // Initialize state from app paths
  $effect(() => {
    loadState();
  });

  async function loadState() {
    try {
      const backend = await getBackend();
      const paths = backend.getAppPaths();
      if (paths) {
        icloudActive = (paths.icloud_active as boolean) ?? false;
      }
    } catch {
      // Not in Tauri environment
    }
    await checkAvailability();
  }

  // Listen for sync status events
  let unlistenSyncStatus: (() => void) | null = null;

  $effect(() => {
    if (icloudActive) {
      setupSyncListener();
    }
    return () => {
      if (unlistenSyncStatus) {
        unlistenSyncStatus();
        unlistenSyncStatus = null;
      }
    };
  });

  async function setupSyncListener() {
    try {
      const { listen } = await import("@tauri-apps/api/event");
      unlistenSyncStatus = await listen<{
        totalItems: number;
        uploading: number;
        downloading: number;
        upToDate: boolean;
      }>("icloud-sync-status-changed", (event) => {
        syncStatus = event.payload;
        // syncing state is derived from syncStatus.upToDate
      });
    } catch {
      // Not in Tauri environment
    }
  }

  async function checkAvailability() {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<{ isAvailable: boolean }>(
        "plugin:icloud|check_icloud_available",
      );
      icloudAvailable = result.isAvailable;
    } catch {
      icloudAvailable = false;
    }
  }

  function handleToggle(enabled: boolean) {
    if (migrating) return;

    if (enabled !== icloudActive) {
      pendingEnabled = enabled;
      showConfirm = true;
    }
  }

  function cancelToggle() {
    showConfirm = false;
  }

  async function confirmToggle() {
    showConfirm = false;
    migrating = true;
    error = null;

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("set_icloud_enabled", {
        enabled: pendingEnabled,
      });
      icloudActive = pendingEnabled;

      // Reload the page to reinitialize with new workspace
      window.location.reload();
    } catch (e) {
      error =
        e instanceof Error
          ? e.message
          : typeof e === "string"
            ? e
            : "Failed to update iCloud settings";
    } finally {
      migrating = false;
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Cloud class="size-4" />
    iCloud Drive
  </h3>

  <p class="text-xs text-muted-foreground px-1">
    Store your workspace in iCloud Drive to sync across devices.
  </p>

  {#if icloudAvailable === false}
    <div class="flex items-start gap-2 p-3 rounded-lg bg-muted">
      <CloudOff class="size-4 text-muted-foreground mt-0.5 shrink-0" />
      <p class="text-sm text-muted-foreground">
        iCloud is not available. Sign in to iCloud in your device Settings to
        enable this feature.
      </p>
    </div>
  {:else if icloudAvailable === null}
    <div class="flex items-center gap-2 p-3 rounded-lg bg-muted">
      <RefreshCw class="size-4 text-muted-foreground animate-spin" />
      <p class="text-sm text-muted-foreground">Checking iCloud availability...</p>
    </div>
  {:else}
    <!-- Toggle -->
    <div class="flex items-center justify-between px-1">
      <Label for="icloud-toggle" class="text-sm">
        {icloudActive ? "iCloud storage enabled" : "Enable iCloud storage"}
      </Label>
      <Switch
        id="icloud-toggle"
        checked={icloudActive}
        onCheckedChange={handleToggle}
        disabled={migrating}
      />
    </div>

    <!-- Confirmation dialog -->
    {#if showConfirm}
      <div class="flex items-start gap-2 p-3 rounded-lg bg-amber-500/10 border border-amber-500/30">
        <AlertCircle class="size-4 text-amber-600 mt-0.5 shrink-0" />
        <div class="flex-1 min-w-0">
          <p class="text-sm text-amber-700 dark:text-amber-400">
            {#if pendingEnabled}
              This will move your workspace files to iCloud Drive. Your data
              will sync across all devices signed in with the same Apple ID.
            {:else}
              This will move your workspace files back to local storage. Files
              will no longer sync across devices.
            {/if}
          </p>
          <div class="flex gap-2 mt-2">
            <Button size="sm" onclick={confirmToggle} disabled={migrating}>
              {#if migrating}
                Migrating...
              {:else}
                Confirm
              {/if}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onclick={cancelToggle}
              disabled={migrating}
            >
              Cancel
            </Button>
          </div>
        </div>
      </div>
    {/if}

    <!-- Sync status -->
    {#if icloudActive}
      <div class="px-1 space-y-1">
        {#if syncStatus && !syncStatus.upToDate}
          <div class="flex items-center gap-2 text-sm text-muted-foreground">
            <RefreshCw class="size-3.5 animate-spin" />
            <span>
              Syncing...
              {#if syncStatus.uploading > 0}
                {syncStatus.uploading} uploading
              {/if}
              {#if syncStatus.uploading > 0 && syncStatus.downloading > 0}
                ,{" "}
              {/if}
              {#if syncStatus.downloading > 0}
                {syncStatus.downloading} downloading
              {/if}
            </span>
          </div>
        {:else}
          <div class="flex items-center gap-2 text-sm text-muted-foreground">
            <Check class="size-3.5 text-green-500" />
            <span>Up to date</span>
          </div>
        {/if}
      </div>
    {/if}
  {/if}

  <!-- Error display -->
  {#if error}
    <div class="flex items-start gap-2 p-3 rounded-lg bg-destructive/10 border border-destructive/30">
      <AlertCircle class="size-4 text-destructive mt-0.5 shrink-0" />
      <p class="text-sm text-destructive">{error}</p>
    </div>
  {/if}
</div>
