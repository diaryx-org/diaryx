<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import { X } from "@lucide/svelte";
  import type { MarkToolbarEntry } from "@/models/stores/pluginStore.svelte";

  interface Props {
    editor: Editor | null;
    entry: MarkToolbarEntry;
    isActive: boolean;
    currentValue?: string | null;
    open?: boolean;
    onOpen?: () => void;
  }

  let { editor, entry, isActive, currentValue = null, open = $bindable(false), onOpen }: Props = $props();
  let wrapperElement: HTMLDivElement | null = $state(null);

  /** Title-case a value string for display (e.g., "red" → "Red"). */
  function titleCase(s: string): string {
    return s.charAt(0).toUpperCase() + s.slice(1);
  }

  function handleValueSelect(value: string) {
    if (!editor || !entry.attribute) return;
    editor.chain().focus().toggleMark(entry.extensionId, { [entry.attribute.name]: value }).run();
    open = false;
  }

  function handleRemove() {
    if (!editor) return;
    editor.chain().focus().unsetMark(entry.extensionId).run();
    open = false;
  }

  function handleClick(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();

    if (!entry.attribute) {
      // Simple mark toggle (no attributes)
      editor?.chain().focus().toggleMark(entry.extensionId).run();
      return;
    }

    // If already active, show the picker (for mobile double-tap behavior)
    if (isActive) {
      onOpen?.();
      open = true;
      return;
    }

    // Otherwise toggle with default value
    editor?.chain().focus().toggleMark(entry.extensionId, { [entry.attribute.name]: entry.attribute.default }).run();
  }

  function handleContextMenu(e: MouseEvent) {
    if (!entry.attribute?.validValues.length) return;
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

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) {
      open = false;
      e.preventDefault();
    }
  }

  $effect(() => {
    if (open) {
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

<div class="attr-picker-wrapper" bind:this={wrapperElement}>
  <button
    type="button"
    class="toolbar-button {isActive ? 'active' : ''}"
    onmousedown={handleClick}
    oncontextmenu={handleContextMenu}
    title="{entry.label}{entry.attribute ? ' (right-click for options)' : ''}"
    aria-pressed={isActive}
    aria-haspopup={!!entry.attribute?.validValues.length}
    aria-expanded={open}
  >
    <span class="attr-button-content">
      <entry.icon class="size-4" />
      {#if isActive && currentValue && entry.attribute?.cssClassPrefix}
        <span
          class="color-indicator {entry.attribute.cssClassPrefix}{currentValue}"
          aria-label="Current: {titleCase(currentValue)}"
        ></span>
      {/if}
    </span>
  </button>

  {#if open && entry.attribute?.validValues.length}
    <div
      class="attr-picker-dropdown"
      role="menu"
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
    >
      <div class="attr-grid">
        {#each entry.attribute.validValues as value}
          <button
            type="button"
            class="attr-swatch {entry.attribute.cssClassPrefix ? `${entry.attribute.cssClassPrefix}${value}` : ''}"
            class:selected={currentValue === value}
            onclick={() => handleValueSelect(value)}
            title={titleCase(value)}
            aria-label={`${entry.label} ${titleCase(value)}`}
            role="menuitem"
          >
            {#if currentValue === value}
              <span class="checkmark">&#10003;</span>
            {:else if !entry.attribute.cssClassPrefix}
              {titleCase(value)}
            {/if}
          </button>
        {/each}
      </div>
      {#if isActive}
        <button
          type="button"
          class="remove-mark"
          onclick={handleRemove}
          title="Remove {entry.label.toLowerCase()}"
          role="menuitem"
        >
          <X class="size-3" />
          <span>Remove {entry.label.toLowerCase()}</span>
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .attr-picker-wrapper {
    position: relative;
    display: inline-flex;
  }

  .attr-picker-dropdown {
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

  .attr-grid {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 4px;
  }

  .attr-swatch {
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

  .attr-swatch:hover {
    transform: scale(1.1);
    border-color: var(--foreground);
  }

  .attr-swatch.selected {
    border-color: var(--foreground);
  }

  .checkmark {
    font-size: 12px;
    font-weight: bold;
    color: var(--foreground);
  }

  .remove-mark {
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

  .remove-mark:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .attr-button-content {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .color-indicator {
    position: absolute;
    bottom: -2px;
    right: -2px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 1px solid var(--background);
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

  @media (max-width: 767px) {
    .attr-picker-wrapper {
      position: static;
    }

    .toolbar-button {
      padding: 8px;
      min-width: 36px;
      min-height: 36px;
    }

    .attr-swatch {
      width: 28px;
      height: 28px;
    }

    .color-indicator {
      width: 10px;
      height: 10px;
    }
  }
</style>
