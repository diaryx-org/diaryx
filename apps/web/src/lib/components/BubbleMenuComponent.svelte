<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Highlighter,
    Link as LinkIcon,
    Unlink,
  } from "@lucide/svelte";

  interface Props {
    editor: Editor | null;
    element?: HTMLDivElement;
  }

  let { editor, element = $bindable() }: Props = $props();

  // Track active states reactively
  let isBoldActive = $state(false);
  let isItalicActive = $state(false);
  let isStrikeActive = $state(false);
  let isCodeActive = $state(false);
  let isHighlightActive = $state(false);
  let isLinkActive = $state(false);

  function updateActiveStates() {
    if (!editor) return;
    isBoldActive = editor.isActive("bold");
    isItalicActive = editor.isActive("italic");
    isStrikeActive = editor.isActive("strike");
    isCodeActive = editor.isActive("code");
    isHighlightActive = editor.isActive("highlight");
    isLinkActive = editor.isActive("link");
  }

  function handleBold() {
    editor?.chain().focus().toggleBold().run();
    updateActiveStates();
  }

  function handleItalic() {
    editor?.chain().focus().toggleItalic().run();
    updateActiveStates();
  }

  function handleStrike() {
    editor?.chain().focus().toggleStrike().run();
    updateActiveStates();
  }

  function handleCode() {
    editor?.chain().focus().toggleCode().run();
    updateActiveStates();
  }

  function handleHighlight() {
    editor?.chain().focus().toggleHighlight().run();
    updateActiveStates();
  }

  function handleLink() {
    if (isLinkActive) {
      editor?.chain().focus().unsetLink().run();
    } else {
      const url = prompt("Enter URL:");
      if (url) {
        editor?.chain().focus().setLink({ href: url }).run();
      }
    }
    updateActiveStates();
  }

  // Update active states when editor changes
  $effect(() => {
    if (!editor) return;

    const ed = editor;
    const handleUpdate = () => updateActiveStates();

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
>
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

  <button
    type="button"
    class="toolbar-button"
    class:active={isStrikeActive}
    onmousedown={(e) => {
      e.preventDefault();
      e.stopPropagation();
      handleStrike();
    }}
    title="Strikethrough"
    aria-pressed={isStrikeActive}
  >
    <Strikethrough class="size-4" />
  </button>

  <div class="toolbar-divider"></div>

  <button
    type="button"
    class="toolbar-button"
    class:active={isCodeActive}
    onmousedown={(e) => {
      e.preventDefault();
      e.stopPropagation();
      handleCode();
    }}
    title="Inline Code"
    aria-pressed={isCodeActive}
  >
    <Code class="size-4" />
  </button>

  <button
    type="button"
    class="toolbar-button"
    class:active={isHighlightActive}
    onmousedown={(e) => {
      e.preventDefault();
      e.stopPropagation();
      handleHighlight();
    }}
    title="Highlight"
    aria-pressed={isHighlightActive}
  >
    <Highlighter class="size-4" />
  </button>

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
</div>

<style>
  .bubble-menu {
    display: flex;
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
    /* Start hidden - TipTap's BubbleMenu extension will show it */
    visibility: hidden;
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

  .toolbar-divider {
    width: 1px;
    height: 16px;
    background: var(--border);
    margin: 0 2px;
    flex-shrink: 0;
  }
</style>
