<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import {
    getDeviceReplacementContext,
    clearDeviceReplacement,
    retryWithDeviceReplacement,
  } from "$lib/auth/authStore.svelte";
  import { Loader2, Monitor } from "@lucide/svelte";

  interface Props {
    onAuthenticated?: () => void;
  }

  let { onAuthenticated }: Props = $props();

  let isReplacing = $state(false);
  let error = $state<string | null>(null);

  let ctx = $derived(getDeviceReplacementContext());
  let open = $derived(ctx !== null);

  function formatDate(iso: string): string {
    try {
      return new Date(iso).toLocaleDateString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
    } catch {
      return iso;
    }
  }

  async function handleReplace(deviceId: string) {
    isReplacing = true;
    error = null;
    try {
      await retryWithDeviceReplacement(deviceId);
      onAuthenticated?.();
    } catch (err) {
      error = err instanceof Error ? err.message : "Failed to replace device";
    } finally {
      isReplacing = false;
    }
  }

  function handleClose() {
    clearDeviceReplacement();
    error = null;
  }
</script>

<Dialog.Root {open} onOpenChange={(v) => { if (!v) handleClose(); }}>
  <Dialog.Content class="sm:max-w-[400px]">
    <Dialog.Header>
      <Dialog.Title>Device limit reached</Dialog.Title>
      <Dialog.Description>
        Choose a device to sign out of and replace with this one.
      </Dialog.Description>
    </Dialog.Header>

    {#if error}
      <p class="text-xs text-destructive">{error}</p>
    {/if}

    <div class="space-y-2">
      {#if ctx}
        {#each ctx.devices as device}
          <button
            class="w-full flex items-center gap-3 rounded-md border p-3 text-left hover:bg-muted/50 transition-colors disabled:opacity-50"
            disabled={isReplacing}
            onclick={() => handleReplace(device.id)}
          >
            <Monitor class="size-5 shrink-0 text-muted-foreground" />
            <div class="flex-1 min-w-0">
              <p class="text-sm font-medium truncate">
                {device.name || "Unnamed device"}
              </p>
              <p class="text-xs text-muted-foreground">
                Last seen {formatDate(device.last_seen_at)}
              </p>
            </div>
            {#if isReplacing}
              <Loader2 class="size-4 animate-spin text-muted-foreground" />
            {/if}
          </button>
        {/each}
      {/if}
    </div>

    <Dialog.Footer>
      <Button variant="ghost" size="sm" onclick={handleClose} disabled={isReplacing}>
        Cancel
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
