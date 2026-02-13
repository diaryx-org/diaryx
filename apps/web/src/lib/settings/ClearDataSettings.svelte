<script lang="ts">
  /**
   * ClearDataSettings - Clear all locally stored data
   *
   * Allows users to wipe all local data including:
   * - OPFS storage (workspace files, CRDT data)
   * - IndexedDB databases
   * - localStorage (settings, auth tokens)
   *
   * After clearing, the page is refreshed to start fresh.
   */
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Trash2, AlertTriangle, Loader2 } from "@lucide/svelte";
  import { isTauri } from "$lib/backend/interface";
  import { clearAllLocalData } from "./clearData";

  // State
  let showConfirmDialog = $state(false);
  let isClearing = $state(false);
  let error = $state<string | null>(null);

  /**
   * Clear all local data and refresh the page.
   */
  async function handleClearData() {
    isClearing = true;
    error = null;

    try {
      showConfirmDialog = false;
      await clearAllLocalData();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to clear data";
      isClearing = false;
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Trash2 class="size-4" />
    Clear Local Data
  </h3>

  <div class="px-1 space-y-2">
    {#if isTauri()}
      <p class="text-xs text-muted-foreground">
        This option is not available in the desktop app. Your files are stored directly on your device.
      </p>
    {:else}
      <p class="text-xs text-muted-foreground">
        Delete all locally stored data including your workspace files, settings, and credentials.
        This will reset the app to its initial state.
      </p>

      <Button
        variant="destructive"
        size="sm"
        onclick={() => (showConfirmDialog = true)}
      >
        <Trash2 class="size-4 mr-2" />
        Clear All Local Data
      </Button>
    {/if}
  </div>
</div>

<!-- Confirmation Dialog -->
<Dialog.Root bind:open={showConfirmDialog}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2 text-destructive">
        <AlertTriangle class="size-5" />
        Clear All Local Data
      </Dialog.Title>
      <Dialog.Description>
        This will permanently delete all data stored in your browser:
      </Dialog.Description>
    </Dialog.Header>

    <ul class="list-disc list-inside text-sm text-muted-foreground space-y-1 py-2">
      <li>All workspace files and notes</li>
      <li>CRDT sync data</li>
      <li>App settings and preferences</li>
      <li>Login credentials and tokens</li>
      <li>Cloud backup configurations</li>
    </ul>

    <p class="text-sm font-medium text-destructive">
      This action cannot be undone. Make sure to export your data first if you want to keep it.
    </p>

    {#if error}
      <p class="text-sm text-destructive bg-destructive/10 p-2 rounded">
        {error}
      </p>
    {/if}

    <Dialog.Footer class="gap-2 sm:gap-0">
      <Button
        variant="outline"
        onclick={() => (showConfirmDialog = false)}
        disabled={isClearing}
      >
        Cancel
      </Button>
      <Button
        variant="destructive"
        onclick={handleClearData}
        disabled={isClearing}
      >
        {#if isClearing}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Clearing...
        {:else}
          Clear Everything
        {/if}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
