<script lang="ts">
  import * as Popover from "$lib/components/ui/popover";
  import * as Command from "$lib/components/ui/command";
  import * as Drawer from "$lib/components/ui/drawer";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
  import type { TreeNode } from "$lib/backend";
  import { FileText } from "@lucide/svelte";
  import type { Snippet } from "svelte";

  interface Props {
    excludePaths?: string[];
    onSelect: (file: { path: string; name: string }) => void;
    placeholder?: string;
    children: Snippet;
  }

  let {
    excludePaths = [],
    onSelect,
    placeholder = "Search files...",
    children,
  }: Props = $props();

  let open = $state(false);
  let searchValue = $state("");

  const mobileState = getMobileState();

  function getAllEntries(node: TreeNode | null): { path: string; name: string }[] {
    if (!node) return [];
    const entries: { path: string; name: string }[] = [];
    function traverse(n: TreeNode) {
      entries.push({ path: n.path, name: n.name });
      for (const child of n.children) {
        traverse(child);
      }
    }
    traverse(node);
    return entries;
  }

  const allEntries = $derived(getAllEntries(workspaceStore.tree));
  const filteredEntries = $derived(
    allEntries
      .filter((e) => !excludePaths.includes(e.path))
      .filter(
        (e) =>
          !searchValue.trim() ||
          e.name.toLowerCase().includes(searchValue.toLowerCase()) ||
          e.path.toLowerCase().includes(searchValue.toLowerCase()),
      ),
  );

  function handleSelect(file: { path: string; name: string }) {
    onSelect(file);
    open = false;
    searchValue = "";
  }
</script>

{#snippet pickerContent()}
  <Command.Root class="rounded-lg border-none shadow-none" shouldFilter={false}>
    <Command.Input placeholder={placeholder} bind:value={searchValue} />
    <Command.List>
      <Command.Empty>No files found.</Command.Empty>
      {#each filteredEntries.slice(0, 20) as file (file.path)}
        <Command.Item value={file.path} onSelect={() => handleSelect(file)}>
          <FileText class="mr-2 size-4 shrink-0" />
          <div class="flex flex-col min-w-0">
            <span class="truncate text-sm">{file.name}</span>
            <span class="truncate text-xs text-muted-foreground">{file.path}</span>
          </div>
        </Command.Item>
      {/each}
    </Command.List>
  </Command.Root>
{/snippet}

{#if mobileState.isMobile}
  <Drawer.Root bind:open direction="bottom">
    <Drawer.Trigger>
      {@render children()}
    </Drawer.Trigger>
    <Drawer.Content>
      <div class="mx-auto w-full max-w-md px-4 pb-4 pt-2">
        {@render pickerContent()}
      </div>
    </Drawer.Content>
  </Drawer.Root>
{:else}
  <Popover.Root bind:open>
    <Popover.Trigger>
      {@render children()}
    </Popover.Trigger>
    <Popover.Content side="left" align="start" class="w-72 p-0">
      {@render pickerContent()}
    </Popover.Content>
  </Popover.Root>
{/if}
