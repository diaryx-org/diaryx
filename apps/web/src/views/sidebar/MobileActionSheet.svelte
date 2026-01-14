<script lang="ts">
  /**
   * MobileActionSheet - Mobile-friendly context menu for tree items
   *
   * Uses shadcn Drawer (vaul-svelte) for native-feeling bottom sheet behavior
   * with proper touch handling, drag-to-dismiss, and accessibility.
   */

  import * as Drawer from "$lib/components/ui/drawer";
  import {
    Plus,
    Clipboard,
    Download,
    Paperclip,
    SearchCheck,
    Trash2,
  } from "@lucide/svelte";

  interface Props {
    open: boolean;
    nodePath: string;
    nodeName: string;
    onClose: () => void;
    onCreateChild: (path: string) => void;
    onExport: (path: string) => void;
    onAddAttachment: (path: string) => void;
    onValidate?: (path: string) => void;
    onDelete: (path: string) => void;
  }

  let {
    open = $bindable(),
    nodePath,
    nodeName,
    onClose,
    onCreateChild,
    onExport,
    onAddAttachment,
    onValidate,
    onDelete,
  }: Props = $props();

  // Action handlers that close the sheet after action
  function handleAction(action: () => void) {
    action();
    onClose();
  }

  async function handleCopyPath() {
    try {
      await navigator.clipboard.writeText(nodePath);
    } catch (e) {
      console.error("Failed to copy path:", e);
    }
    onClose();
  }

  function handleOpenChange(isOpen: boolean) {
    if (!isOpen) {
      onClose();
    }
  }
</script>

<Drawer.Root bind:open onOpenChange={handleOpenChange}>
  <Drawer.Content>
    <div class="mx-auto w-full max-w-sm">
      <Drawer.Header>
        <Drawer.Title>{nodeName.replace(".md", "")}</Drawer.Title>
      </Drawer.Header>

      <div class="flex flex-col pb-4">
        <!-- New Entry -->
        <button
          type="button"
          class="flex items-center gap-4 px-6 py-4 hover:bg-muted active:bg-muted/80 transition-colors text-left"
          onclick={() => handleAction(() => onCreateChild(nodePath))}
        >
          <Plus class="size-5 text-muted-foreground" />
          <span class="text-base">New Entry Here</span>
        </button>

        <!-- Copy Path -->
        <button
          type="button"
          class="flex items-center gap-4 px-6 py-4 hover:bg-muted active:bg-muted/80 transition-colors text-left"
          onclick={handleCopyPath}
        >
          <Clipboard class="size-5 text-muted-foreground" />
          <span class="text-base">Copy Path</span>
        </button>

        <!-- Export -->
        <button
          type="button"
          class="flex items-center gap-4 px-6 py-4 hover:bg-muted active:bg-muted/80 transition-colors text-left"
          onclick={() => handleAction(() => onExport(nodePath))}
        >
          <Download class="size-5 text-muted-foreground" />
          <span class="text-base">Export...</span>
        </button>

        <!-- Add Attachment -->
        <button
          type="button"
          class="flex items-center gap-4 px-6 py-4 hover:bg-muted active:bg-muted/80 transition-colors text-left"
          onclick={() => handleAction(() => onAddAttachment(nodePath))}
        >
          <Paperclip class="size-5 text-muted-foreground" />
          <span class="text-base">Add Attachment...</span>
        </button>

        <!-- Validate -->
        {#if onValidate}
          <button
            type="button"
            class="flex items-center gap-4 px-6 py-4 hover:bg-muted active:bg-muted/80 transition-colors text-left"
            onclick={() => handleAction(() => onValidate(nodePath))}
          >
            <SearchCheck class="size-5 text-muted-foreground" />
            <span class="text-base">Validate</span>
          </button>
        {/if}

        <!-- Separator -->
        <div class="border-t border-border my-2"></div>

        <!-- Delete - Destructive -->
        <button
          type="button"
          class="flex items-center gap-4 px-6 py-4 hover:bg-destructive/10 active:bg-destructive/20 transition-colors text-left text-destructive"
          onclick={() => handleAction(() => onDelete(nodePath))}
        >
          <Trash2 class="size-5" />
          <span class="text-base">Delete</span>
        </button>
      </div>
    </div>
  </Drawer.Content>
</Drawer.Root>
