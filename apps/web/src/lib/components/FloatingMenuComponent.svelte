<script lang="ts">
  import { onMount } from "svelte";
  import type { Editor } from "@tiptap/core";
  import { Plus } from "@lucide/svelte";

  interface Props {
    editor: Editor | null;
    element?: HTMLDivElement;
  }

  let { editor, element = $bindable() }: Props = $props();
  const triggerId = `floating-menu-${Math.random().toString(36).slice(2)}`;
  let expandTimeout: number | undefined;

  function expandPicker() {
    if (!editor || editor.isDestroyed) return;

    editor.commands.focus();

    if (expandTimeout) {
      clearTimeout(expandTimeout);
    }

    // Let the focus transaction settle before inserting the inline picker.
    // Without this, the first click can restore editor focus while the second
    // click is the one that actually inserts the picker.
    expandTimeout = window.setTimeout(() => {
      expandTimeout = undefined;
      if (!editor || editor.isDestroyed) return;
      editor.commands.insertBlockPicker();
    }, 0);
  }

  function handleExpand(event: MouseEvent | TouchEvent | KeyboardEvent) {
    event.stopPropagation();
    event.preventDefault();
    expandPicker();
  }

  function handleDocumentClick(event: MouseEvent) {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const trigger = target.closest(`.trigger-button[data-floating-menu-id="${triggerId}"]`);
    if (!trigger) return;
    handleExpand(event);
  }

  // Expose expand function for external triggering (e.g., Right Arrow keyboard shortcut)
  export function expand() {
    expandPicker();
  }

  function handleTriggerKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" || event.key === " ") {
      handleExpand(event);
    }
  }

  onMount(() => {
    const controller = new AbortController();
    document.addEventListener("click", handleDocumentClick, {
      signal: controller.signal,
    });

    return () => {
      if (expandTimeout) {
        clearTimeout(expandTimeout);
      }
      controller.abort();
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={element}
  class="floating-menu"
  data-floating-menu-id={triggerId}
  role="toolbar"
  aria-label="Block formatting"
  tabindex="-1"
  onpointerdown={(e) => {
    // Prevent focus loss when clicking on the menu.
    // pointerdown fires before mousedown/focus, so preventDefault here
    // keeps the editor focused and prevents TipTap's FloatingMenu from hiding.
    e.preventDefault();
  }}
>
  <button
    type="button"
    class="trigger-button"
    data-floating-menu-id={triggerId}
    onkeydown={handleTriggerKeydown}
    title="Add block"
  >
    <Plus class="size-5" />
  </button>
</div>

<style>
  .floating-menu {
    z-index: 20;
    /* Start hidden - TipTap's FloatingMenu extension will show it via inline
       styles when shouldShow returns true. This prevents the flash of the
       menu being visible before TipTap takes control. */
    display: none;
  }

  .trigger-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 6px;
    background: var(--card);
    border: 1px solid var(--border);
    color: var(--muted-foreground);
    cursor: pointer;
    transition: all 0.15s ease;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
    -webkit-user-select: none;
    user-select: none;
    touch-action: manipulation;
  }

  .trigger-button:hover {
    background: var(--accent);
    color: var(--accent-foreground);
    border-color: var(--accent);
  }

  .trigger-button:active {
    transform: scale(0.9);
  }

  @media (max-width: 767px) {
    .trigger-button {
      width: 36px;
      height: 36px;
    }
  }
</style>
