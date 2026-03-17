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
    Ellipsis,
  } from "@lucide/svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import { getCachedPluginIcon } from "$lib/plugins/pluginIconResolver";

  interface Props {
    editor: Editor;
    showAttachment: boolean;
    onSelect: (action: () => void) => void;
    onInsertAttachment?: () => void;
    onCancel: () => void;
  }

  let { editor, showAttachment, onSelect, onInsertAttachment, onCancel }: Props = $props();

  const pluginStore = getPluginStore();
  const pluginBlockCommands = $derived(pluginStore.editorInsertCommands.block);
  const pluginBlockPickerItems = $derived(pluginStore.blockPickerItems);

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
    event.preventDefault();
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

  function handleAttachment() {
    onInsertAttachment?.();
  }

  function handleBlockPickerItem(item: (typeof pluginBlockPickerItems)[number]) {
    const { contribution } = item;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const params: Record<string, any> = { ...(contribution.params as Record<string, unknown> ?? {}) };
    const editorCommand = contribution.editor_command;
    if (contribution.prompt) {
      const input = window.prompt(contribution.prompt.message, contribution.prompt.default_value);
      if (!input) return;
      params[contribution.prompt.param_key] = input.trim();
    }
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const commands = editor.commands as Record<string, any>;
    const commandFn = commands[editorCommand];
    if (typeof commandFn === "function") {
      onSelect(() => {
        // Re-read the command after the picker atom removes itself so the
        // command executes against the editor's current state.
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const nextCommands = editor.commands as Record<string, any>;
        const nextCommandFn = nextCommands[editorCommand];
        if (typeof nextCommandFn === "function") {
          nextCommandFn(params);
        }
      });
    }
  }

  function handleMenuItemClick(
    event: MouseEvent | TouchEvent,
    action: () => void,
  ) {
    event.preventDefault();
    event.stopPropagation();
    action();
  }

  function handlePointerDownOutside(event: MouseEvent | TouchEvent) {
    if (!menuElement) return;
    const target = event.target as Node;
    if (menuElement.contains(target)) return;
    onCancel();
  }

  $effect(() => {
    if (!menuElement) return;

    // Wait until the next task before listening for outside clicks so the
    // click that opens the picker does not immediately close it again.
    const timeoutId = window.setTimeout(() => {
      document.addEventListener("mousedown", handlePointerDownOutside);
      document.addEventListener("touchend", handlePointerDownOutside);
    }, 0);

    return () => {
      clearTimeout(timeoutId);
      document.removeEventListener("mousedown", handlePointerDownOutside);
      document.removeEventListener("touchend", handlePointerDownOutside);
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  bind:this={menuElement}
  class="block-picker-menu"
  role="toolbar"
  aria-label="Block formatting"
  tabindex="-1"
  onmousedown={(e) => e.preventDefault()}
  onclick={(e) => e.stopPropagation()}
  onkeydown={handleKeydown}
>
  <div class="menu-section">
    <div class="submenu-wrapper">
      <button
        type="button"
        class="menu-item"
        title="Heading"
        onmousedown={(e) => toggleSubmenu("heading", e)}
        aria-expanded={openSubmenu === "heading"}
      >
        <Heading class="size-4" />
        <ChevronDown class="size-3 chevron" />
      </button>
      {#if openSubmenu === "heading"}
        <div class="submenu-dropdown">
          <button type="button" class="submenu-item" onmousedown={(e) => handleMenuItemClick(e, () => handleHeading(1))}>H1</button>
          <button type="button" class="submenu-item" onmousedown={(e) => handleMenuItemClick(e, () => handleHeading(2))}>H2</button>
          <button type="button" class="submenu-item" onmousedown={(e) => handleMenuItemClick(e, () => handleHeading(3))}>H3</button>
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
        onmousedown={(e) => toggleSubmenu("list", e)}
        aria-expanded={openSubmenu === "list"}
      >
        <List class="size-4" />
        <ChevronDown class="size-3 chevron" />
      </button>
      {#if openSubmenu === "list"}
        <div class="submenu-dropdown">
          <button type="button" class="submenu-item" onmousedown={(e) => handleMenuItemClick(e, () => handleList("bullet"))}>
            <List class="size-3.5" /> Bullet
          </button>
          <button type="button" class="submenu-item" onmousedown={(e) => handleMenuItemClick(e, () => handleList("ordered"))}>
            <ListOrdered class="size-3.5" /> Numbered
          </button>
          <button type="button" class="submenu-item" onmousedown={(e) => handleMenuItemClick(e, () => handleList("task"))}>
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
      onmousedown={(e) => handleMenuItemClick(e, handleBlockquote)}
      class="menu-item"
      title="Quote"
    >
      <Quote class="size-4" />
      <span>Blockquote</span>
    </button>
    <button
      type="button"
      onmousedown={(e) => handleMenuItemClick(e, handleCodeBlock)}
      class="menu-item"
      title="Code Block"
    >
      <Braces class="size-4" />
      <span>Code Block</span>
    </button>
    <button
      type="button"
      onmousedown={(e) => handleMenuItemClick(e, handleHorizontalRule)}
      class="menu-item"
      title="Horizontal Rule"
    >
      <Minus class="size-4" />
      <span>Divider</span>
    </button>
    <button
      type="button"
      onmousedown={(e) => handleMenuItemClick(e, handleTable)}
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
        onmousedown={(e) => toggleSubmenu("more", e)}
        aria-expanded={openSubmenu === "more"}
      >
        <Ellipsis class="size-4" />
        <ChevronDown class="size-3 chevron" />
      </button>
      {#if openSubmenu === "more"}
        <div class="submenu-dropdown">
          <button type="button" class="submenu-item" onmousedown={(e) => handleMenuItemClick(e, handleHtmlBlock)}>
            <Code class="size-3.5" /> HTML
          </button>
          {#if pluginBlockPickerItems.length > 0}
            <div class="submenu-divider"></div>
            {#each pluginBlockPickerItems as item (item.contribution.id)}
              {@const Icon = getCachedPluginIcon(item.contribution.icon)}
              <button type="button" class="submenu-item" onmousedown={(e) => handleMenuItemClick(e, () => handleBlockPickerItem(item))}>
                <Icon class="size-3.5" /> {item.contribution.label}
              </button>
            {/each}
          {/if}
          {#if pluginBlockCommands.length > 0}
            <div class="submenu-divider"></div>
            {#each pluginBlockCommands as cmd (cmd.extensionId)}
              <button type="button" class="submenu-item" onmousedown={(e) => handleMenuItemClick(e, () => {
                const id = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
                onSelect(() => editor.chain().focus().insertContent({
                  type: cmd.extensionId,
                  attrs: { source: `${id}](` },
                }).run());
              })}>
                <cmd.icon class="size-3.5" /> {cmd.label}
              </button>
            {/each}
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
        onmousedown={(e) => handleMenuItemClick(e, handleAttachment)}
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
