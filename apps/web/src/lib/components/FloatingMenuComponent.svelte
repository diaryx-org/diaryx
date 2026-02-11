<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Plus,
    Heading,
    List,
    Quote,
    Braces,
    Minus,
    Paperclip,
  } from "@lucide/svelte";
  import * as NativeSelect from "$lib/components/ui/native-select";

  interface Props {
    editor: Editor | null;
    onInsertAttachment?: () => void;
    element?: HTMLDivElement;
  }

  let { editor, onInsertAttachment, element = $bindable() }: Props = $props();

  let isExpanded = $state(false);
  let headingValue = $state("");
  let listValue = $state("");
  let focusedIndex = $state(-1);
  let focusableItems: HTMLElement[] = $state([]);

  function collapseMenu() {
    isExpanded = false;
  }

  function expandMenu(event?: MouseEvent | TouchEvent | KeyboardEvent) {
    // Prevent the event from bubbling to the window click handler
    event?.stopPropagation();
    event?.preventDefault();
    isExpanded = true;
    // Only auto-focus first item for keyboard activation.
    // On mouse/touch, focusing a <select> opens the native picker on mobile.
    const isKeyboard = event instanceof KeyboardEvent;
    if (isKeyboard) {
      setTimeout(() => {
        updateFocusableItems();
        if (focusableItems.length > 0) {
          focusedIndex = 0;
          focusableItems[0]?.focus();
        }
      }, 0);
    }
  }

  // Expose expand function for external triggering (e.g., keyboard shortcut from editor)
  export function expand() {
    if (!isExpanded) {
      expandMenu();
    }
  }

  function updateFocusableItems() {
    focusableItems = Array.from(
      element?.querySelectorAll('[data-slot="native-select"], .menu-item') || []
    ) as HTMLElement[];
  }

  function handleMenuKeydown(event: KeyboardEvent) {
    if (!isExpanded) return;

    switch (event.key) {
      case "Escape":
        event.preventDefault();
        collapseMenu();
        // Refocus editor so user can continue typing
        editor?.commands.focus();
        break;
      case "ArrowRight":
        event.preventDefault();
        updateFocusableItems();
        focusedIndex = (focusedIndex + 1) % focusableItems.length;
        focusableItems[focusedIndex]?.focus();
        break;
      case "ArrowLeft":
        event.preventDefault();
        updateFocusableItems();
        focusedIndex = (focusedIndex - 1 + focusableItems.length) % focusableItems.length;
        focusableItems[focusedIndex]?.focus();
        break;
    }
  }

  function handleTriggerKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" || event.key === " ") {
      expandMenu(event);
    }
  }

  function handleHeadingChange(event: Event) {
    const value = (event.target as HTMLSelectElement).value;
    if (value && editor) {
      const level = parseInt(value) as 1 | 2 | 3;
      editor.chain().focus().toggleHeading({ level }).run();
      collapseMenu();
    }
    // Reset the select value so it can be selected again
    headingValue = "";
  }

  function handleListChange(event: Event) {
    const value = (event.target as HTMLSelectElement).value;
    if (value && editor) {
      switch (value) {
        case "bullet":
          editor.chain().focus().toggleBulletList().run();
          break;
        case "ordered":
          editor.chain().focus().toggleOrderedList().run();
          break;
        case "task":
          editor.chain().focus().toggleTaskList().run();
          break;
      }
      collapseMenu();
    }
    // Reset the select value so it can be selected again
    listValue = "";
  }

  function handleBlockquote() {
    editor?.chain().focus().toggleBlockquote().run();
    collapseMenu();
  }

  function handleCodeBlock() {
    editor?.chain().focus().toggleCodeBlock().run();
    collapseMenu();
  }

  function handleHorizontalRule() {
    editor?.chain().focus().setHorizontalRule().run();
    collapseMenu();
  }

  function handleAttachment() {
    onInsertAttachment?.();
    collapseMenu();
  }

  // Handle menu item clicks - stop propagation to prevent closing
  function handleMenuItemClick(
    event: MouseEvent | TouchEvent,
    action: () => void,
  ) {
    event.stopPropagation();
    action();
  }

  // Close expanded menu when clicking outside
  function handleClickOutside(event: MouseEvent | TouchEvent) {
    if (!isExpanded) return;
    if (!element) return;

    const target = event.target as Node;
    if (!element.contains(target)) {
      isExpanded = false;
    }
  }
</script>

<svelte:window onclick={handleClickOutside} ontouchend={handleClickOutside} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={element}
  class="floating-menu"
  role="toolbar"
  aria-label="Block formatting"
  tabindex="-1"
  onmousedown={(e) => {
    // Prevent focus loss when clicking on the menu
    // This keeps the editor focused so the FloatingMenu extension doesn't hide it
    e.preventDefault();
  }}
  ontouchstart={(e) => {
    // Same for touch events
    e.preventDefault();
  }}
>
  {#if isExpanded}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="menu-expanded" onclick={(e) => e.stopPropagation()} onkeydown={handleMenuKeydown}>
      <div class="menu-section heading-section">
        <NativeSelect.Root
          bind:value={headingValue}
          onchange={handleHeadingChange}
          onmousedown={(e) => e.stopPropagation()}
          class="heading-select"
        >
          <NativeSelect.Option value="" disabled selected>
            <Heading class="size-4 inline mr-1" />H
          </NativeSelect.Option>
          <NativeSelect.Option value="1">H1</NativeSelect.Option>
          <NativeSelect.Option value="2">H2</NativeSelect.Option>
          <NativeSelect.Option value="3">H3</NativeSelect.Option>
        </NativeSelect.Root>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-section list-section">
        <NativeSelect.Root
          bind:value={listValue}
          onchange={handleListChange}
          onmousedown={(e) => e.stopPropagation()}
          class="list-select"
        >
          <NativeSelect.Option value="" disabled selected>
            <List class="size-4 inline" />
          </NativeSelect.Option>
          <NativeSelect.Option value="bullet">Bullet List</NativeSelect.Option>
          <NativeSelect.Option value="ordered">Numbered List</NativeSelect.Option>
          <NativeSelect.Option value="task">Task List</NativeSelect.Option>
        </NativeSelect.Root>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-section">
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleBlockquote)}
          class="menu-item"
          title="Quote"
        >
          <Quote class="size-4" />
          <span>Blockquote</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleCodeBlock)}
          class="menu-item"
          title="Code Block"
        >
          <Braces class="size-4" />
          <span>Code Block</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleHorizontalRule)}
          class="menu-item"
          title="Horizontal Rule"
        >
          <Minus class="size-4" />
          <span>Divider</span>
        </button>
      </div>

      {#if onInsertAttachment}
        <div class="menu-divider"></div>

        <div class="menu-section">
          <button
            type="button"
            onclick={(e) => handleMenuItemClick(e, handleAttachment)}
            class="menu-item"
            title="Insert Attachment"
          >
            <Paperclip class="size-4" />
            <span>Attachment</span>
          </button>
        </div>
      {/if}
    </div>
  {:else}
    <button
      type="button"
      class="trigger-button"
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onclick={expandMenu}
      onkeydown={handleTriggerKeydown}
      title="Add block"
      aria-expanded={isExpanded}
    >
      <Plus class="size-5" />
    </button>
  {/if}
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
    /* Prevent text selection on touch */
    -webkit-user-select: none;
    user-select: none;
    /* Improve touch responsiveness */
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

  .menu-expanded {
    display: flex;
    flex-direction: row;
    align-items: center;
    padding: 2px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    min-width: max-content;
    max-width: 90vw;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .menu-expanded::-webkit-scrollbar {
    display: none;
  }

  .menu-section {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 2px;
  }

  .heading-section :global([data-slot="native-select-wrapper"]) {
    width: auto;
  }

  .heading-section :global([data-slot="native-select"]) {
    height: 32px;
    min-width: 56px;
    padding: 0 24px 0 8px;
    font-size: 13px;
    font-weight: 500;
    border: none;
    background: transparent;
    box-shadow: none;
  }

  .heading-section :global([data-slot="native-select"]:hover),
  .heading-section :global([data-slot="native-select"]:focus) {
    background: var(--accent);
    outline: none;
  }

  .heading-section :global([data-slot="native-select"]:focus-visible) {
    outline: 2px solid var(--ring);
    outline-offset: -2px;
    border-radius: 6px;
  }

  .heading-section :global([data-slot="native-select-icon"]) {
    right: 6px;
  }

  .list-section :global([data-slot="native-select-wrapper"]) {
    width: auto;
  }

  .list-section :global([data-slot="native-select"]) {
    height: 32px;
    min-width: 56px;
    padding: 0 24px 0 8px;
    font-size: 13px;
    font-weight: 500;
    border: none;
    background: transparent;
    box-shadow: none;
  }

  .list-section :global([data-slot="native-select"]:hover),
  .list-section :global([data-slot="native-select"]:focus) {
    background: var(--accent);
    outline: none;
  }

  .list-section :global([data-slot="native-select"]:focus-visible) {
    outline: 2px solid var(--ring);
    outline-offset: -2px;
    border-radius: 6px;
  }

  .list-section :global([data-slot="native-select-icon"]) {
    right: 6px;
  }

  .menu-item {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border-radius: 6px;
    background: transparent;
    border: none;
    color: var(--foreground);
    cursor: pointer;
    transition: all 0.1s ease;
    /* Prevent text selection on touch */
    -webkit-user-select: none;
    user-select: none;
    /* Improve touch responsiveness */
    touch-action: manipulation;
  }

  .menu-item span {
    display: none;
  }

  .menu-item:hover,
  .menu-item:focus {
    background: var(--accent);
    color: var(--accent-foreground);
    outline: none;
  }

  .menu-item:focus-visible {
    outline: 2px solid var(--ring);
    outline-offset: -2px;
  }

  .menu-item:active {
    background: var(--accent);
    transform: scale(0.95);
  }

  .menu-divider {
    width: 1px;
    height: 20px;
    background: var(--border);
    margin: 0 4px;
    opacity: 0.5;
    flex-shrink: 0;
  }

  /* Mobile-specific adjustments */
  @media (max-width: 767px) {
    .trigger-button {
      width: 36px;
      height: 36px;
    }

    .menu-expanded {
      min-width: 0;
      flex-wrap: wrap;
      max-width: min(90vw, 280px);
      justify-content: flex-start;
    }

    .menu-item {
      width: 36px;
      height: 36px;
    }

    .heading-section :global([data-slot="native-select"]) {
      height: 36px;
      min-width: 56px;
      font-size: 13px;
    }

    .list-section :global([data-slot="native-select"]) {
      height: 36px;
      min-width: 56px;
      font-size: 13px;
    }

    .menu-divider {
      height: 16px;
    }
  }
</style>
