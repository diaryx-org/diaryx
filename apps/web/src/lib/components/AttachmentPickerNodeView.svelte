<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import {
    Upload,
    X,
  } from "@lucide/svelte";
  import type { Api } from "$lib/backend/api";
  import { getFilename } from "$lib/../models/services/attachmentService";
  import {
    useAttachmentPicker,
    type AttachmentSelection,
  } from "$lib/hooks/useAttachmentPicker.svelte";

  interface Props {
    entryPath: string;
    api: Api | null;
    onSelect: (result: AttachmentSelection) => void;
    onCancel: () => void;
  }

  let { entryPath, api, onSelect, onCancel }: Props = $props();

  const picker = useAttachmentPicker({
    getEntryPath: () => entryPath,
    getApi: () => api,
    onSelect: (result) => onSelect(result),
    autoLoad: true,
  });

  let fileInput: HTMLInputElement | null = $state(null);
</script>

<div class="picker-container">
  <div class="picker-header">
    <span class="picker-title">Insert Attachment</span>
    <button type="button" class="close-btn" onclick={onCancel}>
      <X class="size-4" />
    </button>
  </div>

  <div class="picker-tabs">
    <button
      type="button"
      class="tab-btn"
      class:active={picker.activeTab === "existing"}
      onclick={() => (picker.activeTab = "existing")}
    >
      Select Existing
    </button>
    <button
      type="button"
      class="tab-btn"
      class:active={picker.activeTab === "upload"}
      onclick={() => (picker.activeTab = "upload")}
    >
      Upload New
    </button>
  </div>

  <div class="picker-content">
    {#if picker.activeTab === "existing"}
      {#if picker.loading}
        <div class="status-message">Loading...</div>
      {:else if picker.error}
        <div class="status-message error">{picker.error}</div>
      {:else if picker.groups.length === 0}
        <div class="status-message">No attachments found. Upload one first.</div>
      {:else}
        <div class="attachments-grid">
          {#each picker.groups as group}
            <div class="attachment-group">
              <h4 class="group-title">{group.entryTitle}</h4>
              <div class="group-items">
                {#each group.attachments as attachment}
                  <button
                    type="button"
                    class="attachment-item"
                    onclick={() => picker.handleSelect(attachment, group.entryPath)}
                    use:picker.lazyThumbnailTarget={{ attachment, sourceEntryPath: group.entryPath }}
                  >
                    {#if attachment.kind === "image" && attachment.thumbnail}
                      <img
                        src={attachment.thumbnail}
                        alt=""
                        class="thumbnail"
                      />
                    {:else}
                      {@const IconComponent = picker.getFileIcon(attachment.path)}
                      <div class="file-icon">
                        <IconComponent class="size-6" />
                      </div>
                    {/if}
                    <span class="filename">{getFilename(attachment.path)}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {:else}
      <div
        class="upload-zone"
        class:dragging={picker.isDragging}
        ondragover={picker.handleDragOver}
        ondragleave={picker.handleDragLeave}
        ondrop={picker.handleDrop}
        role="presentation"
      >
        <Upload class="size-8 text-muted-foreground" />
        <p class="upload-text">Drag and drop a file here, or click to select</p>
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
          <p class="error-text">{picker.error}</p>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .picker-container {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--card);
    overflow: hidden;
    margin: 1em 0;
    -webkit-user-select: none;
    user-select: none;
  }

  .picker-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--muted);
  }

  .picker-title {
    font-weight: 600;
    font-size: 14px;
    color: var(--foreground);
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    border: none;
    background: transparent;
    border-radius: 4px;
    cursor: pointer;
    color: var(--muted-foreground);
  }

  .close-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .picker-tabs {
    display: flex;
    gap: 8px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  .tab-btn {
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    cursor: pointer;
    font-size: 13px;
    color: var(--foreground);
  }

  .tab-btn.active {
    background: var(--primary);
    color: var(--primary-foreground);
    border-color: var(--primary);
  }

  .tab-btn:hover:not(.active) {
    background: var(--accent);
  }

  .picker-content {
    padding: 16px;
    max-height: 300px;
    overflow-y: auto;
  }

  .status-message {
    text-align: center;
    padding: 24px;
    color: var(--muted-foreground);
  }

  .status-message.error {
    color: var(--destructive);
  }

  .attachments-grid {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .group-title {
    font-size: 12px;
    font-weight: 500;
    color: var(--muted-foreground);
    margin-bottom: 8px;
  }

  .group-items {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(80px, 1fr));
    gap: 8px;
  }

  .attachment-item {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .attachment-item:hover {
    border-color: var(--primary);
    background: var(--accent);
  }

  .thumbnail {
    width: 60px;
    height: 60px;
    object-fit: cover;
    border-radius: 4px;
  }

  .file-icon {
    width: 60px;
    height: 60px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--muted);
    border-radius: 4px;
    color: var(--muted-foreground);
  }

  .filename {
    font-size: 11px;
    margin-top: 4px;
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 80px;
    color: var(--foreground);
  }

  .upload-zone {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 32px;
    border: 2px dashed var(--border);
    border-radius: 8px;
    text-align: center;
    transition: all 0.15s ease;
  }

  .upload-zone.dragging {
    border-color: var(--primary);
    background: var(--accent);
  }

  .upload-text {
    color: var(--muted-foreground);
    font-size: 14px;
  }

  .hidden {
    display: none;
  }

  .error-text {
    color: var(--destructive);
    font-size: 13px;
  }
</style>
