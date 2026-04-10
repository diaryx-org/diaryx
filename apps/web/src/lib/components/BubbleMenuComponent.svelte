<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Bold,
    Italic,
    Link as LinkIcon,
    Unlink,
    RemoveFormatting,
    Columns2,
    Trash2,
  } from "@lucide/svelte";
  import AttributeMarkPicker from "./AttributeMarkPicker.svelte";
  import BlockStylePicker from "./BlockStylePicker.svelte";
  import MoreStylesPicker from "./MoreStylesPicker.svelte";
  import LinkInsertPopover from "./LinkInsertPopover.svelte";
  import { Eye, EyeOff } from "@lucide/svelte";
  import { getAudiencePanelStore } from "$lib/stores/audiencePanelStore.svelte";
  import type { Api } from "$lib/backend/api";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import { getVisibilityBlockForSelection } from "$lib/extensions/VisibilityBlock";

  interface Props {
    editor: Editor | null;
    element?: HTMLDivElement;
    /** Current entry path for resolving local links */
    entryPath?: string;
    /** API instance for link formatting */
    api?: Api | null;
    linkPopoverOpen?: boolean;
  }

  interface SelectedLink {
    href: string;
    canonicalPath?: string;
    isLocal: boolean;
  }

  let {
    editor,
    element = $bindable(),
    entryPath = "",
    api = null,
    linkPopoverOpen = $bindable(false),
  }: Props = $props();

  // Track active states reactively
  let isBoldActive = $state(false);
  let isItalicActive = $state(false);
  let isLinkActive = $state(false);
  let isVisActive = $state(false);
  let isInCodeBlock = $state(false);
  let isInTable = $state(false);

  // Generic mark toolbar entries (e.g., highlight color picker)
  const markToolbarEntries = $derived(getPluginStore().markToolbarEntries);
  let markActiveStates = $state<Record<string, boolean>>({});
  let markCurrentValues = $state<Record<string, string | null>>({});
  let markPickerOpen = $state<Record<string, boolean>>({});

  function updateActiveStates() {
    if (!editor) return;
    isBoldActive = editor.isActive("bold");
    isItalicActive = editor.isActive("italic");
    for (const entry of markToolbarEntries) {
      const extName = entry.extensionId;
      const hasExt = editor.extensionManager.extensions.some(e => e.name === extName);
      if (hasExt) {
        markActiveStates[extName] = editor.isActive(extName);
        if (markActiveStates[extName] && entry.attribute) {
          const attrs = editor.getAttributes(extName);
          markCurrentValues[extName] = (attrs[entry.attribute.name] as string) || entry.attribute.default;
        } else {
          markCurrentValues[extName] = null;
        }
      }
    }
    isLinkActive = editor.isActive("link");
    isVisActive =
      editor.isActive("visibilityMark") ||
      getVisibilityBlockForSelection(editor.state) !== null;
    isInCodeBlock = editor.isActive("codeBlock");
    isInTable = editor.isActive("table");
  }

  function handleBold() {
    editor?.chain().focus().toggleBold().run();
    updateActiveStates();
  }

  function handleItalic() {
    editor?.chain().focus().toggleItalic().run();
    updateActiveStates();
  }

  function isLocalHref(href: string): boolean {
    const trimmed = href.trim().toLowerCase();
    return !(
      /^[a-z][a-z0-9+.-]*:/.test(trimmed)
      || trimmed.startsWith("#")
      || trimmed.startsWith("//")
    );
  }

  function getCurrentMarkdown(): string {
    const markdownStorage = editor?.storage?.markdown as
      | { getMarkdown?: () => string }
      | undefined;
    return markdownStorage?.getMarkdown?.() ?? "";
  }

  async function syncAddedLocalLink(canonicalPath?: string) {
    if (!api || !entryPath || !canonicalPath) return;
    await api.addLink(entryPath, canonicalPath, getCurrentMarkdown());
  }

  async function syncRemovedLocalLink(href?: string) {
    if (!api || !entryPath || !href || !isLocalHref(href)) return;
    const canonicalPath = await api.canonicalizeLink(href, entryPath);
    await api.removeLink(entryPath, canonicalPath, getCurrentMarkdown());
  }

  // Dropdown mutual exclusion: only one open at a time
  let blockStyleOpen = $state(false);
  let moreStylesOpen = $state(false);

  function closeAllDropdowns() {
    blockStyleOpen = false;
    linkPopoverOpen = false;
    moreStylesOpen = false;
    for (const key of Object.keys(markPickerOpen)) {
      markPickerOpen[key] = false;
    }
  }

  async function handleLink() {
    if (isLinkActive) {
      const href = editor?.getAttributes?.("link")?.href as string | undefined;
      editor?.chain().focus().unsetLink().run();
      updateActiveStates();
      try {
        await syncRemovedLocalLink(href);
      } catch (error) {
        console.warn("[BubbleMenuComponent] Failed to remove local link metadata:", error);
      }
    } else {
      closeAllDropdowns();
      linkPopoverOpen = true;
    }
  }

  async function handleLinkSelect(link: SelectedLink) {
    editor?.chain().focus().setLink({ href: link.href }).run();
    linkPopoverOpen = false;
    updateActiveStates();
    if (link.isLocal) {
      try {
        await syncAddedLocalLink(link.canonicalPath);
      } catch (error) {
        console.warn("[BubbleMenuComponent] Failed to add local link metadata:", error);
      }
    }
  }

  function handleLinkClose() {
    linkPopoverOpen = false;
    editor?.commands.focus();
  }

  // Update active states when editor changes
  $effect(() => {
    if (!editor) return;

    const ed = editor;
    // Defer to avoid state_unsafe_mutation when called from TipTap transaction
    // handlers that fire during Svelte template evaluation
    const handleUpdate = () => queueMicrotask(() => updateActiveStates());

    ed.on("selectionUpdate", handleUpdate);
    ed.on("transaction", handleUpdate);

    // Initial update
    updateActiveStates();

    return () => {
      ed.off("selectionUpdate", handleUpdate);
      ed.off("transaction", handleUpdate);
    };
  });
</script>

<div
  bind:this={element}
  class="bubble-menu"
  role="toolbar"
  aria-label="Text formatting"
  tabindex="-1"
  onmousedown={(e) => {
    // Prevent focus loss when clicking on the menu
    e.preventDefault();
  }}
  ontouchstart={(e) => {
    // Same for touch events on mobile
    e.preventDefault();
  }}
>
  {#if isInTable}
    <button
      type="button"
      class="toolbar-button"
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        editor?.chain().focus().toggleHeaderRow().run();
      }}
      title="Toggle header row"
    >
      <RemoveFormatting class="size-4" />
    </button>

    <button
      type="button"
      class="toolbar-button"
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        editor?.chain().focus().toggleHeaderColumn().run();
      }}
      title="Toggle header column"
    >
      <Columns2 class="size-4" />
    </button>

    <button
      type="button"
      class="toolbar-button destructive"
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        editor?.chain().focus().deleteTable().run();
      }}
      title="Delete table"
    >
      <Trash2 class="size-4" />
    </button>
  {:else}
    <BlockStylePicker {editor} bind:open={blockStyleOpen} onOpen={() => { closeAllDropdowns(); }} />
  {/if}

  {#if !isInCodeBlock}
    <div class="toolbar-divider"></div>

    <button
      type="button"
      class="toolbar-button"
      class:active={isBoldActive}
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        handleBold();
      }}
      title="Bold"
      aria-pressed={isBoldActive}
    >
      <Bold class="size-4" />
    </button>

    <button
      type="button"
      class="toolbar-button"
      class:active={isItalicActive}
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        handleItalic();
      }}
      title="Italic"
      aria-pressed={isItalicActive}
    >
      <Italic class="size-4" />
    </button>

    {#each markToolbarEntries as entry (entry.extensionId)}
      <AttributeMarkPicker
        {editor}
        {entry}
        isActive={markActiveStates[entry.extensionId] ?? false}
        currentValue={markCurrentValues[entry.extensionId]}
        open={markPickerOpen[entry.extensionId] ?? false}
        onOpen={() => { closeAllDropdowns(); markPickerOpen[entry.extensionId] = true; }}
      />
    {/each}

    <button
      type="button"
      class="toolbar-button"
      class:active={isVisActive}
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        getAudiencePanelStore().openPanel("paint");
      }}
      title="Audience visibility"
    >
      {#if isVisActive}
        <EyeOff class="size-4" />
      {:else}
        <Eye class="size-4" />
      {/if}
    </button>

    <div class="link-button-wrapper">
      <button
        type="button"
        class="toolbar-button"
        class:active={isLinkActive}
        onmousedown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          void handleLink();
        }}
        title={isLinkActive ? "Remove Link" : "Add Link"}
        aria-pressed={isLinkActive}
      >
        {#if isLinkActive}
          <Unlink class="size-4" />
        {:else}
          <LinkIcon class="size-4" />
        {/if}
      </button>

      <LinkInsertPopover
        bind:open={linkPopoverOpen}
        onSelect={handleLinkSelect}
        onClose={handleLinkClose}
        currentEntryPath={entryPath}
        {api}
      />
    </div>

    <div class="toolbar-divider"></div>

    <MoreStylesPicker {editor} bind:open={moreStylesOpen} onOpen={() => { closeAllDropdowns(); }} />
  {/if}
</div>

<style>
  .bubble-menu {
    /* Keep measurable for Floating UI before first show, but hidden until positioned. */
    display: flex;
    visibility: hidden;
    opacity: 0;
    pointer-events: none;
    align-items: center;
    gap: 2px;
    padding: 4px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 50;
    position: fixed;
    /* Prevent text selection on toolbar */
    -webkit-user-select: none;
    user-select: none;
  }

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
    box-shadow: inset 0 -2px 0 var(--ring);
  }

  .toolbar-button.destructive {
    color: var(--destructive, oklch(0.577 0.245 27.325));
  }

  .toolbar-button.destructive:hover {
    background: var(--destructive, oklch(0.577 0.245 27.325));
    color: white;
  }

  .link-button-wrapper {
    position: relative;
    display: inline-flex;
  }

  .toolbar-divider {
    width: 1px;
    height: 16px;
    background: var(--border);
    margin: 0 2px;
    flex-shrink: 0;
  }

  /* Mobile-specific adjustments for touch targets */
  @media (max-width: 767px) {
    .bubble-menu {
      gap: 2px;
      padding: 4px;
      flex-wrap: wrap;
      max-width: min(90vw, 300px);
      justify-content: center;
    }

    .toolbar-button {
      padding: 10px;
      min-width: 44px;
      min-height: 44px;
    }

    .toolbar-divider {
      height: 18px;
      margin: 0 2px;
    }
  }
</style>
