<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Plus,
    Heading,
    List,
    ListOrdered,
    ListTodo,
    ChevronDown,
    Quote,
    Braces,
    Minus,
    Paperclip,
  } from "@lucide/svelte";

  interface Props {
    editor: Editor | null;
    onInsertAttachment?: () => void;
    element?: HTMLDivElement;
  }

  let { editor, onInsertAttachment, element = $bindable() }: Props = $props();

  let isExpanded = $state(false);
  let focusedIndex = $state(-1);
  let focusableItems: HTMLElement[] = $state([]);

  // Sub-menu state: which dropdown is open ("heading" | "list" | null)
  let openSubmenu = $state<"heading" | "list" | null>(null);

  function collapseMenu() {
    openSubmenu = null;
    isExpanded = false;
  }

  function expandMenu(event?: MouseEvent | TouchEvent | KeyboardEvent) {
    // Prevent the event from bubbling to the window click handler
    event?.stopPropagation();
    event?.preventDefault();
    isExpanded = true;
    // Only auto-focus first item for keyboard activation.
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
      element?.querySelectorAll('.menu-item, .submenu-item') || []
    ) as HTMLElement[];
  }

  function handleMenuKeydown(event: KeyboardEvent) {
    if (!isExpanded) return;

    switch (event.key) {
      case "Escape":
        event.preventDefault();
        if (openSubmenu) {
          openSubmenu = null;
        } else {
          collapseMenu();
          // Refocus editor so user can continue typing
          editor?.commands.focus();
        }
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

  function toggleSubmenu(menu: "heading" | "list", event: MouseEvent | TouchEvent) {
    event.stopPropagation();
    openSubmenu = openSubmenu === menu ? null : menu;
  }

  function handleHeading(level: 1 | 2 | 3) {
    editor?.chain().focus().toggleHeading({ level }).run();
    collapseMenu();
  }

  function handleList(type: "bullet" | "ordered" | "task") {
    if (!editor) return;
    switch (type) {
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
    if (element.contains(target)) return;

    isExpanded = false;
    openSubmenu = null;
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
  onpointerdown={(e) => {
    // Prevent focus loss when clicking on the menu.
    // pointerdown fires before mousedown/focus, so preventDefault here
    // keeps the editor focused and prevents TipTap's FloatingMenu from hiding.
    e.preventDefault();
  }}
>
  {#if isExpanded}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="menu-expanded" onclick={(e) => e.stopPropagation()} onkeydown={handleMenuKeydown}>
      <div class="menu-section">
        <div class="submenu-wrapper">
          <button
            type="button"
            class="menu-item"
            title="Heading"
            onclick={(e) => toggleSubmenu("heading", e)}
            aria-expanded={openSubmenu === "heading"}
          >
            <Heading class="size-4" />
            <ChevronDown class="size-3 chevron" />
          </button>
          {#if openSubmenu === "heading"}
            <div class="submenu-dropdown">
              <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleHeading(1); }}>H1</button>
              <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleHeading(2); }}>H2</button>
              <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleHeading(3); }}>H3</button>
            </div>
          {/if}
        </div>
      </div>

      <div class="menu-divider"></div>

      <div class="menu-section">
        <div class="submenu-wrapper">
          <button
            type="button"
            class="menu-item"
            title="List"
            onclick={(e) => toggleSubmenu("list", e)}
            aria-expanded={openSubmenu === "list"}
          >
            <List class="size-4" />
            <ChevronDown class="size-3 chevron" />
          </button>
          {#if openSubmenu === "list"}
            <div class="submenu-dropdown">
              <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleList("bullet"); }}>
                <List class="size-3.5" /> Bullet
              </button>
              <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleList("ordered"); }}>
                <ListOrdered class="size-3.5" /> Numbered
              </button>
              <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleList("task"); }}>
                <ListTodo class="size-3.5" /> Task
              </button>
            </div>
          {/if}
        </div>
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
    overflow: visible;
  }

  .menu-section {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 2px;
  }

  .submenu-wrapper {
    position: relative;
  }

  .submenu-dropdown {
    position: absolute;
    top: 100%;
    left: 50%;
    transform: translateX(-50%);
    margin-top: 4px;
    display: flex;
    flex-direction: column;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    padding: 4px;
    min-width: max-content;
    z-index: 30;
  }

  .submenu-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
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
    touch-action: manipulation;
  }

  .submenu-item:hover {
    background: var(--accent);
    color: var(--accent-foreground);
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

  /* Widen menu items that have a chevron to fit both icons */
  .submenu-wrapper .menu-item {
    width: 40px;
    gap: 1px;
  }

  :global(.chevron) {
    opacity: 0.5;
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

    .submenu-wrapper .menu-item {
      width: 44px;
    }

    .menu-divider {
      height: 16px;
    }
  }
</style>
