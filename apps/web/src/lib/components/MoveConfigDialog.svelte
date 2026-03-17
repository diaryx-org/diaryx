<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import type { Api } from "$lib/backend/api";

  interface Props {
    open: boolean;
    sectionKey: string | null;
    sourcePath: string;
    api: Api | null;
    onOpenChange?: (open: boolean) => void;
    onSuccess?: () => void;
  }

  let {
    open = $bindable(false),
    sectionKey,
    sourcePath,
    api,
    onOpenChange,
    onSuccess,
  }: Props = $props();

  let targetPath = $state("Meta/Config.md");
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function handleConfirm() {
    if (!api || !sectionKey || !targetPath.trim()) return;
    loading = true;
    error = null;
    try {
      await api.moveFrontmatterSectionToFile(sourcePath, sectionKey, targetPath.trim(), true);
      open = false;
      onOpenChange?.(false);
      onSuccess?.();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function handleOpenChange(newOpen: boolean) {
    open = newOpen;
    onOpenChange?.(newOpen);
    if (!newOpen) {
      error = null;
      loading = false;
    }
  }
</script>

<Dialog.Root bind:open onOpenChange={handleOpenChange}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Move to another file</Dialog.Title>
      <Dialog.Description>
        Move <code class="text-xs bg-muted px-1 py-0.5 rounded">{sectionKey}</code> to an external file. The property will be replaced with a link.
      </Dialog.Description>
    </Dialog.Header>
    <div class="space-y-4 py-4">
      <div class="space-y-2">
        <label for="target-path" class="text-sm font-medium">Target file path</label>
        <Input
          id="target-path"
          bind:value={targetPath}
          placeholder="Meta/Config.md"
          class="h-9"
          onkeydown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              handleConfirm();
            }
          }}
        />
      </div>
      {#if error}
        <p class="text-sm text-destructive">{error}</p>
      {/if}
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => handleOpenChange(false)}>
        Cancel
      </Button>
      <Button onclick={handleConfirm} disabled={loading || !targetPath.trim()}>
        {#if loading}
          Moving...
        {:else}
          Move
        {/if}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
