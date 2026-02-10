<script lang="ts">
  /**
   * SyncStatusIndicator - Shows current sync status with visual feedback
   *
   * Displays a small indicator showing:
   * - Connected & synced (green dot)
   * - Syncing (yellow dot with animation)
   * - Disconnected/Error (red dot)
   * - Not configured (gray dot)
   *
   * Click opens Settings dialog on the Sync tab.
   */
  import { Button } from "$lib/components/ui/button";
  import * as Popover from "$lib/components/ui/popover";
  import { Progress } from "$lib/components/ui/progress";
  import { collaborationStore, type SyncStatus } from "@/models/stores/collaborationStore.svelte";
  import { getAuthState } from "$lib/auth";
  import {
    Cloud,
    CloudOff,
    RefreshCw,
    AlertCircle,
    CheckCircle,
  } from "@lucide/svelte";

  interface Props {
    onOpenWizard?: () => void;
  }

  let { onOpenWizard }: Props = $props();

  // Reactive state from stores
  // Use effectiveSyncStatus which considers BOTH metadata AND body sync
  let syncStatus = $derived(collaborationStore.effectiveSyncStatus);
  let bodySyncStatus = $derived(collaborationStore.bodySyncStatus);
  let syncProgress = $derived(collaborationStore.syncProgress);
  let bodySyncProgress = $derived(collaborationStore.bodySyncProgress);
  let syncError = $derived(collaborationStore.syncError);
  let authState = $derived(getAuthState());

  // Combined progress: show body sync progress when metadata is synced but body is still syncing
  let displayProgress = $derived.by(() => {
    // If body sync has progress and metadata is synced, show body sync progress
    if (bodySyncProgress && collaborationStore.syncStatus === 'synced') {
      return bodySyncProgress;
    }
    // Otherwise show metadata sync progress
    return syncProgress;
  });

  // Status display config
  const statusConfig: Record<SyncStatus, {
    icon: typeof Cloud;
    color: string;
    dotColor: string;
    label: string;
    animate?: boolean;
  }> = {
    'not_configured': {
      icon: CloudOff,
      color: 'text-muted-foreground',
      dotColor: 'bg-muted-foreground',
      label: 'Sync not configured',
    },
    'idle': {
      icon: Cloud,
      color: 'text-muted-foreground',
      dotColor: 'bg-muted-foreground',
      label: 'Sync idle',
    },
    'connecting': {
      icon: RefreshCw,
      color: 'text-amber-500',
      dotColor: 'bg-amber-500',
      label: 'Connecting...',
      animate: true,
    },
    'syncing': {
      icon: RefreshCw,
      color: 'text-amber-500',
      dotColor: 'bg-amber-500',
      label: 'Syncing...',
      animate: true,
    },
    'synced': {
      icon: CheckCircle,
      color: 'text-green-500',
      dotColor: 'bg-green-500',
      label: 'Synced',
    },
    'error': {
      icon: AlertCircle,
      color: 'text-destructive',
      dotColor: 'bg-destructive',
      label: 'Sync error',
    },
  };

  let config = $derived(statusConfig[syncStatus]);
  let StatusIcon = $derived(config.icon);
</script>

<Popover.Root>
  <Popover.Trigger>
    <Button
      variant="ghost"
      size="sm"
      class="h-8 gap-1.5 px-2 {config.color}"
      aria-label="Sync status"
    >
      <!-- Status dot -->
      <span
        class="relative flex h-2 w-2"
      >
        {#if config.animate}
          <span class="animate-ping absolute inline-flex h-full w-full rounded-full {config.dotColor} opacity-75"></span>
        {/if}
        <span class="relative inline-flex rounded-full h-2 w-2 {config.dotColor}"></span>
      </span>

      <!-- Icon -->
      <StatusIcon
        class="size-4 {config.animate ? 'animate-spin' : ''}"
      />

      <!-- Label (hidden on mobile) -->
      <span class="hidden sm:inline text-xs">
        {#if syncStatus === 'syncing' && displayProgress && displayProgress.total > 0}
          {displayProgress.completed}/{displayProgress.total}
        {:else if authState.isAuthenticated && syncStatus === 'synced'}
          Synced
        {:else if authState.isAuthenticated}
          Sync
        {:else}
          Sync
        {/if}
      </span>
    </Button>
  </Popover.Trigger>

  <Popover.Content class="w-64 p-3" align="end">
    <div class="space-y-3">
      <!-- Status header -->
      <div class="flex items-center gap-2">
        <StatusIcon
          class="size-5 {config.color} {config.animate ? 'animate-spin' : ''}"
        />
        <span class="font-medium text-sm">{config.label}</span>
      </div>

      <!-- Progress bar when syncing -->
      {#if syncStatus === 'syncing' && displayProgress}
        {@const progress = displayProgress}
        {@const percent = progress.total > 0 ? Math.round((progress.completed / progress.total) * 100) : 0}
        <div class="space-y-1">
          <Progress value={percent} class="h-2" />
          <p class="text-xs text-muted-foreground">
            {progress.completed} of {progress.total} files
            {#if bodySyncStatus === 'syncing' && collaborationStore.syncStatus === 'synced'}
              (downloading content)
            {/if}
          </p>
        </div>
      {/if}

      <!-- Error message -->
      {#if syncError}
        <p class="text-xs text-destructive bg-destructive/10 p-2 rounded-md">
          {syncError}
        </p>
      {/if}

      <!-- Account info when authenticated -->
      {#if authState.isAuthenticated && authState.user}
        <div class="text-xs text-muted-foreground border-t pt-2">
          <p>Signed in as <strong>{authState.user.email}</strong></p>
          <p class="mt-1">{authState.devices.length} device(s) connected</p>
        </div>
      {:else if syncStatus === 'not_configured'}
        <p class="text-xs text-muted-foreground">
          Set up sync to access your notes from any device.
        </p>
      {/if}

      <!-- Action button -->
      <Button
        variant="outline"
        size="sm"
        class="w-full text-xs"
        onclick={onOpenWizard}
      >
        {#if authState.isAuthenticated}
          Manage sync
        {:else}
          Set up sync
        {/if}
      </Button>
    </div>
  </Popover.Content>
</Popover.Root>
