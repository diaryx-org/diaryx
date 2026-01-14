<script lang="ts">
  /**
   * EditorContent - The main editor content area
   * 
   * Wraps the TipTap editor with loading states.
   * This component handles the editor rendering logic.
   */
  
  import LoadingSpinner from "../shared/LoadingSpinner.svelte";
  import type { Doc as YDoc } from "yjs";
  import type { HocuspocusProvider } from "@hocuspocus/provider";

  interface Props {
    Editor: typeof import("$lib/Editor.svelte").default | null;
    editorRef: any;
    content: string;
    editorKey: string;
    collaborationEnabled: boolean;
    currentYDoc: YDoc | null;
    currentProvider: HocuspocusProvider | null;
    onchange: (markdown: string) => void;
    onblur: () => void;
    // These match the Editor component prop types
    onInsertImage?: () => void;
    onFileDrop?: (file: File) => Promise<{ blobUrl: string; attachmentPath: string } | null>;
    onLinkClick?: (href: string) => void;
  }

  let {
    Editor,
    editorRef = $bindable(),
    content,
    editorKey,
    collaborationEnabled,
    currentYDoc,
    currentProvider,
    onchange,
    onblur,
    onInsertImage,
    onFileDrop,
    onLinkClick,
  }: Props = $props();
</script>

<!-- Outer container: edge-to-edge on desktop, scrollbar touches edges -->
<div class="flex-1 overflow-y-auto">
  <!-- Inner wrapper: padding for readable text content -->
  <div class="p-4 md:px-6 md:py-4 min-h-full">
    {#if Editor}
      {#key editorKey}
        <Editor
          debugMenus={false}
          bind:this={editorRef}
          {content}
          {onchange}
          {onblur}
          placeholder="Start writing..."
          {onInsertImage}
          {onFileDrop}
          {onLinkClick}
          ydoc={collaborationEnabled ? (currentYDoc ?? undefined) : undefined}
          provider={collaborationEnabled ? (currentProvider ?? undefined) : undefined}
        />
      {/key}
    {:else}
      <LoadingSpinner />
    {/if}
  </div>
</div>
