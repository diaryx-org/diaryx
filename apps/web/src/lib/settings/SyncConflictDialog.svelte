<script lang="ts">
  /**
   * SyncConflictDialog - Displays sync conflicts and allows user to resolve them.
   */
  import { Button } from "$lib/components/ui/button";
  import {
    AlertTriangle,
    FileText,
    Clock,
    Check,
    X,
  } from "@lucide/svelte";
  import type { ConflictInfo } from "$lib/crdt/p2pSyncBridge";

  interface Props {
    conflicts: ConflictInfo[];
    onResolve: (resolutions: Map<string, 'local' | 'remote' | 'both'>) => void;
    onCancel: () => void;
  }

  let { conflicts, onResolve, onCancel }: Props = $props();

  // Track resolutions for each conflict
  let resolutions: Map<string, 'local' | 'remote' | 'both'> = $state(new Map());

  // Initialize with default resolutions (prefer remote/newer)
  $effect(() => {
    const newResolutions = new Map<string, 'local' | 'remote' | 'both'>();
    for (const conflict of conflicts) {
      // Default to newer version
      if (conflict.remoteModified > conflict.localModified) {
        newResolutions.set(conflict.path, 'remote');
      } else {
        newResolutions.set(conflict.path, 'local');
      }
    }
    resolutions = newResolutions;
  });

  function setResolution(path: string, resolution: 'local' | 'remote' | 'both') {
    resolutions = new Map(resolutions).set(path, resolution);
  }

  function handleResolve() {
    onResolve(resolutions);
  }

  function formatTime(timestamp: number): string {
    const date = new Date(timestamp);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return 'Just now';
    if (diffMins < 60) return `${diffMins} minute${diffMins !== 1 ? 's' : ''} ago`;
    if (diffHours < 24) return `${diffHours} hour${diffHours !== 1 ? 's' : ''} ago`;
    if (diffDays < 7) return `${diffDays} day${diffDays !== 1 ? 's' : ''} ago`;
    
    return date.toLocaleDateString();
  }

  function getFileName(path: string): string {
    return path.split('/').pop() || path;
  }
</script>

<!-- Modal backdrop -->
<div
  class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
  role="dialog"
  aria-modal="true"
  aria-labelledby="conflict-dialog-title"
>
  <div class="bg-background rounded-lg shadow-xl max-w-lg w-full mx-4 max-h-[80vh] flex flex-col">
    <!-- Header -->
    <div class="flex items-center gap-3 p-4 border-b">
      <div class="p-2 rounded-full bg-amber-100 text-amber-600">
        <AlertTriangle class="size-5" />
      </div>
      <div>
        <h2 id="conflict-dialog-title" class="text-lg font-semibold">
          Sync Conflicts
        </h2>
        <p class="text-sm text-muted-foreground">
          {conflicts.length} file{conflicts.length !== 1 ? 's' : ''} modified on both devices
        </p>
      </div>
    </div>

    <!-- Conflict list -->
    <div class="p-4 overflow-y-auto flex-1 space-y-4">
      {#each conflicts as conflict}
        {@const resolution = resolutions.get(conflict.path)}
        <div class="border rounded-lg p-3 space-y-3">
          <!-- File name -->
          <div class="flex items-center gap-2">
            <FileText class="size-4 text-muted-foreground" />
            <span class="font-medium text-sm truncate" title={conflict.path}>
              {getFileName(conflict.path)}
            </span>
          </div>

          <!-- Timestamps -->
          <div class="grid grid-cols-2 gap-2 text-xs">
            <div class="flex items-center gap-1.5 text-muted-foreground">
              <Clock class="size-3" />
              <span>This device: {formatTime(conflict.localModified)}</span>
            </div>
            <div class="flex items-center gap-1.5 text-muted-foreground">
              <Clock class="size-3" />
              <span>Other device: {formatTime(conflict.remoteModified)}</span>
            </div>
          </div>

          <!-- Resolution options -->
          <div class="flex gap-2">
            <Button
              variant={resolution === 'local' ? 'default' : 'outline'}
              size="sm"
              class="flex-1 text-xs h-8"
              onclick={() => setResolution(conflict.path, 'local')}
            >
              {#if resolution === 'local'}
                <Check class="size-3 mr-1" />
              {/if}
              Keep This
            </Button>
            <Button
              variant={resolution === 'remote' ? 'default' : 'outline'}
              size="sm"
              class="flex-1 text-xs h-8"
              onclick={() => setResolution(conflict.path, 'remote')}
            >
              {#if resolution === 'remote'}
                <Check class="size-3 mr-1" />
              {/if}
              Keep Other
            </Button>
            <Button
              variant={resolution === 'both' ? 'default' : 'outline'}
              size="sm"
              class="flex-1 text-xs h-8"
              onclick={() => setResolution(conflict.path, 'both')}
            >
              {#if resolution === 'both'}
                <Check class="size-3 mr-1" />
              {/if}
              Keep Both
            </Button>
          </div>
        </div>
      {/each}
    </div>

    <!-- Footer -->
    <div class="flex items-center justify-end gap-2 p-4 border-t">
      <Button variant="ghost" onclick={onCancel}>
        <X class="size-4 mr-1" />
        Cancel Sync
      </Button>
      <Button onclick={handleResolve}>
        <Check class="size-4 mr-1" />
        Resolve & Continue
      </Button>
    </div>
  </div>
</div>
