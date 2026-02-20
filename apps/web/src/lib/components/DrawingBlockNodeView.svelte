<script lang="ts">
  import { Pencil } from "@lucide/svelte";
  import type { Api } from "$lib/backend/api";
  import DrawingCanvas from "./DrawingCanvas.svelte";

  interface Props {
    src: string;
    alt: string;
    width: number;
    height: number;
    readonly: boolean;
    entryPath: string;
    api: Api | null;
    onUpdate: (attrs: Record<string, unknown>) => void;
    onDrawingSave?: (result: {
      blobUrl: string;
      attachmentPath: string;
    }) => void;
  }

  let {
    src,
    alt,
    width,
    height,
    readonly,
    entryPath,
    api,
    onUpdate,
    onDrawingSave,
  }: Props = $props();

  let editing = $state(false);

  // Auto-open the canvas for new drawings (no src yet)
  $effect(() => {
    if (!src && !readonly) {
      editing = true;
    }
  });

  function handleEdit() {
    if (readonly) return;
    editing = true;
  }

  function handleSave(result: {
    blobUrl: string;
    attachmentPath: string;
    svgWidth: number;
    svgHeight: number;
  }) {
    onUpdate({
      src: result.blobUrl,
      alt,
      width: result.svgWidth,
      height: result.svgHeight,
    });
    onDrawingSave?.({
      blobUrl: result.blobUrl,
      attachmentPath: result.attachmentPath,
    });
    editing = false;
  }

  function handleCancel() {
    editing = false;
  }
</script>

<div class="drawing-block-container">
  {#if editing}
    <DrawingCanvas
      {src}
      {width}
      {height}
      {entryPath}
      {api}
      onSave={handleSave}
      onCancel={handleCancel}
    />
  {:else if src}
    <div class="drawing-block-preview" role="img" aria-label={alt || "Drawing"}>
      <img
        src={src}
        alt={alt || "Drawing"}
        class="drawing-block-image"
        draggable="false"
      />
      {#if !readonly}
        <button
          type="button"
          class="drawing-block-edit-btn"
          title="Edit drawing"
          onclick={handleEdit}
        >
          <Pencil class="size-3.5" />
          <span>Edit</span>
        </button>
      {/if}
    </div>
  {:else}
    <div class="drawing-block-empty">
      <span>Empty drawing</span>
    </div>
  {/if}
</div>

<style>
  .drawing-block-container {
    margin: 0.5em 0;
    border-radius: 6px;
    overflow: hidden;
  }

  .drawing-block-preview {
    position: relative;
    display: inline-block;
    max-width: 100%;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }

  .drawing-block-image {
    display: block;
    max-width: 100%;
    height: auto;
    border-radius: 6px;
  }

  .drawing-block-edit-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px;
    border: none;
    border-radius: 6px;
    background: var(--popover);
    color: var(--foreground);
    font-size: 12px;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.15s ease;
    box-shadow:
      0 1px 3px rgba(0, 0, 0, 0.12),
      0 1px 2px rgba(0, 0, 0, 0.06);
  }

  .drawing-block-preview:hover .drawing-block-edit-btn {
    opacity: 1;
  }

  .drawing-block-edit-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .drawing-block-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100px;
    border: 1px dashed var(--border);
    border-radius: 6px;
    color: var(--muted-foreground);
    font-style: italic;
    font-size: 13px;
  }
</style>
