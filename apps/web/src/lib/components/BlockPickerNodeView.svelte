<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Heading,
    List,
    ListOrdered,
    ListTodo,
    ChevronDown,
    Quote,
    Braces,
    Minus,
    Paperclip,
    Code,
    Table2,
    Pencil,
    Ellipsis,
    GitBranch,
    Users,
  } from "@lucide/svelte";
  import AudienceInlineSelector from "./AudienceInlineSelector.svelte";

  interface Props {
    editor: Editor;
    showAttachment: boolean;
    onSelect: (action: () => void) => void;
    onInsertAttachment?: () => void;
    onCancel: () => void;
  }

  let { editor, showAttachment, onSelect, onInsertAttachment, onCancel }: Props = $props();

  let menuElement: HTMLDivElement | undefined = $state();
  let focusedIndex = $state(0);
  let focusableItems: HTMLElement[] = $state([]);
  let openSubmenu = $state<"heading" | "list" | "more" | null>(null);
  let showAudienceSelector = $state(false);

  function updateFocusableItems() {
    focusableItems = Array.from(
      menuElement?.querySelectorAll('.menu-item, .submenu-item') || []
    ) as HTMLElement[];
  }

  // Auto-focus first item on mount
  $effect(() => {
    if (menuElement) {
      // Use setTimeout to ensure DOM is fully rendered
      setTimeout(() => {
        updateFocusableItems();
        if (focusableItems.length > 0) {
          focusedIndex = 0;
          focusableItems[0]?.focus();
        }
      }, 0);
    }
  });

  function handleKeydown(event: KeyboardEvent) {
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        if (showAudienceSelector) {
          showAudienceSelector = false;
        } else if (openSubmenu) {
          openSubmenu = null;
        } else {
          onCancel();
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

  function toggleSubmenu(menu: "heading" | "list" | "more", event: MouseEvent | TouchEvent) {
    event.stopPropagation();
    openSubmenu = openSubmenu === menu ? null : menu;
    showAudienceSelector = false;
  }

  function handleHeading(level: 1 | 2 | 3) {
    onSelect(() => editor.chain().focus().toggleHeading({ level }).run());
  }

  function handleList(type: "bullet" | "ordered" | "task") {
    onSelect(() => {
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
    });
  }

  function handleBlockquote() {
    onSelect(() => editor.chain().focus().toggleBlockquote().run());
  }

  function handleCodeBlock() {
    onSelect(() => editor.chain().focus().toggleCodeBlock().run());
  }

  function handleHorizontalRule() {
    onSelect(() => editor.chain().focus().setHorizontalRule().run());
  }

  function handleHtmlBlock() {
    onSelect(() => editor.commands.insertHtmlBlock());
  }

  function handleTable() {
    onSelect(() =>
      editor.chain().focus().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run()
    );
  }

  function handleDrawing() {
    onSelect(() => editor.commands.insertDrawingBlock());
  }

  function handleAttachment() {
    onInsertAttachment?.();
  }

  function handleIfElse() {
    const condition = window.prompt("Variable name to check:", "draft");
    if (!condition) return;
    onSelect(() =>
      editor.commands.insertConditionalBlock({
        helperType: "if",
        condition: condition.trim(),
      })
    );
  }

  function handleForAudience() {
    showAudienceSelector = true;
  }

  function handleAudienceSelect(audience: string) {
    showAudienceSelector = false;
    onSelect(() =>
      editor.commands.insertConditionalBlock({
        helperType: "for-audience",
        condition: audience,
      })
    );
  }

  function handleAudienceCancel() {
    showAudienceSelector = false;
  }

  function handleMenuItemClick(
    event: MouseEvent | TouchEvent,
    action: () => void,
  ) {
    event.stopPropagation();
    action();
  }

  function handleClickOutside(event: MouseEvent | TouchEvent) {
    if (!menuElement) return;
    const target = event.target as Node;
    if (menuElement.contains(target)) return;
    onCancel();
  }
</script>

<svelte:window onclick={handleClickOutside} ontouchend={handleClickOutside} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  bind:this={menuElement}
  class="block-picker-menu"
  role="toolbar"
  aria-label="Block formatting"
  tabindex="-1"
  onclick={(e) => e.stopPropagation()}
  onkeydown={handleKeydown}
>
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
    <button
      type="button"
      onclick={(e) => handleMenuItemClick(e, handleTable)}
      class="menu-item"
      title="Table"
    >
      <Table2 class="size-4" />
      <span>Table</span>
    </button>
  </div>

  <div class="menu-divider"></div>

  <div class="menu-section">
    <div class="submenu-wrapper">
      <button
        type="button"
        class="menu-item"
        title="More blocks"
        onclick={(e) => toggleSubmenu("more", e)}
        aria-expanded={openSubmenu === "more"}
      >
        <Ellipsis class="size-4" />
        <ChevronDown class="size-3 chevron" />
      </button>
      {#if openSubmenu === "more"}
        <div class="submenu-dropdown">
          {#if showAudienceSelector}
            <AudienceInlineSelector
              onSelect={handleAudienceSelect}
              onCancel={handleAudienceCancel}
            />
          {:else}
            <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleHtmlBlock(); }}>
              <Code class="size-3.5" /> HTML
            </button>
            <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleDrawing(); }}>
              <Pencil class="size-3.5" /> Drawing
            </button>
            <div class="submenu-divider"></div>
            <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleIfElse(); }}>
              <GitBranch class="size-3.5" /> If / Else
            </button>
            <button type="button" class="submenu-item" onclick={(e) => { e.stopPropagation(); handleForAudience(); }}>
              <Users class="size-3.5" /> For Audience
            </button>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  {#if showAttachment}
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

<style>
  .block-picker-menu {
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
    margin: 4px 0;
    width: fit-content;
    -webkit-user-select: none;
    user-select: none;
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

  .submenu-divider {
    height: 1px;
    background: var(--border);
    margin: 4px 0;
    opacity: 0.5;
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
    -webkit-user-select: none;
    user-select: none;
    touch-action: manipulation;
  }

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

  @media (max-width: 767px) {
    .block-picker-menu {
      max-width: 90vw;
    }

    .menu-divider {
      height: 16px;
    }
  }
</style>
