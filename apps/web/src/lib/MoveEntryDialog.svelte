<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Input } from "$lib/components/ui/input";
  import type { TreeNode } from "./backend";
  import {
    getRenderableSidebarChildren,
    findTreeNode,
  } from "./leftSidebarSelection";
  import {
    ChevronRight,
    Folder,
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

  // Compute the set of paths that are descendants of the entry being moved (including itself)
  let disabledPaths = $derived.by(() => {
    const paths = new Set<string>();
    if (!tree || !entryPath) return paths;
    const entryNode = findTreeNode(tree, entryPath);
    if (!entryNode) return paths;
    function collectDescendants(node: TreeNode) {
      paths.add(node.path);
      for (const child of getRenderableSidebarChildren(node)) {
        collectDescendants(child);
      }
    }
    collectDescendants(entryNode);
    return paths;
  });

  // Compute which nodes match the search query (and their ancestors)
  let matchingPaths = $derived.by(() => {
    if (!searchQuery.trim() || !tree) return null;
    const query = searchQuery.toLowerCase();
    const matches = new Set<string>();
    const ancestors = new Set<string>();

    function visit(node: TreeNode, parentChain: string[]) {
      const name = node.name.replace(".md", "").toLowerCase();
      if (name.includes(query)) {
        matches.add(node.path);
        for (const p of parentChain) ancestors.add(p);
      }
      const children = getRenderableSidebarChildren(node);
      for (const child of children) {
        visit(child, [...parentChain, node.path]);
      }
    }
    visit(tree, []);
    return { matches, ancestors };
  });

  // Auto-expand the branch containing the entry being moved on open
  $effect(() => {
    if (open && tree && entryPath) {
      const initial = new Set<string>();
      function findAndExpand(node: TreeNode, chain: string[]): boolean {
        if (node.path === entryPath) {
          for (const p of chain) initial.add(p);
          return true;
        }
        for (const child of getRenderableSidebarChildren(node)) {
          if (findAndExpand(child, [...chain, node.path])) return true;
        }
        return false;
      }
      findAndExpand(tree, []);
      expandedNodes = initial;
      searchQuery = "";
      hoverTarget = null;
    }
  });

  function findParentNode(root: TreeNode, targetPath: string): TreeNode | null {
    for (const child of root.children) {
      if (child.path === targetPath) return root;
      const found = findParentNode(child, targetPath);
      if (found) return found;
    }
    return null;
  }

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
    if (!hoverTarget || !tree) return;
    const { position } = hoverTarget;

    if (position === 'on') {
      onMove(entryPath, path);
    } else {
      const targetParent = findParentNode(tree, path);
      const sourceParent = findParentNode(tree, entryPath);

      if (targetParent && sourceParent && targetParent.path === sourceParent.path) {
        // Same parent — reorder
        const children = getRenderableSidebarChildren(targetParent);
        const childPaths = children.map(c => c.path);
        const fromIndex = childPaths.indexOf(entryPath);
        const toIndex = childPaths.indexOf(path);
        if (fromIndex !== -1 && toIndex !== -1 && fromIndex !== toIndex) {
          childPaths.splice(fromIndex, 1);
          const insertIndex = position === 'below'
            ? childPaths.indexOf(path) + 1
            : childPaths.indexOf(path);
          childPaths.splice(insertIndex, 0, entryPath);
          onReorderChildren(targetParent.path, childPaths);
        }
      } else if (targetParent) {
        // Different parent — reparent with position hint
        onMove(entryPath, targetParent.path, {
          beforePath: position === 'above' ? path : undefined,
          afterPath: position === 'below' ? path : undefined,
        });
      }
    }

    open = false;
    onClose();
  }

  function isNodeVisible(node: TreeNode): boolean {
    if (!matchingPaths) return true;
    return matchingPaths.matches.has(node.path) || matchingPaths.ancestors.has(node.path);
  }

  function isNodeExpanded(path: string): boolean {
    if (matchingPaths) {
      // When searching, auto-expand ancestors of matches
      return matchingPaths.ancestors.has(path) || matchingPaths.matches.has(path);
    }
    return expandedNodes.has(path);
  }

  function highlightMatch(text: string): string {
    if (!searchQuery.trim()) return text;
    const query = searchQuery.toLowerCase();
    const lower = text.toLowerCase();
    const idx = lower.indexOf(query);
    if (idx === -1) return text;
    const before = text.slice(0, idx);
    const match = text.slice(idx, idx + query.length);
    const after = text.slice(idx + query.length);
    return `${before}<mark class="bg-yellow-200 dark:bg-yellow-800 rounded-sm">${match}</mark>${after}`;
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
                {#if isFolder}
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
                      class="size-4 transition-transform duration-200 {!hasChildren
                        ? 'text-muted-foreground/40'
                        : 'text-muted-foreground'} {isNodeExpanded(node.path) ? 'rotate-90' : ''}"
                    />
                  </button>
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
                  {#if hasChildren || node.is_index}
                    <Folder class="size-4 shrink-0 text-muted-foreground" />
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
