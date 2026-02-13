<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Pilcrow,
    Heading1,
    Heading2,
    Heading3,
    List,
    ListOrdered,
    ListTodo,
    Quote,
    Braces,
    ChevronDown,
  } from "@lucide/svelte";
  import type { Component } from "svelte";

  interface Props {
    editor: Editor | null;
    open?: boolean;
    onOpen?: () => void;
  }

  let { editor, open = $bindable(false), onOpen }: Props = $props();
  let wrapperElement: HTMLDivElement | null = $state(null);

  type BlockType =
    | "paragraph"
    | "heading1"
    | "heading2"
    | "heading3"
    | "bulletList"
    | "orderedList"
    | "taskList"
    | "blockquote"
    | "codeBlock";

  interface BlockOption {
    type: BlockType;
    label: string;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    icon: Component<any>;
    action: () => void;
  }

  // Use a tick counter to trigger reactivity when the editor state changes.
  // This avoids the state_unsafe_mutation error from updating $state inside event handlers
  // that may fire during template evaluation.
  let editorTick = $state(0);

  let activeBlock = $derived.by(() => {
    // Read tick to establish reactive dependency
    void editorTick;
    if (!editor) return "paragraph" as BlockType;
    if (editor.isActive("heading", { level: 1 })) return "heading1" as BlockType;
    if (editor.isActive("heading", { level: 2 })) return "heading2" as BlockType;
    if (editor.isActive("heading", { level: 3 })) return "heading3" as BlockType;
    if (editor.isActive("bulletList")) return "bulletList" as BlockType;
    if (editor.isActive("orderedList")) return "orderedList" as BlockType;
    if (editor.isActive("taskList")) return "taskList" as BlockType;
    if (editor.isActive("blockquote")) return "blockquote" as BlockType;
    if (editor.isActive("codeBlock")) return "codeBlock" as BlockType;
    return "paragraph" as BlockType;
  });

  const headingOptions: BlockOption[] = [
    {
      type: "heading1",
      label: "Heading 1",
      icon: Heading1,
      action: () => editor?.chain().focus().toggleHeading({ level: 1 }).run(),
    },
    {
      type: "heading2",
      label: "Heading 2",
      icon: Heading2,
      action: () => editor?.chain().focus().toggleHeading({ level: 2 }).run(),
    },
    {
      type: "heading3",
      label: "Heading 3",
      icon: Heading3,
      action: () => editor?.chain().focus().toggleHeading({ level: 3 }).run(),
    },
  ];

  const listOptions: BlockOption[] = [
    {
      type: "bulletList",
      label: "Bullet List",
      icon: List,
      action: () => editor?.chain().focus().toggleBulletList().run(),
    },
    {
      type: "orderedList",
      label: "Numbered List",
      icon: ListOrdered,
      action: () => editor?.chain().focus().toggleOrderedList().run(),
    },
    {
      type: "taskList",
      label: "Task List",
      icon: ListTodo,
      action: () => editor?.chain().focus().toggleTaskList().run(),
    },
  ];

  const otherOptions: BlockOption[] = [
    {
      type: "blockquote",
      label: "Blockquote",
      icon: Quote,
      action: () => editor?.chain().focus().toggleBlockquote().run(),
    },
    {
      type: "codeBlock",
      label: "Code Block",
      icon: Braces,
      action: () => editor?.chain().focus().toggleCodeBlock().run(),
    },
    {
      type: "paragraph",
      label: "Paragraph",
      icon: Pilcrow,
      action: () => editor?.chain().focus().setParagraph().run(),
    },
  ];

  const allOptions = [...headingOptions, ...listOptions, ...otherOptions];

  function bumpEditorTick() {
    // Defer to avoid state_unsafe_mutation when called from TipTap transaction
    // handlers that fire during Svelte template evaluation
    queueMicrotask(() => { editorTick++; });
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let ActiveIcon: Component<any> = $derived(
    allOptions.find((o) => o.type === activeBlock)?.icon ?? Pilcrow,
  );

  function handleSelect(option: BlockOption) {
    option.action();
    open = false;
    bumpEditorTick();
  }

  function handleClick(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (!open) onOpen?.();
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

  // Set up click outside listener when open
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

  // Update active block state when editor changes
  $effect(() => {
    if (!editor) return;

    const ed = editor;
    const handleUpdate = () => bumpEditorTick();

    ed.on("selectionUpdate", handleUpdate);
    ed.on("transaction", handleUpdate);

    return () => {
      ed.off("selectionUpdate", handleUpdate);
      ed.off("transaction", handleUpdate);
    };
  });
</script>

<div class="block-style-picker-wrapper" bind:this={wrapperElement}>
  <button
    type="button"
    class="toolbar-button"
    onmousedown={handleClick}
    title="Block style"
    aria-haspopup="true"
    aria-expanded={open}
  >
    <span class="block-style-button-content">
      <ActiveIcon class="size-4" />
      <ChevronDown class="size-3 chevron-indicator" />
    </span>
  </button>

  {#if open}
    <div
      class="block-style-dropdown"
      role="menu"
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
    >
      <div class="icon-grid">
        {#each allOptions as option}
          <button
            type="button"
            class="icon-button"
            class:active={activeBlock === option.type}
            onclick={() => handleSelect(option)}
            title={option.label}
            role="menuitem"
          >
            <option.icon class="size-4" />
          </button>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .block-style-picker-wrapper {
    position: relative;
    display: inline-flex;
  }

  .block-style-button-content {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  :global(.chevron-indicator) {
    opacity: 0.5;
  }

  .block-style-dropdown {
    position: absolute;
    top: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    padding: 6px;
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
      transform: translateX(-50%) translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  .icon-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
  }

  .icon-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 4px;
    background: transparent;
    border: none;
    color: var(--foreground);
    cursor: pointer;
    transition: all 0.1s ease;
    -webkit-user-select: none;
    user-select: none;
  }

  .icon-button:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .icon-button.active {
    background: var(--accent);
    color: var(--accent-foreground);
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
    .block-style-picker-wrapper {
      /* Remove positioning context so dropdown centers relative to .bubble-menu instead of this button */
      position: static;
    }

    .toolbar-button {
      padding: 8px;
      min-width: 36px;
      min-height: 36px;
    }

    .icon-button {
      width: 36px;
      height: 36px;
    }
  }
</style>
