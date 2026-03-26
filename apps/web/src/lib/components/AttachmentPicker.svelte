<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import {
    Upload,
    FolderOpen,
  } from "@lucide/svelte";
  import type { Api } from "$lib/backend/api";
  import { getFilename } from "$lib/../models/services/attachmentService";
  import {
    useAttachmentPicker,
    type AttachmentSelection,
  } from "$lib/hooks/useAttachmentPicker.svelte";

  export type { AttachmentSelection };

  interface Props {
    open: boolean;
    entryPath: string;
    api: Api | null;
    onSelect: (result: AttachmentSelection | null) => void;
    onClose: () => void;
  }

  let { open = $bindable(), entryPath, api, onSelect, onClose }: Props = $props();

  const picker = useAttachmentPicker({
    getEntryPath: () => entryPath,
    getApi: () => api,
    onSelect: (result) => {
      onSelect(result);
      onClose();
    },
    autoLoad: false,
  });

  let fileInput: HTMLInputElement | null = $state(null);

  // Track previous open state to detect transitions
  let prevOpen = false;

  // Single effect to handle open/close transitions
  $effect(() => {
    const currentOpen = open;

    if (currentOpen && !prevOpen) {
      // Dialog just opened - load attachments
      prevOpen = true;
      if (api && entryPath) {
        picker.load();
      }
    } else if (!currentOpen && prevOpen) {
      // Dialog just closed - schedule cleanup
      prevOpen = false;
      // Use setTimeout to avoid state updates during effect
      setTimeout(() => {
        picker.reset();
      }, 0);
    }
  });

  function handleOpenChange(isOpen: boolean) {
    open = isOpen;
    if (!isOpen) {
      onClose();
    }
  }
</script>

<Dialog.Root {open} onOpenChange={handleOpenChange}>
  <Dialog.Content class="sm:max-w-2xl max-h-[80vh] flex flex-col">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <FolderOpen class="size-5" />
        Insert Attachment
      </Dialog.Title>
      <Dialog.Description>
        Select an existing attachment or upload a new one.
      </Dialog.Description>
    </Dialog.Header>

    <!-- Tab buttons -->
    <div class="flex gap-2 border-b pb-2">
      <Button
        variant={picker.activeTab === "existing" ? "default" : "outline"}
        size="sm"
        onclick={() => (picker.activeTab = "existing")}
      >
        <FolderOpen class="size-4 mr-2" />
        Select Existing
      </Button>
      <Button
        variant={picker.activeTab === "upload" ? "default" : "outline"}
        size="sm"
        onclick={() => (picker.activeTab = "upload")}
      >
        <Upload class="size-4 mr-2" />
        Upload New
      </Button>
    </div>

    <!-- Content based on active tab -->
    <div class="flex-1 overflow-y-auto min-h-[200px]">
      {#if picker.activeTab === "existing"}
        {#if picker.loading}
          <div class="p-4 text-center text-muted-foreground">Loading...</div>
        {:else if picker.error}
          <div class="p-4 text-center text-destructive">{picker.error}</div>
        {:else if picker.groups.length === 0}
          <div class="p-4 text-center text-muted-foreground">
            No attachments found. Upload one first.
          </div>
        {:else}
          <div class="p-2 space-y-4">
            {#each picker.groups as group}
              <div>
                <h4
                  class="text-sm font-medium text-muted-foreground mb-2 px-2"
                >
                  {group.entryTitle}
                </h4>
                <div
                  class="grid gap-2"
                  style="grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));"
                >
                  {#each group.attachments as attachment}
                    <button
                      type="button"
                      class="attachment-item group relative flex flex-col items-center p-2 rounded-lg border border-transparent hover:border-primary hover:bg-accent transition-colors cursor-pointer"
                      onclick={() => picker.handleSelect(attachment, group.entryPath)}
                      use:picker.lazyThumbnailTarget={{ attachment, sourceEntryPath: group.entryPath }}
                    >
                      {#if attachment.kind === "image" && attachment.thumbnail}
                        <img
                          src={attachment.thumbnail}
                          alt=""
                          class="w-full h-20 object-cover rounded"
                        />
                      {:else}
                        {@const IconComponent = picker.getFileIcon(attachment.path)}
                        <div
                          class="w-full h-20 flex items-center justify-center bg-muted rounded"
                        >
                          <IconComponent class="size-8 text-muted-foreground" />
                        </div>
                      {/if}
                      <span
                        class="text-xs mt-1 text-center truncate w-full px-1"
                        title={getFilename(attachment.path)}
                      >
                        {getFilename(attachment.path)}
                      </span>
                    </button>
                  {/each}
                </div>
              </div>
            {/each}
          </div>
        {/if}
      {:else}
        <!-- Upload tab content -->
        <div
          class="p-4 h-full"
          ondragover={picker.handleDragOver}
          ondragleave={picker.handleDragLeave}
          ondrop={picker.handleDrop}
          role="presentation"
        >
          <div
            class="border-2 border-dashed rounded-lg p-8 text-center transition-colors h-full flex flex-col items-center justify-center"
            class:border-primary={picker.isDragging}
            class:bg-accent={picker.isDragging}
          >
            <Upload class="size-12 text-muted-foreground mb-4" />
            <p class="text-muted-foreground mb-4">
              Drag and drop a file here, or click to select
            </p>
            <input
              type="file"
              bind:this={fileInput}
              onchange={picker.handleFileInputChange}
              class="hidden"
            />
            <Button onclick={() => fileInput?.click()} disabled={picker.loading}>
              {picker.loading ? "Uploading..." : "Choose File"}
            </Button>
            {#if picker.error}
              <p class="text-destructive mt-4 text-sm">{picker.error}</p>
            {/if}
          </div>
        </div>
      {/if}
    </div>

    </Dialog.Content>
</Dialog.Root>
