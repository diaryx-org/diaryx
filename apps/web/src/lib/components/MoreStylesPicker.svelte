<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Ellipsis,
    Strikethrough,
    Code,
    EyeOff,
  } from "@lucide/svelte";

  interface Props {
    editor: Editor | null;
    enableSpoilers?: boolean;
    open?: boolean;
    onOpen?: () => void;
  }

  let { editor, enableSpoilers = true, open = $bindable(false), onOpen }: Props = $props();
  let wrapperElement: HTMLDivElement | null = $state(null);
  let showBelow = $state(false);

  let isStrikeActive = $state(false);
  let isCodeActive = $state(false);
  let isSpoilerActive = $state(false);

  // True if any of the overflow items is active (to show indicator on the button)
  let hasActiveItem = $derived(isStrikeActive || isCodeActive || isSpoilerActive);

  function updateActiveStates() {
    if (!editor) return;
    isStrikeActive = editor.isActive("strike");
    isCodeActive = editor.isActive("code");
    isSpoilerActive = editor.isActive("spoiler");
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

  function handleClick(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (!open) {
      onOpen?.();
      if (wrapperElement) {
        const rect = wrapperElement.getBoundingClientRect();
        showBelow = rect.top < 200;
      }
    }
    open = !open;
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

  $effect(() => {
    if (!editor) return;

    const ed = editor;
    const handleUpdate = () => updateActiveStates();

    ed.on("selectionUpdate", handleUpdate);
    ed.on("transaction", handleUpdate);
    updateActiveStates();

    return () => {
      ed.off("selectionUpdate", handleUpdate);
      ed.off("transaction", handleUpdate);
    };
  });
</script>

<div class="more-styles-wrapper" bind:this={wrapperElement}>
  <button
    type="button"
    class="toolbar-button"
    class:active={hasActiveItem}
    onmousedown={handleClick}
    title="More formatting"
    aria-haspopup="true"
    aria-expanded={open}
  >
    <Ellipsis class="size-4" />
  </button>

  {#if open}
    <div
      class="more-styles-dropdown"
      class:show-below={showBelow}
      role="menu"
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
    >
      <button
        type="button"
        class="more-styles-option"
        class:active={isStrikeActive}
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleStrike();
        }}
        title="Strikethrough"
        aria-checked={isStrikeActive}
        role="menuitemcheckbox"
      >
        <Strikethrough class="size-4" />
        <span>Strikethrough</span>
      </button>

      <button
        type="button"
        class="more-styles-option"
        class:active={isCodeActive}
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleCode();
        }}
        title="Inline Code"
        aria-checked={isCodeActive}
        role="menuitemcheckbox"
      >
        <Code class="size-4" />
        <span>Inline Code</span>
      </button>

      {#if enableSpoilers}
        <button
          type="button"
          class="more-styles-option"
          class:active={isSpoilerActive}
          onmousedown={(e) => {
            e.preventDefault();
            e.stopPropagation();
            handleSpoiler();
          }}
          title="Spoiler"
          aria-checked={isSpoilerActive}
          role="menuitemcheckbox"
        >
          <EyeOff class="size-4" />
          <span>Spoiler</span>
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .more-styles-wrapper {
    position: relative;
    display: inline-flex;
  }

  .more-styles-dropdown {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    padding: 4px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 100;
    min-width: max-content;
    animation: fadeInAbove 0.15s ease;
  }

  .more-styles-dropdown.show-below {
    bottom: auto;
    top: calc(100% + 8px);
    animation: fadeInBelow 0.15s ease;
  }

  @keyframes fadeInAbove {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  @keyframes fadeInBelow {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  .more-styles-option {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 4px;
    background: transparent;
    border: none;
    color: var(--foreground);
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
    transition: background 0.1s ease;
    -webkit-user-select: none;
    user-select: none;
  }

  .more-styles-option:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .more-styles-option.active {
    background: var(--accent);
    color: var(--accent-foreground);
    font-weight: 500;
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

    .more-styles-option {
      padding: 8px 12px;
    }
  }
</style>
