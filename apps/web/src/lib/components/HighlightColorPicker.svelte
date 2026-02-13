<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import { Highlighter, X } from "@lucide/svelte";
  import { HIGHLIGHT_COLORS, type HighlightColor } from "$lib/extensions/ColoredHighlightMark";

  interface Props {
    editor: Editor | null;
    isActive: boolean;
    currentColor?: HighlightColor | null;
    open?: boolean;
    onOpen?: () => void;
  }

  let { editor, isActive, currentColor = null, open = $bindable(false), onOpen }: Props = $props();
  let wrapperElement: HTMLDivElement | null = $state(null);

  // Color display configuration
  const colorConfig: Record<HighlightColor, { label: string }> = {
    red: { label: "Red" },
    orange: { label: "Orange" },
    yellow: { label: "Yellow" },
    green: { label: "Green" },
    cyan: { label: "Cyan" },
    blue: { label: "Blue" },
    violet: { label: "Violet" },
    pink: { label: "Pink" },
    brown: { label: "Brown" },
    grey: { label: "Grey" },
  };

  function handleColorSelect(color: HighlightColor) {
    if (!editor) return;
    editor.chain().focus().toggleColoredHighlight(color).run();
    open = false;
  }

  function handleRemoveHighlight() {
    if (!editor) return;
    editor.chain().focus().unsetColoredHighlight().run();
    open = false;
  }

  function handleClick(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();

    // If already highlighted, show the color picker (for mobile double-tap behavior)
    if (isActive) {
      onOpen?.();
      open = true;
      return;
    }

    // Otherwise toggle yellow highlight
    editor?.chain().focus().toggleColoredHighlight("yellow").run();
  }

  function handleContextMenu(e: MouseEvent) {
    // Right-click: show color picker (desktop)
    e.preventDefault();
    e.stopPropagation();
    onOpen?.();
    open = true;
  }

  function handleClickOutside(e: MouseEvent) {
    if (wrapperElement && !wrapperElement.contains(e.target as Node)) {
      open = false;
    }
  }

  // Close on escape key
  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) {
      open = false;
      e.preventDefault();
    }
  }

  // Set up click outside listener when open
  $effect(() => {
    if (open) {
      // Use setTimeout to avoid the current click from closing immediately
      const timeoutId = setTimeout(() => {
        document.addEventListener("mousedown", handleClickOutside);
        document.addEventListener("keydown", handleKeydown);
      }, 0);

      return () => {
        clearTimeout(timeoutId);
        document.removeEventListener("mousedown", handleClickOutside);
        document.removeEventListener("keydown", handleKeydown);
      };
    }
  });
</script>

<div class="highlight-picker-wrapper" bind:this={wrapperElement}>
  <button
    type="button"
    class="toolbar-button {isActive ? 'active' : ''}"
    onmousedown={handleClick}
    oncontextmenu={handleContextMenu}
    title="Highlight (right-click for colors)"
    aria-pressed={isActive}
    aria-haspopup="true"
    aria-expanded={open}
  >
    <span class="highlight-button-content">
      <Highlighter class="size-4" />
      {#if isActive && currentColor}
        <span
          class="color-indicator highlight-{currentColor}"
          aria-label="Current color: {colorConfig[currentColor].label}"
        ></span>
      {/if}
    </span>
  </button>

  {#if open}
    <div
      class="color-picker-dropdown"
      role="menu"
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
    >
      <div class="color-grid">
        {#each HIGHLIGHT_COLORS as color}
          <button
            type="button"
            class="color-swatch highlight-{color}"
            class:selected={currentColor === color}
            onclick={() => handleColorSelect(color)}
            title={colorConfig[color].label}
            aria-label={`Highlight ${colorConfig[color].label}`}
            role="menuitem"
          >
            {#if currentColor === color}
              <span class="checkmark">&#10003;</span>
            {/if}
          </button>
        {/each}
      </div>
      {#if isActive}
        <button
          type="button"
          class="remove-highlight"
          onclick={handleRemoveHighlight}
          title="Remove highlight"
          role="menuitem"
        >
          <X class="size-3" />
          <span>Remove highlight</span>
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .highlight-picker-wrapper {
    position: relative;
    display: inline-flex;
  }

  .color-picker-dropdown {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 100;
    animation: fadeIn 0.15s ease;
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  .color-grid {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 4px;
  }

  .color-swatch {
    width: 24px;
    height: 24px;
    border-radius: 4px;
    border: 2px solid transparent;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s ease;
  }

  .color-swatch:hover {
    transform: scale(1.1);
    border-color: var(--foreground);
  }

  .color-swatch.selected {
    border-color: var(--foreground);
  }

  .checkmark {
    font-size: 12px;
    font-weight: bold;
    color: var(--foreground);
  }

  .remove-highlight {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border: none;
    background: var(--muted);
    color: var(--muted-foreground);
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
    transition: all 0.15s ease;
  }

  .remove-highlight:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  /* Button content wrapper for positioning indicator */
  .highlight-button-content {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* Small color indicator dot */
  .color-indicator {
    position: absolute;
    bottom: -2px;
    right: -2px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 1px solid var(--background);
  }

  /* Toolbar button styles (matching BubbleMenuComponent) */
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

  /* Mobile adjustments */
  @media (max-width: 767px) {
    .toolbar-button {
      padding: 8px;
      min-width: 36px;
      min-height: 36px;
    }

    .color-swatch {
      width: 28px;
      height: 28px;
    }

    .color-indicator {
      width: 10px;
      height: 10px;
    }
  }
</style>
