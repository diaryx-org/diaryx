<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { ChevronRight, Folder, FileText } from "@lucide/svelte";
  import type { TreeNode, Api } from "$lib/backend";

  interface Props {
    tree: TreeNode | null;
    rootIndexPath: string | null;
    api: Api | null;
    onSave: (title: string, parentPath: string | null) => void;
    onCancel: () => void;
  }

  let { tree, rootIndexPath, api, onSave, onCancel }: Props = $props();

  let name = $state("");
  let selectedParentPath = $state<string | null>(null);
  let filenamePreview = $state("");

  // Sync selectedParentPath with rootIndexPath prop
  $effect(() => {
    if (rootIndexPath && selectedParentPath === null) {
      selectedParentPath = rootIndexPath;
    }
  });

  // Track expanded nodes in the parent picker tree
  let pickerExpanded = $state(new Set<string>());

  // Pre-expand root node
  $effect(() => {
    if (!tree || pickerExpanded.has(tree.path)) return;
    pickerExpanded = new Set([...pickerExpanded, tree.path]);
  });

  // Debounced filename preview via Rust backend
  let previewTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const trimmed = name.trim();
    if (!trimmed || !api) {
      filenamePreview = "";
      return;
    }
    if (previewTimer) clearTimeout(previewTimer);
    previewTimer = setTimeout(async () => {
      try {
        filenamePreview = await api!.generateFilename(trimmed, rootIndexPath ?? undefined);
      } catch {
        filenamePreview = "";
      }
    }, 100);
  });

  function handleSave() {
    if (!name.trim()) return;
    onSave(name.trim(), selectedParentPath);
    onCancel();
  }

  function handleOpenChange(isOpen: boolean) {
    if (!isOpen) {
      onCancel();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && name.trim()) {
      e.preventDefault();
      handleSave();
    }
  }

  function selectParent(path: string) {
    selectedParentPath = path;
  }

  function togglePickerNode(path: string) {
    if (pickerExpanded.has(path)) {
      pickerExpanded.delete(path);
    } else {
      pickerExpanded.add(path);
    }
    pickerExpanded = new Set(pickerExpanded);
  }

  function getNodeDisplayName(node: TreeNode): string {
    return node.name.replace(".md", "");
  }
</script>

<Dialog.Root open={true} onOpenChange={handleOpenChange}>
  <Dialog.Content class="sm:max-w-[480px]">
    <Dialog.Header>
      <Dialog.Title>New Entry</Dialog.Title>
      <Dialog.Description>
        Create a new entry in your workspace.
      </Dialog.Description>
    </Dialog.Header>

    <div class="grid gap-4 py-4">
      <!-- Name Input -->
      <div class="grid gap-2">
        <Label for="entry-name">Name</Label>
        <Input
          id="entry-name"
          bind:value={name}
          placeholder="My New Entry"
          onkeydown={handleKeydown}
          autofocus
        />
        {#if filenamePreview}
          <p class="text-xs text-muted-foreground font-mono">
            {filenamePreview}
          </p>
        {/if}
      </div>

      <!-- Parent Picker -->
      {#if tree}
        <div class="grid gap-2">
          <Label>Add to</Label>
          <div
            class="border rounded-md max-h-48 overflow-y-auto p-1"
            role="tree"
            aria-label="Select parent entry"
          >
            {@render parentNode(tree, 0)}
          </div>
        </div>
      {/if}
    </div>

    <Dialog.Footer>
      <Button variant="outline" onclick={() => handleOpenChange(false)}>
        Cancel
      </Button>
      <Button onclick={handleSave} disabled={!name.trim()}>Create</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

{#snippet parentNode(node: TreeNode, depth: number)}
  <div class="select-none" role="treeitem" aria-selected={selectedParentPath === node.path}>
    <div
      class="flex items-center gap-1 rounded-md hover:bg-accent transition-colors cursor-pointer
        {selectedParentPath === node.path ? 'bg-accent ring-1 ring-primary' : ''}"
      style="padding-left: {depth * 12 + 4}px"
      role="presentation"
    >
      {#if node.children.length > 0}
        <button
          type="button"
          class="p-0.5 rounded-sm hover:bg-accent-foreground/10 transition-colors shrink-0"
          onclick={(e) => {
            e.stopPropagation();
            togglePickerNode(node.path);
          }}
          tabindex={-1}
        >
          <ChevronRight
            class="size-3.5 text-muted-foreground transition-transform duration-200 {pickerExpanded.has(node.path) ? 'rotate-90' : ''}"
          />
        </button>
      {:else}
        <span class="w-5 shrink-0"></span>
      {/if}
      <button
        type="button"
        class="flex-1 min-w-0 flex items-center gap-1.5 py-1.5 pr-2 text-sm text-left rounded-md transition-colors"
        onclick={() => selectParent(node.path)}
      >
        {#if node.children.length > 0}
          <Folder class="size-3.5 shrink-0 text-muted-foreground" />
        {:else}
          <FileText class="size-3.5 shrink-0 text-muted-foreground" />
        {/if}
        <span class="truncate">{getNodeDisplayName(node)}</span>
      </button>
    </div>

    {#if node.children.length > 0 && pickerExpanded.has(node.path)}
      <div role="group">
        {#each node.children.filter((c) => !c.name.startsWith("... (")) as child (child.path)}
          {@render parentNode(child, depth + 1)}
        {/each}
      </div>
    {/if}
  </div>
{/snippet}
