<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Plus,
    Heading,
    List,
    ListOrdered,
    CheckSquare,
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

  function collapseMenu() {
    isExpanded = false;
  }

  function expandMenu(event: MouseEvent | TouchEvent) {
    // Prevent the event from bubbling to the window click handler
    event.stopPropagation();
    event.preventDefault();
    isExpanded = true;
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

  function handleBulletList() {
    editor?.chain().focus().toggleBulletList().run();
    collapseMenu();
  }

  function handleOrderedList() {
    editor?.chain().focus().toggleOrderedList().run();
    collapseMenu();
  }

  function handleTaskList() {
    editor?.chain().focus().toggleTaskList().run();
    collapseMenu();
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
    <div class="menu-expanded" onclick={(e) => e.stopPropagation()}>
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

      <div class="menu-section">
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleBulletList)}
          class="menu-item"
          title="Bullet List"
        >
          <List class="size-4" />
          <span>Bullet List</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleOrderedList)}
          class="menu-item"
          title="Numbered List"
        >
          <ListOrdered class="size-4" />
          <span>Numbered List</span>
        </button>
        <button
          type="button"
          onclick={(e) => handleMenuItemClick(e, handleTaskList)}
          class="menu-item"
          title="Task List"
        >
          <CheckSquare class="size-4" />
          <span>Task List</span>
        </button>
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

  .heading-section :global([data-slot="native-select"]:hover) {
    background: var(--accent);
  }

  .heading-section :global([data-slot="native-select-icon"]) {
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

  .menu-item:hover {
    background: var(--accent);
    color: var(--accent-foreground);
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

    .menu-item {
      width: 40px;
      height: 40px;
    }

    .heading-section :global([data-slot="native-select"]) {
      height: 40px;
      min-width: 64px;
      font-size: 14px;
    }
  }
</style>
