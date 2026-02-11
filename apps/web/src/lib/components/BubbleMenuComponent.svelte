<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Link as LinkIcon,
    Unlink,
    EyeOff,
  } from "@lucide/svelte";
  import HighlightColorPicker from "./HighlightColorPicker.svelte";
  import type { HighlightColor } from "$lib/extensions/ColoredHighlightMark";

  interface Props {
    editor: Editor | null;
    element?: HTMLDivElement;
    /** Whether spoiler functionality is enabled */
    enableSpoilers?: boolean;
  }

  let { editor, element = $bindable(), enableSpoilers = true }: Props = $props();

  // Track active states reactively
  let isBoldActive = $state(false);
  let isItalicActive = $state(false);
  let isStrikeActive = $state(false);
  let isCodeActive = $state(false);
  let isHighlightActive = $state(false);
  let currentHighlightColor = $state<HighlightColor | null>(null);
  let isLinkActive = $state(false);
  let isSpoilerActive = $state(false);

  function updateActiveStates() {
    if (!editor) return;
    isBoldActive = editor.isActive("bold");
    isItalicActive = editor.isActive("italic");
    isStrikeActive = editor.isActive("strike");
    isCodeActive = editor.isActive("code");
    isHighlightActive = editor.isActive("coloredHighlight");
    // Get the current highlight color from the editor state
    if (isHighlightActive) {
      const attrs = editor.getAttributes("coloredHighlight");
      currentHighlightColor = (attrs.color as HighlightColor) || "yellow";
    } else {
      currentHighlightColor = null;
    }
    isLinkActive = editor.isActive("link");
    isSpoilerActive = editor.isActive("spoiler");
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

  function handleSpoiler() {
    editor?.chain().focus().toggleSpoiler().run();
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
  ontouchstart={(e) => {
    // Same for touch events on mobile
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

  <HighlightColorPicker {editor} isActive={isHighlightActive} currentColor={currentHighlightColor} />

  {#if enableSpoilers}
    <button
      type="button"
      class="toolbar-button"
      class:active={isSpoilerActive}
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        handleSpoiler();
      }}
      title="Spoiler"
      aria-pressed={isSpoilerActive}
    >
      <EyeOff class="size-4" />
    </button>
  {/if}

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
