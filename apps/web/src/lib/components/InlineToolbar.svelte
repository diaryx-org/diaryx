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
    Undo,
    Redo,
    Check,
  } from "@lucide/svelte";

  interface Props {
    editor: Editor | null;
    /** Whether to show the "Done" button (for dismissing keyboard on mobile) */
    showDone?: boolean;
    /** Callback when Done button is pressed */
    onDone?: () => void;
    /** Position mode: 'top' for desktop, 'bottom' for mobile above keyboard */
    position?: "top" | "bottom";
    /** Visual viewport offset from top (iOS scroll offset when keyboard is open) */
    viewportOffsetTop?: number;
    /** Visual viewport height (shrinks when keyboard is visible) */
    viewportHeight?: number;
  }

  let {
    editor,
    showDone = false,
    onDone,
    position = "top",
    viewportOffsetTop = 0,
    viewportHeight = 0,
  }: Props = $props();

  // Track active states reactively
  let isBoldActive = $state(false);
  let isItalicActive = $state(false);
  let isStrikeActive = $state(false);
  let isCodeActive = $state(false);
  let isHighlightActive = $state(false);
  let isLinkActive = $state(false);
  let canUndo = $state(false);
  let canRedo = $state(false);

  function updateActiveStates() {
    if (!editor) return;
    isBoldActive = editor.isActive("bold");
    isItalicActive = editor.isActive("italic");
    isStrikeActive = editor.isActive("strike");
    isCodeActive = editor.isActive("code");
    isHighlightActive = editor.isActive("highlight");
    isLinkActive = editor.isActive("link");
    // undo/redo may not be available in collaboration mode (Y.js handles history)
    try {
      const can = editor.can();
      canUndo = typeof can.undo === "function" ? can.undo() : false;
      canRedo = typeof can.redo === "function" ? can.redo() : false;
    } catch {
      canUndo = false;
      canRedo = false;
    }
  }

  function handleAction(event: Event, action: () => void) {
    event.preventDefault();
    event.stopPropagation();
    action();
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

  function handleUndo() {
    try {
      editor?.chain().focus().undo().run();
    } catch {
      // undo not available (e.g., in collaboration mode)
    }
    updateActiveStates();
  }

  function handleRedo() {
    try {
      editor?.chain().focus().redo().run();
    } catch {
      // redo not available (e.g., in collaboration mode)
    }
    updateActiveStates();
  }

  function handleDone() {
    // Blur the editor to dismiss keyboard
    editor?.commands.blur();
    onDone?.();
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
  class="inline-toolbar"
  class:position-top={position === "top"}
  class:position-bottom={position === "bottom"}
  style={position === "bottom" ? `top: ${viewportOffsetTop + viewportHeight}px; transform: translateY(-100%);` : ""}
  role="toolbar"
  aria-label="Text formatting"
  tabindex="-1"
  onpointerdown={(e) => {
    // Prevent focus loss when clicking/tapping on the toolbar
    // This keeps the editor focused so formatting commands work
    // Using pointerdown handles both mouse and touch uniformly
    e.preventDefault();
  }}
>
  <div class="toolbar-scroll">
    <div class="toolbar-group">
      <button
        type="button"
        class="toolbar-button"
        class:active={isBoldActive}
        onclick={(e) => handleAction(e, handleBold)}
        title="Bold"
        aria-pressed={isBoldActive}
      >
        <Bold class="size-4" />
      </button>

      <button
        type="button"
        class="toolbar-button"
        class:active={isItalicActive}
        onclick={(e) => handleAction(e, handleItalic)}
        title="Italic"
        aria-pressed={isItalicActive}
      >
        <Italic class="size-4" />
      </button>

      <button
        type="button"
        class="toolbar-button"
        class:active={isStrikeActive}
        onclick={(e) => handleAction(e, handleStrike)}
        title="Strikethrough"
        aria-pressed={isStrikeActive}
      >
        <Strikethrough class="size-4" />
      </button>
    </div>

    <div class="toolbar-divider"></div>

    <div class="toolbar-group">
      <button
        type="button"
        class="toolbar-button"
        class:active={isCodeActive}
        onclick={(e) => handleAction(e, handleCode)}
        title="Inline Code"
        aria-pressed={isCodeActive}
      >
        <Code class="size-4" />
      </button>

      <button
        type="button"
        class="toolbar-button"
        class:active={isHighlightActive}
        onclick={(e) => handleAction(e, handleHighlight)}
        title="Highlight"
        aria-pressed={isHighlightActive}
      >
        <Highlighter class="size-4" />
      </button>

      <button
        type="button"
        class="toolbar-button"
        class:active={isLinkActive}
        onclick={(e) => handleAction(e, handleLink)}
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

    <div class="toolbar-divider"></div>

    <div class="toolbar-group">
      <button
        type="button"
        class="toolbar-button"
        onclick={(e) => handleAction(e, handleUndo)}
        disabled={!canUndo}
        title="Undo"
      >
        <Undo class="size-4" />
      </button>

      <button
        type="button"
        class="toolbar-button"
        onclick={(e) => handleAction(e, handleRedo)}
        disabled={!canRedo}
        title="Redo"
      >
        <Redo class="size-4" />
      </button>
    </div>

    {#if showDone}
      <div class="toolbar-divider"></div>

      <div class="toolbar-group">
        <button
          type="button"
          class="toolbar-button done-button"
          onclick={(e) => handleAction(e, handleDone)}
          title="Done"
        >
          <Check class="size-4" />
          <span class="done-label">Done</span>
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .inline-toolbar {
    display: flex;
    align-items: center;
    background: var(--card);
    border-color: var(--border);
    z-index: 20;
    /* Prevent text selection on toolbar */
    -webkit-user-select: none;
    user-select: none;
  }

  .position-top {
    position: relative;
    border-bottom-width: 1px;
    border-bottom-style: solid;
    padding: 6px 8px;
  }

  .position-bottom {
    position: fixed;
    left: 0;
    right: 0;
    /* Using top positioning with transform: translateY(-100%) for iOS visual viewport compatibility */
    /* The top value and transform are set via inline style */
    border-top-width: 1px;
    border-top-style: solid;
    padding: 8px;
    background: var(--card);
    /* Subtle shadow for elevation */
    box-shadow: 0 -2px 10px rgba(0, 0, 0, 0.1);
  }

  .toolbar-scroll {
    display: flex;
    align-items: center;
    gap: 4px;
    overflow-x: auto;
    overflow-y: hidden;
    /* Hide scrollbar but allow scrolling */
    scrollbar-width: none;
    -ms-overflow-style: none;
    /* Smooth scrolling */
    scroll-behavior: smooth;
    -webkit-overflow-scrolling: touch;
  }

  .toolbar-scroll::-webkit-scrollbar {
    display: none;
  }

  .toolbar-group {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }

  .toolbar-divider {
    width: 1px;
    height: 20px;
    background: var(--border);
    margin: 0 4px;
    flex-shrink: 0;
  }

  .toolbar-button {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 8px;
    border-radius: 6px;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: all 0.15s ease;
    min-width: 36px;
    min-height: 36px;
    touch-action: manipulation;
    -webkit-touch-callout: none;
  }

  .toolbar-button:hover:not(:disabled) {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .toolbar-button:active:not(:disabled) {
    transform: scale(0.95);
    background: var(--accent);
  }

  .toolbar-button.active {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .toolbar-button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .done-button {
    gap: 4px;
    padding: 8px 12px;
    background: var(--primary);
    color: var(--primary-foreground);
    font-weight: 500;
  }

  .done-button:hover {
    opacity: 0.9;
    background: var(--primary);
    color: var(--primary-foreground);
  }

  .done-button:active {
    background: var(--primary);
    color: var(--primary-foreground);
  }

  .done-label {
    font-size: 14px;
  }

  /* Mobile adjustments */
  @media (max-width: 767px) {
    .position-bottom {
      padding: 6px 8px;
    }

    .toolbar-button {
      min-width: 40px;
      min-height: 40px;
    }
  }
</style>
