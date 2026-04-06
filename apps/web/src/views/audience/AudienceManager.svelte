<script lang="ts">
  import type { Api } from "$lib/backend/api";
  import type { TreeNode } from "$lib/backend/interface";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor, AUDIENCE_PALETTE } from "$lib/utils/audienceDotColor";
  import { toast } from "svelte-sonner";
  import {
    X,
    Plus,
    Pencil,
    Trash2,
    Check,
    Loader2,
    AlertTriangle,
    ArrowLeft,
    Search,
    FileText,
    FolderOpen,
    FolderClosed,
    ChevronRight,
    RotateCcw,
  } from "@lucide/svelte";

  interface Props {
    api: Api | null;
    rootPath: string;
    onClose: () => void;
  }

  let { api, rootPath, onClose }: Props = $props();

  const mobileState = getMobileState();
  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();

  let closing = $state(false);

  function handleClose() {
    closing = true;
    setTimeout(onClose, 200);
  }

  // ── Audience list state ───────────────────────────────────────────────
  let audiences = $state<string[]>([]);
  let audienceLoading = $state(true);

  // Create
  let isCreating = $state(false);
  let createValue = $state("");

  // Rename
  let editingAudience = $state<string | null>(null);
  let editValue = $state("");

  // Delete
  let deleteCandidate = $state<{ name: string; count: number } | null>(null);
  let deleteWorking = $state(false);

  // Working flag for rename
  let working = $state(false);

  // Color picker
  let colorPickerOpen = $state<string | null>(null);

  // ── Active brush for painting ─────────────────────────────────────────
  let activeBrush = $state<string | null>(null);
  let showDetail = $state(false); // mobile tree slide-over

  // ── Entry tree + map state ───────────────────────────────────────────
  let workspaceTree = $state<TreeNode | null>(null);
  let entryTitleMap = $state<Map<string, string>>(new Map());
  // null = no audience property (inherits), [] = explicit empty (opts out), [...] = explicit tags
  let entryAudienceMap = $state<Map<string, string[] | null>>(new Map());
  let entriesLoading = $state(true);
  let togglingPaths = $state<Set<string>>(new Set());

  // Folder collapse
  let collapsedFolders = $state<Set<string>>(new Set());

  // Entry search
  let entrySearch = $state("");

  // ── Helpers ───────────────────────────────────────────────────────────

  /** Returns the display title for a path */
  function getTitle(path: string): string {
    return entryTitleMap.get(path) ?? path.split("/").pop()?.replace(/\.md$/, "") ?? path;
  }

  /** Check if a node or any descendant matches the search query */
  function nodeMatchesSearch(node: TreeNode, q: string): boolean {
    const title = getTitle(node.path).toLowerCase();
    const pathName = node.path.toLowerCase();
    if (title.includes(q) || pathName.includes(q)) return true;
    return node.children.some((c) => nodeMatchesSearch(c, q));
  }

  /** Build a parent lookup from the tree for ancestor-chain walking */
  let parentMap = $derived.by(() => {
    const map = new Map<string, string>();
    function walk(node: TreeNode) {
      for (const child of node.children) {
        map.set(child.path, node.path);
        walk(child);
      }
    }
    if (workspaceTree) walk(workspaceTree);
    return map;
  });

  /** Resolve effective audience tags for display.
   *  Returns the entry's own explicit tags, or walks ancestors to find
   *  the nearest explicit audience (marking those as inherited). */
  function getEffectiveTags(path: string): { name: string; inherited: boolean }[] {
    const rawTags = entryAudienceMap.get(path);
    if (rawTags !== null && rawTags !== undefined) {
      return rawTags.map((name) => ({ name, inherited: false }));
    }
    // Walk ancestors — stop at the first with an explicit audience property
    let cur = parentMap.get(path);
    while (cur) {
      const tags = entryAudienceMap.get(cur);
      if (tags !== null && tags !== undefined) {
        return tags.map((name) => ({ name, inherited: true }));
      }
      cur = parentMap.get(cur);
    }
    return [];
  }

  /** Count entries that explicitly include this audience tag */
  function getEntryCount(audienceName: string): number {
    let count = 0;
    for (const tags of entryAudienceMap.values()) {
      if (tags?.includes(audienceName)) count++;
    }
    return count;
  }

  /** Recursively collect all paths in the tree */
  function collectPaths(node: TreeNode): string[] {
    const out: string[] = [node.path];
    for (const child of node.children) out.push(...collectPaths(child));
    return out;
  }

  // ── Load data on mount ────────────────────────────────────────────────
  $effect(() => {
    if (api && rootPath) {
      loadAudiences();
      loadEntries();
    }
  });

  async function loadAudiences() {
    audienceLoading = true;
    try {
      audiences = await api!.getAvailableAudiences(rootPath);
      for (const name of audiences) colorStore.assignColor(name);
    } catch {
      audiences = [];
    } finally {
      audienceLoading = false;
    }
  }

  async function loadEntries() {
    entriesLoading = true;
    try {
      const tree = await api!.getWorkspaceTree(rootPath);
      const paths = collectPaths(tree);
      const titleMap = new Map<string, string>();
      const audMap = new Map<string, string[] | null>();

      const results = await Promise.allSettled(
        paths.map(async (path) => {
          const fm = await api!.getFrontmatter(path);
          const title =
            (fm.title as string) ??
            path
              .split("/")
              .pop()
              ?.replace(/\.md$/, "") ??
            path;
          // null = no audience property (inherits from parent)
          // []   = explicitly empty (opts out of inheritance)
          // [..] = explicit audience tags
          const aud =
            fm.audience !== undefined
              ? Array.isArray(fm.audience)
                ? (fm.audience as string[])
                : []
              : null;
          return { path, title, aud };
        }),
      );

      for (const r of results) {
        if (r.status === "fulfilled") {
          titleMap.set(r.value.path, r.value.title);
          audMap.set(r.value.path, r.value.aud);
        }
      }

      workspaceTree = tree;
      entryTitleMap = titleMap;
      entryAudienceMap = audMap;
    } catch (e) {
      console.error("[AudienceManager] Failed to load entries:", e);
    } finally {
      entriesLoading = false;
    }
  }

  // ── Painting ──────────────────────────────────────────────────────────

  /** Paint the active brush onto an entry (toggle on/off).
   *  For inheriting entries, this makes the audience explicit. */
  async function paintEntry(entryPath: string) {
    if (!api || !activeBrush || togglingPaths.has(entryPath)) return;

    const rawTags = entryAudienceMap.get(entryPath) ?? null;
    const effective = getEffectiveTags(entryPath).map((t) => t.name);
    const has = effective.includes(activeBrush);

    const newTags = has
      ? effective.filter((t) => t !== activeBrush)
      : [...effective, activeBrush];

    // Optimistic update
    entryAudienceMap.set(entryPath, newTags);
    entryAudienceMap = new Map(entryAudienceMap);

    togglingPaths = new Set([...togglingPaths, entryPath]);
    try {
      // Always write explicitly — even [] signals opt-out from inheritance
      await api.setFrontmatterProperty(entryPath, "audience", newTags, rootPath);
    } catch (e) {
      entryAudienceMap.set(entryPath, rawTags);
      entryAudienceMap = new Map(entryAudienceMap);
      toast.error("Failed to update audience");
      console.error("[AudienceManager] Paint failed:", e);
    } finally {
      togglingPaths = new Set([...togglingPaths].filter((p) => p !== entryPath));
    }
  }

  /** Remove a specific audience tag from an entry's explicit list */
  async function removeAudienceFromEntry(entryPath: string, audienceName: string) {
    if (!api || togglingPaths.has(entryPath)) return;

    const rawTags = entryAudienceMap.get(entryPath) ?? null;
    const currentTags = rawTags ?? [];
    if (!currentTags.includes(audienceName)) return;

    const newTags = currentTags.filter((t) => t !== audienceName);

    entryAudienceMap.set(entryPath, newTags);
    entryAudienceMap = new Map(entryAudienceMap);

    togglingPaths = new Set([...togglingPaths, entryPath]);
    try {
      await api.setFrontmatterProperty(entryPath, "audience", newTags, rootPath);
    } catch (e) {
      entryAudienceMap.set(entryPath, rawTags);
      entryAudienceMap = new Map(entryAudienceMap);
      toast.error("Failed to update audience");
      console.error("[AudienceManager] Remove tag failed:", e);
    } finally {
      togglingPaths = new Set([...togglingPaths].filter((p) => p !== entryPath));
    }
  }

  /** Remove explicit audience — entry reverts to inheriting from parent */
  async function setInherited(entryPath: string) {
    if (!api || togglingPaths.has(entryPath)) return;

    const rawTags = entryAudienceMap.get(entryPath) ?? null;
    if (rawTags === null) return; // Already inheriting

    entryAudienceMap.set(entryPath, null);
    entryAudienceMap = new Map(entryAudienceMap);

    togglingPaths = new Set([...togglingPaths, entryPath]);
    try {
      await api.removeFrontmatterProperty(entryPath, "audience");
    } catch (e) {
      entryAudienceMap.set(entryPath, rawTags);
      entryAudienceMap = new Map(entryAudienceMap);
      toast.error("Failed to update audience");
      console.error("[AudienceManager] Set inherited failed:", e);
    } finally {
      togglingPaths = new Set([...togglingPaths].filter((p) => p !== entryPath));
    }
  }

  // ── Create ────────────────────────────────────────────────────────────
  function startCreate() {
    isCreating = true;
    createValue = "";
    editingAudience = null;
    deleteCandidate = null;
    colorPickerOpen = null;
  }

  function cancelCreate() {
    isCreating = false;
    createValue = "";
  }

  function confirmCreate() {
    const name = createValue.trim();
    if (!name) {
      cancelCreate();
      return;
    }
    if (audiences.some((a) => a.toLowerCase() === name.toLowerCase())) {
      toast.error(`"${name}" already exists`);
      return;
    }
    colorStore.assignColor(name);
    audiences = [...audiences, name].sort();
    templateContextStore.bumpAudiencesVersion();
    cancelCreate();
    // Auto-select as active brush
    activeBrush = name;
    if (mobileState.isMobile) showDetail = true;
  }

  // ── Rename ────────────────────────────────────────────────────────────
  function startEdit(name: string) {
    editingAudience = name;
    editValue = name;
    deleteCandidate = null;
    isCreating = false;
    colorPickerOpen = null;
  }

  function cancelEdit() {
    editingAudience = null;
    editValue = "";
  }

  async function findEntriesWithAudience(target: string): Promise<string[]> {
    if (!api) return [];
    const tree = await api.getWorkspaceTree(rootPath);
    const allPaths = collectPaths(tree);
    const settled = await Promise.allSettled(
      allPaths.map(async (path) => {
        const fm = await api!.getFrontmatter(path);
        const aud = fm.audience;
        return Array.isArray(aud) && aud.includes(target) ? path : null;
      }),
    );
    return settled
      .filter((r) => r.status === "fulfilled" && r.value !== null)
      .map((r) => (r as PromiseFulfilledResult<string>).value);
  }

  async function confirmRename() {
    const oldName = editingAudience;
    const newName = editValue.trim();
    if (!oldName || !newName || !api) {
      cancelEdit();
      return;
    }
    if (oldName === newName) {
      cancelEdit();
      return;
    }
    if (
      audiences.some(
        (a) => a !== oldName && a.toLowerCase() === newName.toLowerCase(),
      )
    ) {
      toast.error(`"${newName}" already exists`);
      return;
    }
    working = true;
    try {
      const paths = await findEntriesWithAudience(oldName);
      await Promise.all(
        paths.map(async (path) => {
          const fm = await api!.getFrontmatter(path);
          const updated = (fm.audience as string[]).map((a) =>
            a === oldName ? newName : a,
          );
          await api!.setFrontmatterProperty(path, "audience", updated, rootPath);
          const entry = await api!.getEntry(path);
          const marker = `{{#for-audience "${oldName}"}}`;
          if (entry.content.includes(marker)) {
            const newContent = entry.content.replaceAll(
              marker,
              `{{#for-audience "${newName}"}}`,
            );
            await api!.saveEntry(path, newContent, rootPath);
          }
        }),
      );
      colorStore.renameColor(oldName, newName);
      audiences = audiences.map((a) => (a === oldName ? newName : a));
      // Update entryAudienceMap
      for (const [path, tags] of entryAudienceMap) {
        if (tags?.includes(oldName)) {
          entryAudienceMap.set(
            path,
            tags.map((t) => (t === oldName ? newName : t)),
          );
        }
      }
      entryAudienceMap = new Map(entryAudienceMap);
      if (activeBrush === oldName) activeBrush = newName;
      templateContextStore.bumpAudiencesVersion();
      toast.success(
        `Renamed "${oldName}" \u2192 "${newName}" across ${paths.length} entr${paths.length === 1 ? "y" : "ies"}`,
      );
    } catch (e) {
      console.error("[AudienceManager] Rename failed:", e);
      toast.error("Rename failed \u2014 check console");
    } finally {
      working = false;
      cancelEdit();
    }
  }

  // ── Delete ────────────────────────────────────────────────────────────
  async function requestDelete(name: string) {
    if (!api) return;
    editingAudience = null;
    isCreating = false;
    colorPickerOpen = null;
    working = true;
    try {
      const paths = await findEntriesWithAudience(name);
      deleteCandidate = { name, count: paths.length };
    } catch {
      deleteCandidate = { name, count: 0 };
    } finally {
      working = false;
    }
  }

  function cancelDelete() {
    deleteCandidate = null;
  }

  async function confirmDelete() {
    if (!deleteCandidate || !api) return;
    const { name } = deleteCandidate;
    deleteWorking = true;
    try {
      const paths = await findEntriesWithAudience(name);
      await Promise.all(
        paths.map(async (path) => {
          const fm = await api!.getFrontmatter(path);
          const remaining = (fm.audience as string[]).filter((a) => a !== name);
          if (remaining.length === 0) {
            await api!.removeFrontmatterProperty(path, "audience");
          } else {
            await api!.setFrontmatterProperty(
              path,
              "audience",
              remaining,
              rootPath,
            );
          }
        }),
      );
      colorStore.deleteColor(name);
      audiences = audiences.filter((a) => a !== name);
      // Update entryAudienceMap
      for (const [path, tags] of entryAudienceMap) {
        if (tags?.includes(name)) {
          const remaining = tags.filter((t) => t !== name);
          entryAudienceMap.set(path, remaining);
        }
      }
      entryAudienceMap = new Map(entryAudienceMap);
      if (activeBrush === name) activeBrush = null;
      templateContextStore.bumpAudiencesVersion();
      toast.success(
        `Deleted "${name}" from ${paths.length} entr${paths.length === 1 ? "y" : "ies"}`,
      );
    } catch (e) {
      console.error("[AudienceManager] Delete failed:", e);
      toast.error("Delete failed \u2014 check console");
    } finally {
      deleteWorking = false;
      deleteCandidate = null;
    }
  }

  // ── Color picker ──────────────────────────────────────────────────────
  function toggleColorPicker(name: string) {
    colorPickerOpen = colorPickerOpen === name ? null : name;
  }

  function pickColor(name: string, tailwindClass: string) {
    colorStore.setColor(name, tailwindClass);
    colorPickerOpen = null;
  }

  // ── Brush selection ───────────────────────────────────────────────────
  function selectBrush(name: string) {
    if (activeBrush === name) {
      activeBrush = null;
    } else {
      activeBrush = name;
      if (mobileState.isMobile) showDetail = true;
    }
  }

  // ── Folder collapse/expand ────────────────────────────────────────────
  function toggleFolder(path: string) {
    const next = new Set(collapsedFolders);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    collapsedFolders = next;
  }
</script>

<div
  class="fixed inset-0 z-50 bg-background overflow-hidden {closing
    ? 'animate-marketplace-out'
    : 'animate-marketplace-in'}"
>
  <div class="h-full flex flex-col">
    <!-- Header -->
    <header
      class="border-b px-4 py-3 flex items-center justify-between gap-3 pt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height)+0.75rem)] shrink-0"
    >
      <div class="flex items-center gap-2 min-w-0">
        <div>
          <h2 class="text-lg font-semibold">Manage Audiences</h2>
          <p class="text-xs text-muted-foreground hidden sm:block">
            Select an audience, then click entries to paint them.
          </p>
        </div>
      </div>
      <Button
        variant="ghost"
        size="icon"
        class="size-11 md:size-9"
        onclick={handleClose}
        aria-label="Close audience manager"
      >
        <X class="size-5 md:size-4" />
      </Button>
    </header>

    <!-- Body -->
    <div
      class="flex-1 min-h-0 grid grid-rows-[minmax(0,1fr)] grid-cols-1 md:grid-cols-[300px_minmax(0,1fr)]"
    >
      <!-- Left column: Audience palette -->
      <section
        class="min-h-0 overflow-auto {mobileState.isMobile
          ? ''
          : 'border-r'} pb-[env(safe-area-inset-bottom)]"
      >
        <!-- New audience button -->
        <div class="p-3 border-b">
          {#if isCreating}
            <div class="flex items-center gap-2">
              <span
                class="size-2.5 rounded-full shrink-0 {getAudienceColor(
                  createValue,
                  colorStore.audienceColors,
                )}"
              ></span>
              <!-- svelte-ignore a11y_autofocus -->
              <input
                class="flex-1 min-w-0 h-11 md:h-8 bg-transparent border-b border-border focus:border-primary outline-none text-base md:text-sm py-0.5 placeholder:text-muted-foreground"
                placeholder="New audience name..."
                bind:value={createValue}
                onkeydown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    confirmCreate();
                  }
                  if (e.key === "Escape") {
                    e.preventDefault();
                    cancelCreate();
                  }
                }}
                autofocus
              />
              <button
                class="p-2.5 md:p-0.5 text-muted-foreground hover:text-primary transition-colors rounded"
                onclick={confirmCreate}
                aria-label="Add audience"
              >
                <Check class="size-5 md:size-4" />
              </button>
              <button
                class="p-2.5 md:p-0.5 text-muted-foreground hover:text-foreground transition-colors rounded"
                onclick={cancelCreate}
                aria-label="Cancel"
              >
                <X class="size-5 md:size-4" />
              </button>
            </div>
          {:else}
            <Button
              variant="outline"
              class="w-full h-11 md:h-9 text-sm"
              onclick={startCreate}
              disabled={working || deleteWorking}
            >
              <Plus class="size-4 md:size-3.5 mr-1.5" />
              New audience
            </Button>
          {/if}
        </div>

        {#if audienceLoading}
          <div
            class="flex items-center justify-center py-8 text-muted-foreground"
          >
            <Loader2 class="size-4 animate-spin mr-2" />
            <span class="text-sm">Loading...</span>
          </div>
        {:else if audiences.length === 0 && !isCreating}
          <p class="py-6 text-center text-sm text-muted-foreground px-3">
            No audiences yet. Create one to get started.
          </p>
        {:else}
          <ul class="divide-y divide-border">
            {#each audiences as name (name)}
              {@const isEditing = editingAudience === name}
              {@const isPendingDelete = deleteCandidate?.name === name}
              {@const dotClass = getAudienceColor(
                name,
                colorStore.audienceColors,
              )}
              {@const pickerOpen = colorPickerOpen === name}
              {@const isActive = activeBrush === name}
              <li
                class="flex flex-col text-sm transition-colors {isPendingDelete
                  ? 'bg-destructive/10'
                  : isActive
                    ? 'bg-accent'
                    : 'hover:bg-muted/40'}"
              >
                <div class="flex items-center gap-2 px-3 py-3 md:py-2">
                  <!-- Color dot -->
                  <button
                    class="size-8 md:size-6 rounded-full shrink-0 flex items-center justify-center focus:outline-none focus:ring-2 focus:ring-offset-1 focus:ring-foreground/30 hover:scale-110 transition-transform"
                    onclick={(e) => {
                      e.stopPropagation();
                      toggleColorPicker(name);
                    }}
                    aria-label="Change color for {name}"
                    title="Click to change color"
                    disabled={working || deleteWorking}
                  >
                    <span class="size-3 md:size-2.5 rounded-full {dotClass}"></span>
                  </button>

                  {#if isEditing}
                    <!-- svelte-ignore a11y_autofocus -->
                    <input
                      class="flex-1 min-w-0 bg-transparent border-b border-border focus:border-primary outline-none text-base md:text-sm py-0.5"
                      bind:value={editValue}
                      onkeydown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          confirmRename();
                        }
                        if (e.key === "Escape") {
                          e.preventDefault();
                          cancelEdit();
                        }
                      }}
                      autofocus
                      disabled={working}
                    />
                    <button
                      class="p-2.5 md:p-0.5 text-muted-foreground hover:text-primary transition-colors disabled:opacity-40 rounded"
                      onclick={confirmRename}
                      disabled={working}
                      aria-label="Save rename"
                    >
                      {#if working}
                        <Loader2 class="size-5 md:size-3.5 animate-spin" />
                      {:else}
                        <Check class="size-5 md:size-3.5" />
                      {/if}
                    </button>
                    <button
                      class="p-2.5 md:p-0.5 text-muted-foreground hover:text-foreground transition-colors rounded"
                      onclick={cancelEdit}
                      disabled={working}
                      aria-label="Cancel rename"
                    >
                      <X class="size-5 md:size-3.5" />
                    </button>
                  {:else}
                    <!-- Clickable row to select as brush -->
                    <button
                      type="button"
                      class="flex-1 text-left truncate min-h-[44px] md:min-h-0 flex items-center text-base md:text-sm {isPendingDelete
                        ? 'text-muted-foreground line-through'
                        : ''}"
                      onclick={() => selectBrush(name)}
                    >
                      {name}
                    </button>
                    <span
                      class="text-xs text-muted-foreground tabular-nums shrink-0"
                    >
                      {#if !entriesLoading}
                        {getEntryCount(name)}
                      {/if}
                    </span>
                    <div
                      class="flex items-center gap-1 md:gap-0.5 md:opacity-0 md:focus-within:opacity-100 md:[li:hover_&]:opacity-100 shrink-0"
                    >
                      <button
                        class="text-muted-foreground hover:text-foreground transition-colors p-2.5 md:p-0.5 rounded"
                        onclick={(e) => {
                          e.stopPropagation();
                          startEdit(name);
                        }}
                        disabled={working || deleteWorking || !!deleteCandidate}
                        aria-label="Rename {name}"
                      >
                        <Pencil class="size-4 md:size-3.5" />
                      </button>
                      <button
                        class="text-muted-foreground hover:text-destructive transition-colors p-2.5 md:p-0.5 rounded"
                        onclick={(e) => {
                          e.stopPropagation();
                          requestDelete(name);
                        }}
                        disabled={working || deleteWorking || !!deleteCandidate}
                        aria-label="Delete {name}"
                      >
                        {#if working && !deleteCandidate}
                          <Loader2 class="size-4 md:size-3.5 animate-spin" />
                        {:else}
                          <Trash2 class="size-4 md:size-3.5" />
                        {/if}
                      </button>
                    </div>
                  {/if}
                </div>

                <!-- Inline swatch picker -->
                {#if pickerOpen}
                  <div class="flex items-center gap-2 px-3 pb-2.5 pt-0">
                    <span class="text-[10px] text-muted-foreground mr-0.5"
                      >Color:</span
                    >
                    {#each AUDIENCE_PALETTE as swatch}
                      {@const isActiveSwatch = dotClass === swatch}
                      <button
                        class="size-8 md:size-6 rounded-full shrink-0 flex items-center justify-center transition-transform hover:scale-110
                          {isActiveSwatch
                          ? 'ring-2 ring-offset-1 ring-foreground/60 scale-110'
                          : ''}"
                        onclick={() => pickColor(name, swatch)}
                        aria-label="Set color {swatch}"
                        title={swatch}
                      >
                        <span class="size-4 md:size-3 rounded-full {swatch}"></span>
                      </button>
                    {/each}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}

        <!-- Delete warning banner -->
        {#if deleteCandidate}
          <div
            class="mx-3 my-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2.5 flex flex-col gap-2"
          >
            <div class="flex items-start gap-2 text-sm">
              <AlertTriangle
                class="size-4 text-destructive shrink-0 mt-0.5"
              />
              <p class="text-foreground leading-snug">
                <strong>"{deleteCandidate.name}"</strong> is used in
                <strong
                  >{deleteCandidate.count}
                  {deleteCandidate.count === 1
                    ? "entry"
                    : "entries"}</strong
                >. Deleting will remove this tag from all of them.
              </p>
            </div>
            <div class="flex items-center gap-2 justify-end">
              <Button
                variant="ghost"
                class="h-11 md:h-9"
                onclick={cancelDelete}
                disabled={deleteWorking}
              >
                Cancel
              </Button>
              <Button
                variant="destructive"
                class="h-11 md:h-9"
                onclick={confirmDelete}
                disabled={deleteWorking}
              >
                {#if deleteWorking}
                  <Loader2 class="size-3 animate-spin mr-1" />
                  Deleting...
                {:else}
                  Delete
                {/if}
              </Button>
            </div>
          </div>
        {/if}

        <!-- Mobile: Browse entries button -->
        {#if mobileState.isMobile && !audienceLoading}
          <div class="p-3 border-t">
            <Button
              variant="outline"
              class="w-full h-11 text-sm"
              onclick={() => (showDetail = true)}
            >
              Browse entries
            </Button>
          </div>
        {/if}
      </section>

      <!-- Right column: Entry tree (desktop) -->
      {#if !mobileState.isMobile}
        <aside class="min-h-0 overflow-auto flex flex-col">
          {@render treeContent()}
        </aside>
      {/if}
    </div>

    <!-- Mobile slide-over tree panel -->
    {#if mobileState.isMobile && showDetail}
      <div
        class="fixed inset-0 z-[60] bg-background animate-detail-in flex flex-col"
      >
        <header
          class="border-b px-4 py-3 flex items-center gap-3 pt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height)+0.75rem)] shrink-0"
        >
          <Button
            variant="ghost"
            size="icon"
            class="size-11"
            onclick={() => (showDetail = false)}
            aria-label="Back"
          >
            <ArrowLeft class="size-5" />
          </Button>
          <h2 class="text-lg font-semibold truncate">Entries</h2>
        </header>
        <!-- Mobile brush bar -->
        {#if audiences.length > 0}
          <div
            class="flex items-center gap-2 px-4 py-2 border-b overflow-x-auto shrink-0"
          >
            {#each audiences as name (name)}
              {@const dotClass = getAudienceColor(
                name,
                colorStore.audienceColors,
              )}
              {@const isActive = activeBrush === name}
              <button
                class="inline-flex items-center gap-1.5 shrink-0 px-2.5 py-1.5 rounded-full border text-xs transition-colors
                  {isActive
                  ? 'border-primary bg-accent font-medium'
                  : 'border-border hover:bg-muted/50'}"
                onclick={() => {
                  activeBrush = isActive ? null : name;
                }}
              >
                <span class="size-2 rounded-full {dotClass}"></span>
                {name}
              </button>
            {/each}
          </div>
        {/if}
        <div class="flex-1 overflow-auto flex flex-col">
          {@render treeContent()}
        </div>
      </div>
    {/if}
  </div>
</div>

{#snippet treeContent()}
  <div class="p-3 border-b shrink-0">
    {#if activeBrush && !mobileState.isMobile}
      <div class="flex items-center gap-2 mb-2">
        <span class="text-xs text-muted-foreground">Painting:</span>
        <span
          class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full border text-xs font-medium"
        >
          <span
            class="size-2 rounded-full {getAudienceColor(
              activeBrush,
              colorStore.audienceColors,
            )}"
          ></span>
          {activeBrush}
        </span>
        <button
          class="text-xs text-muted-foreground hover:text-foreground transition-colors underline"
          onclick={() => (activeBrush = null)}
        >
          Clear
        </button>
      </div>
    {/if}
    <div class="relative">
      <Search
        class="size-4 absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground"
      />
      <Input
        class="pl-8 h-11 md:h-9 text-base md:text-sm"
        placeholder="Filter entries..."
        bind:value={entrySearch}
      />
    </div>
  </div>

  {#if entriesLoading}
    <div
      class="flex-1 flex items-center justify-center text-muted-foreground gap-2 py-8"
    >
      <Loader2 class="size-4 animate-spin" />
      Loading entries...
    </div>
  {:else if !workspaceTree}
    <div class="p-4 text-sm text-muted-foreground">No entries found.</div>
  {:else}
    <div
      class="flex-1 overflow-auto pb-[calc(env(safe-area-inset-bottom)+1rem)]"
    >
      {@render entryTreeNode(workspaceTree, 0)}
    </div>
  {/if}
{/snippet}

{#snippet entryTreeNode(node: TreeNode, depth: number)}
  {@const q = entrySearch.trim().toLowerCase()}
  {@const matchesSearch = !q || nodeMatchesSearch(node, q)}
  {#if matchesSearch}
    {@const rawTags = entryAudienceMap.get(node.path)}
    {@const hasExplicit = rawTags !== null && rawTags !== undefined}
    {@const effectiveTags = getEffectiveTags(node.path)}
    {@const toggling = togglingPaths.has(node.path)}
    {@const hasChildren = node.children.length > 0}
    {@const isCollapsed = !q && collapsedFolders.has(node.path)}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- Entry row -->
    <div
      class="group/row flex items-center gap-1.5 pr-3 py-2.5 md:py-1.5 text-base md:text-sm transition-colors
        {toggling ? 'opacity-60' : ''}
        {activeBrush ? 'cursor-pointer hover:bg-muted/50' : 'hover:bg-muted/20'}"
      style="padding-left: {4 + depth * 16}px;"
      role={activeBrush ? "button" : undefined}
      tabindex={activeBrush ? 0 : undefined}
      onclick={() => {
        if (activeBrush) paintEntry(node.path);
      }}
      onkeydown={(e) => {
        if (activeBrush && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          paintEntry(node.path);
        }
      }}
    >
      {#if hasChildren}
        <button
          type="button"
          class="p-1 rounded-sm hover:bg-muted transition-colors shrink-0"
          onclick={(e) => {
            e.stopPropagation();
            toggleFolder(node.path);
          }}
          aria-label="{isCollapsed ? 'Expand' : 'Collapse'} folder"
          tabindex={-1}
        >
          <ChevronRight
            class="size-4 md:size-3.5 transition-transform duration-200 text-muted-foreground {isCollapsed
              ? ''
              : 'rotate-90'}"
          />
        </button>
      {:else}
        <span class="w-6 shrink-0"></span>
      {/if}
      {#if hasChildren && !isCollapsed}
        <FolderOpen
          class="size-4 md:size-3.5 text-muted-foreground shrink-0"
        />
      {:else if hasChildren}
        <FolderClosed
          class="size-4 md:size-3.5 text-muted-foreground shrink-0"
        />
      {:else}
        <FileText
          class="size-4 md:size-3.5 text-muted-foreground shrink-0"
        />
      {/if}
      <span class="flex-1 truncate min-w-0">
        {getTitle(node.path)}
      </span>
      <!-- Audience badges -->
      {#each effectiveTags as tag (tag.name)}
        {@const badgeColor = getAudienceColor(
          tag.name,
          colorStore.audienceColors,
        )}
        {#if tag.inherited}
          <span
            class="inline-flex items-center gap-1 text-[10px] leading-tight px-1.5 py-0.5 rounded-full border border-dashed border-border/60 text-muted-foreground shrink-0"
            title="{tag.name} (inherited)"
          >
            <span class="size-1.5 rounded-full shrink-0 {badgeColor}"></span>
            <span class="max-w-16 truncate">{tag.name}</span>
          </span>
        {:else}
          <button
            class="group/badge inline-flex items-center gap-1 text-[10px] leading-tight pl-1.5 pr-1 py-0.5 rounded-full border border-border shrink-0 hover:border-destructive/50 hover:bg-destructive/10 transition-colors"
            title="Remove {tag.name}"
            onclick={(e) => {
              e.stopPropagation();
              removeAudienceFromEntry(node.path, tag.name);
            }}
          >
            <span class="size-1.5 rounded-full shrink-0 {badgeColor}"></span>
            <span class="max-w-16 truncate">{tag.name}</span>
            <X
              class="size-2.5 shrink-0 opacity-0 group-hover/badge:opacity-100 -mr-0.5 transition-opacity"
            />
          </button>
        {/if}
      {/each}
      <!-- Inherit button (only for entries with explicit audience) -->
      {#if hasExplicit}
        <button
          class="p-1 rounded-sm text-muted-foreground hover:text-foreground hover:bg-muted transition-colors shrink-0
            md:opacity-0 md:group-hover/row:opacity-100 md:focus:opacity-100"
          onclick={(e) => {
            e.stopPropagation();
            setInherited(node.path);
          }}
          title="Inherit from parent"
          aria-label="Inherit from parent"
        >
          <RotateCcw class="size-3.5 md:size-3" />
        </button>
      {/if}
    </div>
    <!-- Children -->
    {#if hasChildren && !isCollapsed}
      {#each node.children as child (child.path)}
        {@render entryTreeNode(child, depth + 1)}
      {/each}
    {/if}
  {/if}
{/snippet}

<style>
  @keyframes marketplace-in {
    from {
      opacity: 0;
      transform: translateY(12px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  :global(.animate-marketplace-in) {
    animation: marketplace-in 0.25s ease-out;
  }

  @keyframes marketplace-out {
    from {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
    to {
      opacity: 0;
      transform: translateY(12px) scale(0.98);
    }
  }

  :global(.animate-marketplace-out) {
    animation: marketplace-out 0.2s ease-in forwards;
  }

  @keyframes detail-slide-in {
    from {
      transform: translateX(100%);
    }
    to {
      transform: translateX(0);
    }
  }

  :global(.animate-detail-in) {
    animation: detail-slide-in 0.2s ease-out;
  }
</style>
