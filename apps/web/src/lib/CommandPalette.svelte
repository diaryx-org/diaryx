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
  } from "@lucide/svelte";

  interface Props {
    open: boolean;
    api: Api | null;
    onImportFromClipboard: () => void;
    onImportMarkdownFile: () => void;
    onOpenBackupImport: () => void;
  }

  let {
    open = $bindable(),
    api,
    onImportFromClipboard,
    onImportMarkdownFile,
    onOpenBackupImport,
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
      placeholder="Search backup/import actions..."
      bind:value={searchValue}
    />
    <Command.List>
      <Command.Empty>No results found.</Command.Empty>
      <Command.Group heading="Backup & Import">
        <Command.Item onSelect={() => handleCommand(onOpenBackupImport)}>
          <Settings class="mr-2 size-4" />
          <span>Download Backup ZIP</span>
          <Command.Shortcut>Download zip</Command.Shortcut>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onImportFromClipboard)}>
          <ClipboardPaste class="mr-2 size-4" />
          <span>Import from Clipboard</span>
          <Command.Shortcut>Create entry from clipboard</Command.Shortcut>
        </Command.Item>
        <Command.Item onSelect={() => handleCommand(onImportMarkdownFile)}>
          <FileDown class="mr-2 size-4" />
          <span>Import Markdown File</span>
          <Command.Shortcut>Import .md file(s)</Command.Shortcut>
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
