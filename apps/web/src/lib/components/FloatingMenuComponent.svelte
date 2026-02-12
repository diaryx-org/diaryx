<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import { Plus } from "@lucide/svelte";

  interface Props {
    editor: Editor | null;
    element?: HTMLDivElement;
  }

  let { editor, element = $bindable() }: Props = $props();

  function handleClick(event: MouseEvent | TouchEvent) {
    event.stopPropagation();
    event.preventDefault();
    editor?.commands.insertBlockPicker();
  }

  // Expose expand function for external triggering (e.g., Right Arrow keyboard shortcut)
  export function expand() {
    editor?.commands.insertBlockPicker();
  }

  function handleTriggerKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" || event.key === " ") {
      event.stopPropagation();
      event.preventDefault();
      editor?.commands.insertBlockPicker();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={element}
  class="floating-menu"
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
    onmousedown={(e) => {
      e.preventDefault();
      e.stopPropagation();
    }}
    onclick={handleClick}
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
