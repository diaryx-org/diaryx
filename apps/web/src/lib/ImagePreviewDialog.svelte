<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { X } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";

  interface Props {
    open: boolean;
    imageUrl: string | null;
    imageName: string;
    onOpenChange: (open: boolean) => void;
  }

  let { open, imageUrl, imageName, onOpenChange }: Props = $props();
</script>

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Content
    class="max-w-[90vw] max-h-[90vh] w-auto p-0 overflow-hidden bg-black/95 border-none"
    showCloseButton={false}
  >
    <div class="relative flex items-center justify-center w-full h-full">
      <!-- Header bar -->
      <div class="absolute top-0 left-0 right-0 flex items-center justify-between px-4 py-2 bg-gradient-to-b from-black/60 to-transparent z-10">
        <span class="text-sm text-white/80 truncate">{imageName}</span>
        <Button
          variant="ghost"
          size="icon"
          class="size-8 text-white/80 hover:text-white hover:bg-white/10"
          onclick={() => onOpenChange(false)}
          aria-label="Close preview"
        >
          <X class="size-5" />
        </Button>
      </div>

      <!-- Image -->
      {#if imageUrl}
        <img
          src={imageUrl}
          alt={imageName}
          class="max-w-[88vw] max-h-[85vh] object-contain select-none"
          draggable="false"
        />
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
