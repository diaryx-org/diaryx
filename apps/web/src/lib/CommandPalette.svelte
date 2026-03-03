<script lang="ts">
  import { tick } from "svelte";
  import * as Command from "$lib/components/ui/command";
  import * as Drawer from "$lib/components/ui/drawer";
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
    onFindInFile?: () => void;
    onWordCount?: () => void;
    onCopyAsMarkdown?: () => void;
    onViewMarkdown?: () => void;
    onReorderFootnotes?: () => void;
  }

  let {
    open = $bindable(),
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
  <!-- Mobile: Use Drawer from top -->
  <Drawer.Root bind:open direction="top">
    <Drawer.Content>
      <div class="mx-auto w-full max-w-md px-4 pb-4 pt-[env(safe-area-inset-top)]">
        <Command.Root class="rounded-lg border-none shadow-none">
          {@render commandContent()}
        </Command.Root>
      </div>
    </Drawer.Content>
  </Drawer.Root>
{:else}
  <!-- Desktop: Use Dialog -->
  <Command.Dialog bind:open title="Command Palette" description="Search or run a command">
    {@render commandContent()}
  </Command.Dialog>
{/if}
