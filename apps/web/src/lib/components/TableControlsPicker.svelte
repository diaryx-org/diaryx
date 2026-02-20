<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Table,
    BetweenHorizontalStart,
    BetweenHorizontalEnd,
    BetweenVerticalStart,
    BetweenVerticalEnd,
    TableCellsMerge,
    TableCellsSplit,
    RemoveFormatting,
    Rows2,
    Columns2,
    Trash2,
  } from "@lucide/svelte";

  interface Props {
    editor: Editor | null;
    open?: boolean;
    onOpen?: () => void;
  }

  let { editor, open = $bindable(false), onOpen }: Props = $props();
  let wrapperElement: HTMLDivElement | null = $state(null);
  let showBelow = $state(false);

  let canMergeCells = $state(false);
  let canSplitCell = $state(false);

  function updateStates() {
    if (!editor) return;
    canMergeCells = editor.can().mergeCells();
    canSplitCell = editor.can().splitCell();
  }

  function handleAddRowBefore() {
    editor?.chain().focus().addRowBefore().run();
  }

  function handleAddRowAfter() {
    editor?.chain().focus().addRowAfter().run();
  }

  function handleDeleteRow() {
    editor?.chain().focus().deleteRow().run();
  }

  function handleAddColumnBefore() {
    editor?.chain().focus().addColumnBefore().run();
  }

  function handleAddColumnAfter() {
    editor?.chain().focus().addColumnAfter().run();
  }

  function handleDeleteColumn() {
    editor?.chain().focus().deleteColumn().run();
  }

  function handleToggleHeaderRow() {
    editor?.chain().focus().toggleHeaderRow().run();
  }

  function handleToggleHeaderColumn() {
    editor?.chain().focus().toggleHeaderColumn().run();
  }

  function handleMergeCells() {
    editor?.chain().focus().mergeCells().run();
    updateStates();
  }

  function handleSplitCell() {
    editor?.chain().focus().splitCell().run();
    updateStates();
  }

  function handleDeleteTable() {
    editor?.chain().focus().deleteTable().run();
  }

  function handleClick(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (!open) {
      onOpen?.();
      if (wrapperElement) {
        const rect = wrapperElement.getBoundingClientRect();
        showBelow = rect.top < 320;
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
      updateStates();
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
    const handleUpdate = () => queueMicrotask(() => updateStates());

    ed.on("selectionUpdate", handleUpdate);
    ed.on("transaction", handleUpdate);
    updateStates();

    return () => {
      ed.off("selectionUpdate", handleUpdate);
      ed.off("transaction", handleUpdate);
    };
  });
</script>

<div class="table-controls-wrapper" bind:this={wrapperElement}>
  <button
    type="button"
    class="toolbar-button"
    onmousedown={handleClick}
    title="Table controls"
    aria-haspopup="true"
    aria-expanded={open}
  >
    <Table class="size-4" />
  </button>

  {#if open}
    <div
      class="table-controls-dropdown"
      class:show-below={showBelow}
      role="menu"
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
    >
      <div class="table-controls-section-label">Rows</div>
      <button
        type="button"
        class="table-controls-option"
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleAddRowBefore();
        }}
        title="Insert row above"
        role="menuitem"
      >
        <BetweenHorizontalStart class="size-4" />
        <span>Insert above</span>
      </button>
      <button
        type="button"
        class="table-controls-option"
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleAddRowAfter();
        }}
        title="Insert row below"
        role="menuitem"
      >
        <BetweenHorizontalEnd class="size-4" />
        <span>Insert below</span>
      </button>
      <button
        type="button"
        class="table-controls-option destructive"
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleDeleteRow();
        }}
        title="Delete row"
        role="menuitem"
      >
        <Rows2 class="size-4" />
        <span>Delete row</span>
      </button>

      <div class="table-controls-divider"></div>

      <div class="table-controls-section-label">Columns</div>
      <button
        type="button"
        class="table-controls-option"
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleAddColumnBefore();
        }}
        title="Insert column left"
        role="menuitem"
      >
        <BetweenVerticalStart class="size-4" />
        <span>Insert left</span>
      </button>
      <button
        type="button"
        class="table-controls-option"
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleAddColumnAfter();
        }}
        title="Insert column right"
        role="menuitem"
      >
        <BetweenVerticalEnd class="size-4" />
        <span>Insert right</span>
      </button>
      <button
        type="button"
        class="table-controls-option destructive"
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleDeleteColumn();
        }}
        title="Delete column"
        role="menuitem"
      >
        <Columns2 class="size-4" />
        <span>Delete column</span>
      </button>

      <div class="table-controls-divider"></div>

      <div class="table-controls-section-label">Header</div>
      <button
        type="button"
        class="table-controls-option"
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleToggleHeaderRow();
        }}
        title="Toggle header row"
        role="menuitem"
      >
        <RemoveFormatting class="size-4" />
        <span>Toggle header row</span>
      </button>
      <button
        type="button"
        class="table-controls-option"
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleToggleHeaderColumn();
        }}
        title="Toggle header column"
        role="menuitem"
      >
        <RemoveFormatting class="size-4" />
        <span>Toggle header col</span>
      </button>

      {#if canMergeCells || canSplitCell}
        <div class="table-controls-divider"></div>

        <div class="table-controls-section-label">Cells</div>
        {#if canMergeCells}
          <button
            type="button"
            class="table-controls-option"
            onmousedown={(e) => {
              e.preventDefault();
              e.stopPropagation();
              handleMergeCells();
            }}
            title="Merge cells"
            role="menuitem"
          >
            <TableCellsMerge class="size-4" />
            <span>Merge cells</span>
          </button>
        {/if}
        {#if canSplitCell}
          <button
            type="button"
            class="table-controls-option"
            onmousedown={(e) => {
              e.preventDefault();
              e.stopPropagation();
              handleSplitCell();
            }}
            title="Split cell"
            role="menuitem"
          >
            <TableCellsSplit class="size-4" />
            <span>Split cell</span>
          </button>
        {/if}
      {/if}

      <div class="table-controls-divider"></div>

      <button
        type="button"
        class="table-controls-option destructive"
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          handleDeleteTable();
        }}
        title="Delete table"
        role="menuitem"
      >
        <Trash2 class="size-4" />
        <span>Delete table</span>
      </button>
    </div>
  {/if}
</div>

<style>
  .table-controls-wrapper {
    position: relative;
    display: inline-flex;
  }

  .table-controls-dropdown {
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

  .table-controls-dropdown.show-below {
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

  .table-controls-section-label {
    padding: 4px 10px 2px;
    font-size: 11px;
    font-weight: 600;
    color: var(--muted-foreground);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .table-controls-option {
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

  .table-controls-option:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .table-controls-option.destructive {
    color: var(--destructive, oklch(0.577 0.245 27.325));
  }

  .table-controls-option.destructive:hover {
    background: var(--destructive, oklch(0.577 0.245 27.325));
    color: white;
  }

  .table-controls-divider {
    height: 1px;
    background: var(--border);
    margin: 4px 0;
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

  /* Mobile adjustments */
  @media (max-width: 767px) {
    .table-controls-wrapper {
      position: static;
    }

    .toolbar-button {
      padding: 8px;
      min-width: 36px;
      min-height: 36px;
    }

    .table-controls-option {
      padding: 8px 12px;
    }
  }
</style>
