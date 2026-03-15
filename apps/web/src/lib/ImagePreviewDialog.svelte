<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Loader2, X } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";
  import type { AttachmentMediaKind } from "@/models/services/attachmentService";

  interface Props {
    open: boolean;
    mediaUrl: string | null;
    mediaName: string;
    mediaKind?: AttachmentMediaKind;
    loading?: boolean;
    onOpenChange: (open: boolean) => void;
  }

  let {
    open,
    mediaUrl,
    mediaName,
    mediaKind = "image",
    loading = false,
    onOpenChange,
  }: Props = $props();
</script>

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Content
    class="!inset-0 !translate-x-0 !translate-y-0 !start-0 !top-0 flex items-center justify-center max-w-none w-full h-full p-0 overflow-hidden bg-black/95 border-none rounded-none data-[state=open]:!zoom-in-100 data-[state=closed]:!zoom-out-100"
    showCloseButton={false}
  >
    <div class="relative flex items-center justify-center w-full h-full max-w-[90vw] max-h-[90vh] mx-auto">
      <!-- Header bar -->
      <div class="absolute top-0 left-0 right-0 flex items-center justify-between px-4 py-2 bg-gradient-to-b from-black/60 to-transparent z-10">
        <span class="text-sm text-white/80 truncate">{mediaName}</span>
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

      {#if mediaUrl}
        {#if mediaKind === "video"}
          <!-- svelte-ignore a11y_media_has_caption -->
          <video
            src={mediaUrl}
            controls
            preload="metadata"
            playsinline
            class="max-w-[88vw] max-h-[85vh] rounded object-contain"
          ></video>
        {:else if mediaKind === "audio"}
          <div class="flex items-center justify-center w-[min(36rem,88vw)] h-[24rem] px-6">
            <audio
              src={mediaUrl}
              controls
              preload="metadata"
              class="w-full"
            ></audio>
          </div>
        {:else}
          <img
            src={mediaUrl}
            alt={mediaName}
            class="max-w-[88vw] max-h-[85vh] object-contain select-none"
            draggable="false"
          />
        {/if}
      {:else if loading}
        <div class="flex items-center justify-center w-[60vw] h-[50vh]">
          <Loader2 class="size-8 text-white/70 animate-spin" />
        </div>
      {/if}

      {#if loading}
        <div class="absolute inset-x-0 bottom-0 flex items-center justify-center pb-4 pointer-events-none">
          <div class="inline-flex items-center gap-2 rounded-full bg-black/60 px-3 py-1 text-xs text-white/80">
            <Loader2 class="size-3.5 animate-spin" />
            {mediaKind === "image" ? "Loading full image" : "Loading preview"}
          </div>
        </div>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
