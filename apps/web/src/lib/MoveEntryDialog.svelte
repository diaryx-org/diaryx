<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Input } from "$lib/components/ui/input";
  import type { TreeNode } from "./backend";
  import {
    getRenderableSidebarChildren,
  } from "./leftSidebarSelection";
  import {
    collectInitiallyExpandedMovePaths,
    collectMoveDisabledPaths,
    collectMoveMatchingPaths,
    computeMoveDialogAction,
    highlightMoveQueryMatch,
    isMoveNodeExpanded,
    isMoveNodeVisible,
  } from "./moveEntryDialog";
  import {
    ChevronRight,
    FolderClosed,
    FolderMinus,
    FolderOpen,
    FolderPlus,
    FileText,
  } from "@lucide/svelte";

  interface Props {
    open: boolean;
    entryPath: string;
    tree: TreeNode | null;
    onMove: (entryPath: string, targetPath: string, position?: { beforePath?: string; afterPath?: string }) => void;
    onReorderChildren: (parentPath: string, childPaths: string[]) => void;
    onClose: () => void;
  }

  let {
    open = $bindable(false),
    entryPath,
    tree,
    onMove,
    onReorderChildren,
    onClose,
  }: Props = $props();

  let searchQuery = $state("");
  let hoverTarget = $state<{ path: string; position: 'above' | 'below' | 'on' } | null>(null);
  let expandedNodes = $state(new Set<string>());

  let disabledPaths = $derived.by(() => {
    return collectMoveDisabledPaths(tree, entryPath);
  });

  let matchingPaths = $derived.by(() => {
    return collectMoveMatchingPaths(tree, searchQuery);
  });

  $effect(() => {
    if (open && tree && entryPath) {
      expandedNodes = collectInitiallyExpandedMovePaths(tree, entryPath);
      searchQuery = "";
      hoverTarget = null;
    }
  });

  function toggleExpand(path: string) {
    const next = new Set(expandedNodes);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    expandedNodes = next;
  }

  function handlePointerMove(e: PointerEvent, path: string, hasChildren: boolean) {
    if (disabledPaths.has(path)) {
      hoverTarget = null;
      return;
    }
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const relY = (e.clientY - rect.top) / rect.height;

    if (hasChildren) {
      // Folder: three-zone — top 25% above, middle 50% reparent, bottom 25% below
      if (relY < 0.25) hoverTarget = { path, position: 'above' };
      else if (relY > 0.75) hoverTarget = { path, position: 'below' };
      else hoverTarget = { path, position: 'on' };
    } else {
      // Leaf: two-zone — top/bottom halves for reorder only
      // (reparent into leaf is handled by the dedicated gutter button)
      hoverTarget = relY < 0.5
        ? { path, position: 'above' }
        : { path, position: 'below' };
    }
  }

  function handleReparentZoneEnter(path: string) {
    if (disabledPaths.has(path)) return;
    hoverTarget = { path, position: 'on' };
  }

  function handleReparentZoneClick(e: MouseEvent, path: string) {
    e.stopPropagation();
    if (!tree || disabledPaths.has(path)) return;
    onMove(entryPath, path);
    open = false;
    onClose();
  }

  function handlePointerLeave() {
    hoverTarget = null;
  }

  function handleClick(path: string) {
    const action = computeMoveDialogAction(tree, entryPath, hoverTarget && { ...hoverTarget, path });
    if (!action) return;

    if (action.type === "reorder") {
      onReorderChildren(action.parentPath, action.childPaths);
    } else {
      onMove(entryPath, action.targetPath, action.position);
    }

    open = false;
    onClose();
  }

  function isNodeVisible(node: TreeNode): boolean {
    return isMoveNodeVisible(node.path, matchingPaths);
  }

  function isNodeExpanded(path: string): boolean {
    return isMoveNodeExpanded(path, expandedNodes, matchingPaths);
  }

  function highlightMatch(text: string): string {
    return highlightMoveQueryMatch(text, searchQuery);
  }

  function handleOpenChange(newOpen: boolean) {
    open = newOpen;
    if (!newOpen) onClose();
  }

  function getEntryDisplayName(): string {
    if (!entryPath) return "";
    return entryPath.split("/").pop()?.replace(".md", "") ?? entryPath;
  }
</script>

<Dialog.Root bind:open onOpenChange={handleOpenChange}>
  <Dialog.Content class="sm:max-w-lg max-h-[80vh] flex flex-col">
    <Dialog.Header>
      <Dialog.Title>Move Entry</Dialog.Title>
      <Dialog.Description>
        Move "<span class="font-medium">{getEntryDisplayName()}</span>" to a new location
      </Dialog.Description>
    </Dialog.Header>

    <div class="px-1 py-2">
      <Input
        placeholder="Search entries..."
        bind:value={searchQuery}
        class="h-9"
      />
    </div>

    <div class="overflow-y-auto flex-1 min-h-0 px-1 pb-2" role="tree">
      {#if tree}
        {#snippet moveTreeNode(node: TreeNode, depth: number)}
          {@const children = getRenderableSidebarChildren(node)}
          {@const hasChildren = children.length > 0}
          {@const isFolder = hasChildren || node.is_index}
          {@const isSource = node.path === entryPath}
          {@const isDisabled = disabledPaths.has(node.path)}
          {@const isHoverAbove = hoverTarget?.path === node.path && hoverTarget.position === 'above'}
          {@const isHoverBelow = hoverTarget?.path === node.path && hoverTarget.position === 'below'}
          {@const isHoverOn = hoverTarget?.path === node.path && hoverTarget.position === 'on'}

          {#if isNodeVisible(node)}
            <div class="select-none" role="treeitem" aria-level={depth + 1} aria-selected={false}>
              <div
                class="flex items-center gap-1 rounded-md transition-colors
                  {isSource ? 'opacity-40 pointer-events-none' : ''}
                  {isDisabled && !isSource ? 'opacity-50' : ''}
                  {!isDisabled && !isSource ? 'hover:bg-sidebar-accent cursor-pointer' : ''}
                  {isHoverOn ? 'bg-primary/20 ring-2 ring-primary' : ''}
                  {isHoverAbove ? 'border-t-2 border-primary' : ''}
                  {isHoverBelow ? 'border-b-2 border-primary' : ''}"
                style="padding-left: {depth * 12 + 4}px;"
                role="presentation"
                onpointermove={(e) => !isDisabled && handlePointerMove(e, node.path, isFolder)}
                onpointerleave={handlePointerLeave}
                onclick={() => !isDisabled && handleClick(node.path)}
              >
                {#if hasChildren}
                  <button
                    type="button"
                    class="p-1 rounded-sm hover:bg-sidebar-accent transition-colors"
                    onclick={(e) => {
                      e.stopPropagation();
                      toggleExpand(node.path);
                    }}
                    aria-label="Toggle folder"
                    tabindex={-1}
                  >
                    <ChevronRight
                      class="size-4 transition-transform duration-200 text-muted-foreground {isNodeExpanded(node.path) ? 'rotate-90' : ''}"
                    />
                  </button>
                {:else if node.is_index}
                  <span class="w-6"></span>
                {:else if !isSource && !isDisabled}
                  <button
                    type="button"
                    class="p-1 rounded-sm transition-colors
                      {isHoverOn ? 'bg-primary/20 text-primary' : 'text-muted-foreground/40 hover:text-muted-foreground hover:bg-sidebar-accent'}"
                    onclick={(e) => handleReparentZoneClick(e, node.path)}
                    onpointerenter={() => handleReparentZoneEnter(node.path)}
                    onpointerleave={handlePointerLeave}
                    onpointermove={(e) => e.stopPropagation()}
                    aria-label="Move into {node.name.replace('.md', '')} (converts to folder)"
                    title="Move into (converts to folder)"
                    tabindex={-1}
                  >
                    <FolderPlus class="size-4" />
                  </button>
                {:else}
                  <span class="w-6"></span>
                {/if}

                <div class="flex-1 min-w-0 flex items-center gap-2 py-1.5 pr-2 text-sm">
                  {#if hasChildren && isNodeExpanded(node.path)}
                    <FolderOpen class="size-4 shrink-0 text-muted-foreground" />
                  {:else if hasChildren}
                    <FolderClosed class="size-4 shrink-0 text-muted-foreground" />
                  {:else if node.is_index}
                    <FolderMinus class="size-4 shrink-0 text-muted-foreground" />
                  {:else}
                    <FileText class="size-4 shrink-0 text-muted-foreground" />
                  {/if}
                  <span class="truncate flex-1">
                    {#if searchQuery.trim()}
                      {@html highlightMatch(node.name.replace(".md", ""))}
                    {:else}
                      {node.name.replace(".md", "")}
                    {/if}
                  </span>
                  {#if isSource}
                    <span class="text-xs text-muted-foreground italic">(moving)</span>
                  {/if}
                </div>
              </div>

              {#if isFolder && isNodeExpanded(node.path)}
                <div role="group">
                  {#each children as child (child.path)}
                    {@render moveTreeNode(child, depth + 1)}
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        {/snippet}

        {@render moveTreeNode(tree, 0)}
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
