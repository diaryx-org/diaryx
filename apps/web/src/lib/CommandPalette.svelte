<script lang="ts">
  import { tick } from "svelte";
  import * as Command from "$lib/components/ui/command";
  import type { Api } from "./backend/api";
  import { getMobileState } from "./hooks/useMobile.svelte";
  import { getPluginStore } from "../models/stores/pluginStore.svelte";
  import PluginSidebarPanel from "$lib/components/PluginSidebarPanel.svelte";
  import {
    Settings,
    ClipboardPaste,
    FileDown,
    Copy,
    Pencil,
    Trash2,
    FolderInput,
    FilePlus,
    RefreshCw,
    ShieldCheck,
    Search,
    LetterText,
    ClipboardCopy,
    Code,
    ListOrdered,
  } from "@lucide/svelte";

  interface Props {
    open: boolean;
    /** 0-1 during an interactive swipe-up gesture, null otherwise */
    swipeProgress?: number | null;
    api: Api | null;
    hasEntry: boolean;
    hasEditor: boolean;
    onImportFromClipboard: () => void;
    onImportMarkdownFile: () => void;
    onOpenBackupImport: () => void;
    onDuplicateEntry?: () => void;
    onRenameEntry?: () => void;
    onDeleteEntry?: () => void;
    onMoveEntry?: () => void;
    onCreateChildEntry?: () => void;
    onRefreshTree?: () => void;
    onValidateWorkspace?: () => void;
    onOpenWorkspaceSettings?: () => void;
    onFindInFile?: () => void;
    onWordCount?: () => void;
    onCopyAsMarkdown?: () => void;
    onViewMarkdown?: () => void;
    onReorderFootnotes?: () => void;
  }

  let {
    open = $bindable(),
    swipeProgress = null,
    api,
    hasEntry,
    hasEditor,
    onImportFromClipboard,
    onImportMarkdownFile,
    onOpenBackupImport,
    onDuplicateEntry,
    onRenameEntry,
    onDeleteEntry,
    onMoveEntry,
    onCreateChildEntry,
    onRefreshTree,
    onValidateWorkspace,
    onOpenWorkspaceSettings,
    onFindInFile,
    onWordCount,
    onCopyAsMarkdown,
    onViewMarkdown,
    onReorderFootnotes,
  }: Props = $props();

  const pluginStore = getPluginStore();
  const commandPaletteOwner = $derived(pluginStore.commandPaletteOwner);

  let searchValue = $state("");

  async function handleCommand(action: () => void | Promise<void>) {
    open = false;
    searchValue = "";
    // Let the palette dialog unmount before executing commands that open another dialog.
    await tick();
    await action();
  }

  const mobileState = getMobileState();

  // Show the mobile sheet when open OR actively swiping OR animating closed
  const swiping = $derived(swipeProgress != null && swipeProgress > 0);
  let closing = $state(false);
  const showMobileSheet = $derived(open || swiping || closing);

  /** Animate close, then actually set open = false */
  function closeWithAnimation() {
    if (closing) return;
    closing = true;
    // Wait for the CSS transition (300ms) to finish before unmounting
    setTimeout(() => {
      open = false;
      searchValue = "";
      closing = false;
    }, 300);
  }

  // Drag-to-dismiss: only activates from the drag handle area at the top of the sheet.
  // Touching the scrollable command list does NOT start a dismiss gesture.
  let dismissDragY = $state(0);
  let dismissDragging = $state(false);
  let dismissStartY = 0;

  function handleHandleTouchStart(e: TouchEvent) {
    if (!open || e.touches.length !== 1) return;
    dismissStartY = e.touches[0].clientY;
    dismissDragY = 0;
    dismissDragging = true;
  }

  function handleHandleTouchMove(e: TouchEvent) {
    if (!dismissDragging || e.touches.length !== 1) return;
    const delta = e.touches[0].clientY - dismissStartY;
    dismissDragY = Math.max(0, delta);
  }

  function handleHandleTouchEnd() {
    if (!dismissDragging) return;
    // If dragged more than 80px downward, dismiss
    if (dismissDragY > 80) {
      closeWithAnimation();
    }
    dismissDragY = 0;
    dismissDragging = false;
  }
</script>

{#snippet commandContent()}
  {#if commandPaletteOwner && api}
    <div class="h-[60vh] max-h-[640px] overflow-hidden">
      <PluginSidebarPanel
        pluginId={commandPaletteOwner.pluginId}
        component={commandPaletteOwner.contribution.component}
        {api}
      />
    </div>
  {:else}
    <Command.Input
      placeholder="Search commands..."
      bind:value={searchValue}
    />
    <Command.List>
      <Command.Empty>No results found.</Command.Empty>

      {#if hasEntry}
        <Command.Group heading="Entry">
          {#if onDuplicateEntry}
            <Command.Item onSelect={() => handleCommand(onDuplicateEntry)}>
              <Copy class="mr-2 size-4" />
              <span>Duplicate Entry</span>
            </Command.Item>
          {/if}
          {#if onRenameEntry}
            <Command.Item onSelect={() => handleCommand(onRenameEntry)}>
              <Pencil class="mr-2 size-4" />
              <span>Rename Entry</span>
            </Command.Item>
          {/if}
          {#if onDeleteEntry}
            <Command.Item onSelect={() => handleCommand(onDeleteEntry)}>
              <Trash2 class="mr-2 size-4" />
              <span>Delete Entry</span>
            </Command.Item>
          {/if}
          {#if onMoveEntry}
            <Command.Item onSelect={() => handleCommand(onMoveEntry)}>
              <FolderInput class="mr-2 size-4" />
              <span>Move Entry</span>
            </Command.Item>
          {/if}
          {#if onCreateChildEntry}
            <Command.Item onSelect={() => handleCommand(onCreateChildEntry)}>
              <FilePlus class="mr-2 size-4" />
              <span>Create Child Entry</span>
            </Command.Item>
          {/if}
        </Command.Group>
      {/if}

      {#if hasEditor}
        <Command.Group heading="Editor">
          {#if onFindInFile}
            <Command.Item onSelect={() => handleCommand(onFindInFile)}>
              <Search class="mr-2 size-4" />
              <span>Find in File</span>
              <Command.Shortcut>Cmd/Ctrl+F</Command.Shortcut>
            </Command.Item>
          {/if}
          {#if onWordCount}
            <Command.Item onSelect={() => handleCommand(onWordCount)}>
              <LetterText class="mr-2 size-4" />
              <span>Word Count</span>
            </Command.Item>
          {/if}
          {#if onCopyAsMarkdown}
            <Command.Item onSelect={() => handleCommand(onCopyAsMarkdown)}>
              <ClipboardCopy class="mr-2 size-4" />
              <span>Copy as Markdown</span>
            </Command.Item>
          {/if}
          {#if onViewMarkdown}
            <Command.Item onSelect={() => handleCommand(onViewMarkdown)}>
              <Code class="mr-2 size-4" />
              <span>View Markdown Source</span>
            </Command.Item>
          {/if}
          {#if onReorderFootnotes}
            <Command.Item onSelect={() => handleCommand(onReorderFootnotes)}>
              <ListOrdered class="mr-2 size-4" />
              <span>Reorder Footnotes</span>
            </Command.Item>
          {/if}
        </Command.Group>
      {/if}

      <Command.Group heading="Workspace">
        {#if onOpenWorkspaceSettings}
          <Command.Item onSelect={() => handleCommand(onOpenWorkspaceSettings)}>
            <Settings class="mr-2 size-4" />
            <span>Workspace Settings</span>
          </Command.Item>
        {/if}
        {#if onRefreshTree}
          <Command.Item onSelect={() => handleCommand(onRefreshTree)}>
            <RefreshCw class="mr-2 size-4" />
            <span>Refresh Tree</span>
          </Command.Item>
        {/if}
        {#if onValidateWorkspace}
          <Command.Item onSelect={() => handleCommand(onValidateWorkspace)}>
            <ShieldCheck class="mr-2 size-4" />
            <span>Validate Workspace</span>
          </Command.Item>
        {/if}
        <Command.Item onSelect={() => handleCommand(onOpenBackupImport)}>
          <Settings class="mr-2 size-4" />
          <span>Download Backup ZIP</span>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onImportFromClipboard)}>
          <ClipboardPaste class="mr-2 size-4" />
          <span>Import from Clipboard</span>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onImportMarkdownFile)}>
          <FileDown class="mr-2 size-4" />
          <span>Import Markdown File</span>
        </Command.Item>
      </Command.Group>
    </Command.List>
  {/if}
{/snippet}

{#if mobileState.isMobile}
  {#if showMobileSheet}
    <!-- Backdrop -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-50 {swiping ? 'pointer-events-none' : ''}"
      style="background: rgba(0,0,0,{closing
        ? 0
        : open
          ? (dismissDragging ? Math.max(0, 0.5 - dismissDragY / 600) : 0.5)
          : (swipeProgress ?? 0) * 0.5});
             {!swiping && !dismissDragging ? 'transition: background 0.3s ease-in-out;' : ''}"
      onclick={closeWithAnimation}
      onkeydown={(e) => { if (e.key === 'Escape') closeWithAnimation(); }}
    ></div>

    <!-- Sheet -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-x-0 bottom-0 z-50 rounded-t-lg border-t bg-background max-h-[80vh] overflow-hidden {swiping ? 'pointer-events-none' : ''}"
      style="transform: translateY({closing
        ? '100%'
        : open
          ? (dismissDragging ? dismissDragY + 'px' : '0')
          : (100 - (swipeProgress ?? 0) * 100) + '%'});
             {!swiping && !dismissDragging ? 'transition: transform 0.3s ease-in-out;' : ''}"
    >
      <!-- Drag handle (touch target for dismiss gesture) -->
      <div
        class="flex justify-center py-4 cursor-grab active:cursor-grabbing"
        ontouchstart={handleHandleTouchStart}
        ontouchmove={handleHandleTouchMove}
        ontouchend={handleHandleTouchEnd}
        ontouchcancel={handleHandleTouchEnd}
      >
        <div class="h-2 w-[100px] shrink-0 rounded-full bg-muted"></div>
      </div>

      <div class="mx-auto w-full max-w-md px-4 pb-[calc(env(safe-area-inset-bottom)+1rem)]">
        <Command.Root class="rounded-lg border-none shadow-none">
          {@render commandContent()}
        </Command.Root>
      </div>
    </div>
  {/if}
{:else}
  <!-- Desktop: Use Dialog -->
  <Command.Dialog bind:open title="Command Palette" description="Search or run a command">
    {@render commandContent()}
  </Command.Dialog>
{/if}
