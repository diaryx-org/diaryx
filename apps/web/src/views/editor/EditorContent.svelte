<script lang="ts">
  /**
   * EditorContent - The main editor content area
   *
   * Wraps the TipTap editor with loading states.
   * This component handles the editor rendering logic.
   */

  import LoadingSpinner from "../shared/LoadingSpinner.svelte";
  import type { Api } from "$lib/backend/api";
  import type { AttachmentMediaKind } from "@/models/services/attachmentService";

  interface Props {
    Editor: typeof import("$lib/Editor.svelte").default | null;
    editorRef: any;
    content: string;
    editorKey: string;
    readonly?: boolean;
    onchange: () => void;
    onblur: () => void;
    // These match the Editor component prop types
    onFileDrop?: (
      file: File,
    ) => Promise<{ blobUrl: string; attachmentPath: string; kind: AttachmentMediaKind } | null>;
    onLinkClick?: (href: string) => void;
    // Attachment picker props
    entryPath?: string;
    api?: Api | null;
    onAttachmentInsert?: (selection: {
      path: string;
      kind: AttachmentMediaKind;
      blobUrl?: string;
      sourceEntryPath: string;
    }) => void;
  }

  let {
    Editor,
    editorRef = $bindable(),
    content,
    editorKey,
    readonly = false,
    onchange,
    onblur,
    onFileDrop,
    onLinkClick,
    entryPath,
    api,
    onAttachmentInsert,
  }: Props = $props();
</script>

<!-- Outer container: scrollable area -->
<div class="flex-1 overflow-y-auto">
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- Inner wrapper: padding and max-width controlled by --editor-content-max-width -->
  <div
    class="px-4 py-8 md:px-6 md:py-12 min-h-full mx-auto"
    style:max-width="var(--editor-content-max-width)"
    onclick={(e) => {
      // Only handle clicks directly on this container (not bubbled from editor content)
      // This allows clicking in the empty space below the editor to focus at the end
      if (e.target === e.currentTarget) {
        // Only trigger when clicking below the editor content, not on side padding
        const editorEl = (e.currentTarget as HTMLElement).querySelector('.editor-content');
        if (editorEl) {
          const rect = editorEl.getBoundingClientRect();
          if (e.clientY > rect.bottom) {
            editorRef?.focusAtEnd?.();
          }
        }
      }
    }}
  >
    {#if Editor}
      {#key editorKey}
        <Editor
          debugMenus={false}
          bind:this={editorRef}
          {content}
          {onchange}
          {onblur}
          placeholder={readonly ? "" : "Start writing..."}
          {readonly}
          {onFileDrop}
          {onLinkClick}
          {entryPath}
          {api}
          {onAttachmentInsert}
        />
      {/key}
    {:else}
      <LoadingSpinner />
    {/if}
  </div>
</div>
