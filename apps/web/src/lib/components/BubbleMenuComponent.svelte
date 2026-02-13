<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Bold,
    Italic,
    Link as LinkIcon,
    Unlink,
  } from "@lucide/svelte";
  import HighlightColorPicker from "./HighlightColorPicker.svelte";
  import BlockStylePicker from "./BlockStylePicker.svelte";
  import MoreStylesPicker from "./MoreStylesPicker.svelte";
  import LinkInsertPopover from "./LinkInsertPopover.svelte";
  import type { HighlightColor } from "$lib/extensions/ColoredHighlightMark";
  import type { Api } from "$lib/backend/api";

  interface Props {
    editor: Editor | null;
    element?: HTMLDivElement;
    /** Whether spoiler functionality is enabled */
    enableSpoilers?: boolean;
    /** Current entry path for resolving local links */
    entryPath?: string;
    /** API instance for link formatting */
    api?: Api | null;
  }

  let { editor, element = $bindable(), enableSpoilers = true, entryPath = "", api = null }: Props = $props();

  // Track active states reactively
  let isBoldActive = $state(false);
  let isItalicActive = $state(false);
  let isHighlightActive = $state(false);
  let currentHighlightColor = $state<HighlightColor | null>(null);
  let isLinkActive = $state(false);
  let isInCodeBlock = $state(false);

  function updateActiveStates() {
    if (!editor) return;
    isBoldActive = editor.isActive("bold");
    isItalicActive = editor.isActive("italic");
    isHighlightActive = editor.isActive("coloredHighlight");
    // Get the current highlight color from the editor state
    if (isHighlightActive) {
      const attrs = editor.getAttributes("coloredHighlight");
      currentHighlightColor = (attrs.color as HighlightColor) || "yellow";
    } else {
      currentHighlightColor = null;
    }
    isLinkActive = editor.isActive("link");
    isInCodeBlock = editor.isActive("codeBlock");
  }

  function handleBold() {
    editor?.chain().focus().toggleBold().run();
    updateActiveStates();
  }

  function handleItalic() {
    editor?.chain().focus().toggleItalic().run();
    updateActiveStates();
  }

  // Dropdown mutual exclusion: only one open at a time
  let blockStyleOpen = $state(false);
  let highlightOpen = $state(false);
  let linkPopoverOpen = $state(false);
  let moreStylesOpen = $state(false);

  function closeAllDropdowns() {
    blockStyleOpen = false;
    highlightOpen = false;
    linkPopoverOpen = false;
    moreStylesOpen = false;
  }

  function handleLink() {
    if (isLinkActive) {
      editor?.chain().focus().unsetLink().run();
      updateActiveStates();
    } else {
      closeAllDropdowns();
      linkPopoverOpen = true;
    }
  }

  function handleLinkSelect(href: string) {
    editor?.chain().focus().setLink({ href }).run();
    linkPopoverOpen = false;
    updateActiveStates();
  }

  function handleLinkClose() {
    linkPopoverOpen = false;
    editor?.commands.focus();
  }

  // Update active states when editor changes
  $effect(() => {
    if (!editor) return;

    const ed = editor;
    // Defer to avoid state_unsafe_mutation when called from TipTap transaction
    // handlers that fire during Svelte template evaluation
    const handleUpdate = () => queueMicrotask(() => updateActiveStates());

    ed.on("selectionUpdate", handleUpdate);
    ed.on("transaction", handleUpdate);

    // Initial update
    updateActiveStates();

    return () => {
      ed.off("selectionUpdate", handleUpdate);
      ed.off("transaction", handleUpdate);
    };
  });
</script>

<div
  bind:this={element}
  class="bubble-menu"
  role="toolbar"
  aria-label="Text formatting"
  tabindex="-1"
  onmousedown={(e) => {
    // Prevent focus loss when clicking on the menu
    e.preventDefault();
  }}
  ontouchstart={(e) => {
    // Same for touch events on mobile
    e.preventDefault();
  }}
>
  <BlockStylePicker {editor} bind:open={blockStyleOpen} onOpen={() => { closeAllDropdowns(); }} />

  {#if !isInCodeBlock}
    <div class="toolbar-divider"></div>

    <button
      type="button"
      class="toolbar-button"
      class:active={isBoldActive}
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        handleBold();
      }}
      title="Bold"
      aria-pressed={isBoldActive}
    >
      <Bold class="size-4" />
    </button>

    <button
      type="button"
      class="toolbar-button"
      class:active={isItalicActive}
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        handleItalic();
      }}
      title="Italic"
      aria-pressed={isItalicActive}
    >
      <Italic class="size-4" />
    </button>

    <HighlightColorPicker {editor} isActive={isHighlightActive} currentColor={currentHighlightColor} bind:open={highlightOpen} onOpen={() => { closeAllDropdowns(); }} />

    <div class="link-button-wrapper">
      <button
        type="button"
        class="toolbar-button"
        class:active={isLinkActive}
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleLink();
        }}
        title={isLinkActive ? "Remove Link" : "Add Link"}
        aria-pressed={isLinkActive}
      >
        {#if isLinkActive}
          <Unlink class="size-4" />
        {:else}
          <LinkIcon class="size-4" />
        {/if}
      </button>

      <LinkInsertPopover
        bind:open={linkPopoverOpen}
        onSelect={handleLinkSelect}
        onClose={handleLinkClose}
        currentEntryPath={entryPath}
        {api}
      />
    </div>

    <div class="toolbar-divider"></div>

    <MoreStylesPicker {editor} {enableSpoilers} bind:open={moreStylesOpen} onOpen={() => { closeAllDropdowns(); }} />
  {/if}
</div>

<style>
  .bubble-menu {
    /* Start hidden - controlled via onShow/onHide in Editor.svelte to prevent flash on initial load */
    display: none;
    align-items: center;
    gap: 2px;
    padding: 4px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 50;
    position: fixed;
    /* Prevent text selection on toolbar */
    -webkit-user-select: none;
    user-select: none;
  }

  .toolbar-button {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 6px;
    border-radius: 4px;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .toolbar-button:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .toolbar-button:active {
    transform: scale(0.95);
    background: var(--accent);
  }

  .toolbar-button.active {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .link-button-wrapper {
    position: relative;
    display: inline-flex;
  }

  .toolbar-divider {
    width: 1px;
    height: 16px;
    background: var(--border);
    margin: 0 2px;
    flex-shrink: 0;
  }

  /* Mobile-specific adjustments for touch targets */
  @media (max-width: 767px) {
    .bubble-menu {
      gap: 2px;
      padding: 4px;
      flex-wrap: wrap;
      max-width: min(90vw, 300px);
      justify-content: center;
    }

    .toolbar-button {
      padding: 8px;
      min-width: 36px;
      min-height: 36px;
    }

    .toolbar-divider {
      height: 18px;
      margin: 0 2px;
    }
  }
</style>
