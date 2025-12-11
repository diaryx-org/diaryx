<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    getBackend,
    startAutoPersist,
    stopAutoPersist,
    persistNow,
    type Backend,
    type TreeNode,
    type EntryData,
    type SearchResults,
  } from "./lib/backend";
  import LeftSidebar from "./lib/LeftSidebar.svelte";
  import RightSidebar from "./lib/RightSidebar.svelte";
  import NewEntryModal from "./lib/NewEntryModal.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Save, Download, PanelLeft, PanelRight, Menu } from "@lucide/svelte";

  // Dynamically import Editor to avoid SSR issues
  let Editor: typeof import("./lib/Editor.svelte").default | null =
    $state(null);

  // Backend instance
  let backend: Backend | null = $state(null);

  // State
  let tree: TreeNode | null = $state(null);
  let currentEntry: EntryData | null = $state(null);
  let isDirty = $state(false);
  let isLoading = $state(true);
  let error: string | null = $state(null);
  let searchQuery = $state("");
  let searchResults: SearchResults | null = $state(null);
  let isSearching = $state(false);
  let expandedNodes = $state(new Set<string>());
  let editorRef: any = $state(null);
  let showNewEntryModal = $state(false);

  // Sidebar states - collapsed by default on mobile
  let leftSidebarCollapsed = $state(true);
  let rightSidebarCollapsed = $state(true);

  // Check if we're on desktop and expand sidebars by default
  onMount(async () => {
    // Expand sidebars on desktop
    if (window.innerWidth >= 768) {
      leftSidebarCollapsed = false;
      rightSidebarCollapsed = false;
    }

    try {
      // Dynamically import the Editor component
      const module = await import("./lib/Editor.svelte");
      Editor = module.default;

      // Initialize the backend (auto-detects Tauri vs WASM)
      backend = await getBackend();

      // Start auto-persist for WASM backend (no-op for Tauri)
      startAutoPersist(5000);

      tree = await backend.getWorkspaceTree();

      // Expand root by default
      if (tree) {
        expandedNodes.add(tree.path);
      }
    } catch (e) {
      console.error("[App] Initialization error:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  });

  onDestroy(() => {
    // Stop auto-persist and do a final persist
    stopAutoPersist();
    persistNow();
  });

  // Open an entry
  async function openEntry(path: string) {
    if (!backend) return;

    if (isDirty) {
      const confirm = window.confirm(
        "You have unsaved changes. Do you want to discard them?",
      );
      if (!confirm) return;
    }

    try {
      isLoading = true;
      currentEntry = await backend.getEntry(path);
      console.log("[App] Loaded entry:", currentEntry);
      console.log("[App] Frontmatter:", currentEntry?.frontmatter);
      console.log(
        "[App] Frontmatter keys:",
        Object.keys(currentEntry?.frontmatter ?? {}),
      );
      isDirty = false;
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  }

  // Save current entry
  async function save() {
    if (!backend || !currentEntry || !editorRef) return;

    try {
      const markdown = editorRef.getMarkdown();
      await backend.saveEntry(currentEntry.path, markdown);
      isDirty = false;
      // Trigger persist for WASM backend
      await persistNow();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // Handle content changes
  function handleContentChange(_markdown: string) {
    isDirty = true;
  }

  // Search
  async function handleSearch() {
    if (!backend || !searchQuery.trim()) {
      searchResults = null;
      return;
    }

    try {
      isSearching = true;
      searchResults = await backend.searchWorkspace(searchQuery);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isSearching = false;
    }
  }

  function clearSearch() {
    searchQuery = "";
    searchResults = null;
  }

  // Toggle node expansion
  function toggleNode(path: string) {
    if (expandedNodes.has(path)) {
      expandedNodes.delete(path);
    } else {
      expandedNodes.add(path);
    }
    expandedNodes = new Set(expandedNodes); // Trigger reactivity
  }

  // Sidebar toggles
  function toggleLeftSidebar() {
    leftSidebarCollapsed = !leftSidebarCollapsed;
  }

  function toggleRightSidebar() {
    rightSidebarCollapsed = !rightSidebarCollapsed;
  }

  // Keyboard shortcuts
  function handleKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "s") {
      event.preventDefault();
      save();
    }
    // Toggle left sidebar with Cmd/Ctrl + B
    if ((event.metaKey || event.ctrlKey) && event.key === "b") {
      event.preventDefault();
      toggleLeftSidebar();
    }
    // Toggle right sidebar with Cmd/Ctrl + I (for Info)
    if (
      (event.metaKey || event.ctrlKey) &&
      event.shiftKey &&
      event.key === "I"
    ) {
      event.preventDefault();
      toggleRightSidebar();
    }
  }

  function handleNewEntry() {
    showNewEntryModal = true;
  }

  async function createNewEntry(path: string, title: string) {
    if (!backend) return;
    try {
      const newPath = await backend.createEntry(path, { title });
      // Backend automatically adds entry to parent index's contents
      tree = await backend.getWorkspaceTree(); // Refresh tree
      await openEntry(newPath);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      showNewEntryModal = false;
    }
  }

  function exportEntry() {
    if (!currentEntry) return;
    const blob = new Blob([currentEntry.content], {
      type: "text/markdown;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = currentEntry.path.split("/").pop() || "entry.md";
    a.click();
    URL.revokeObjectURL(url);
  }

  function getEntryTitle(entry: EntryData): string {
    return (
      entry.title ??
      entry.path.split("/").pop()?.replace(".md", "") ??
      "Untitled"
    );
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if showNewEntryModal}
  <NewEntryModal
    onSave={createNewEntry}
    onCancel={() => (showNewEntryModal = false)}
  />
{/if}

<div class="flex h-screen bg-background overflow-hidden">
  <!-- Left Sidebar -->
  <LeftSidebar
    {tree}
    {currentEntry}
    {isLoading}
    {error}
    bind:searchQuery
    {searchResults}
    {isSearching}
    {expandedNodes}
    collapsed={leftSidebarCollapsed}
    onOpenEntry={openEntry}
    onSearch={handleSearch}
    onClearSearch={clearSearch}
    onToggleNode={toggleNode}
    onNewEntry={handleNewEntry}
    onToggleCollapse={toggleLeftSidebar}
  />

  <!-- Main Content Area -->
  <main class="flex-1 flex flex-col overflow-hidden min-w-0">
    {#if currentEntry}
      <header
        class="flex items-center justify-between px-4 md:px-6 py-3 md:py-4 border-b border-border bg-card shrink-0"
      >
        <!-- Left side: toggle + title -->
        <div class="flex items-center gap-2 min-w-0 flex-1">
          <!-- Mobile menu button -->
          <Button
            variant="ghost"
            size="icon"
            onclick={toggleLeftSidebar}
            class="size-8 md:hidden shrink-0"
            aria-label="Toggle navigation"
          >
            <Menu class="size-4" />
          </Button>

          <!-- Desktop left sidebar toggle -->
          <Button
            variant="ghost"
            size="icon"
            onclick={toggleLeftSidebar}
            class="size-8 hidden md:flex shrink-0"
            aria-label="Toggle navigation sidebar"
          >
            <PanelLeft class="size-4" />
          </Button>

          <div class="min-w-0 flex-1">
            <h2
              class="text-lg md:text-xl font-semibold text-foreground truncate"
            >
              {getEntryTitle(currentEntry)}
            </h2>
            <p
              class="text-xs md:text-sm text-muted-foreground truncate hidden sm:block"
            >
              {currentEntry.path}
            </p>
          </div>
        </div>

        <!-- Right side: actions -->
        <div class="flex items-center gap-1 md:gap-2 ml-2 shrink-0">
          {#if isDirty}
            <span
              class="hidden sm:inline-flex px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
            >
              Unsaved
            </span>
          {/if}
          <Button
            onclick={save}
            disabled={!isDirty}
            size="sm"
            class="gap-1 md:gap-2"
          >
            <Save class="size-4" />
            <span class="hidden sm:inline">Save</span>
          </Button>
          <Button
            onclick={exportEntry}
            variant="outline"
            size="sm"
            class="gap-1 md:gap-2 hidden sm:flex"
          >
            <Download class="size-4" />
            <span class="hidden md:inline">Export</span>
          </Button>

          <!-- Properties panel toggle -->
          <Button
            variant="ghost"
            size="icon"
            onclick={toggleRightSidebar}
            class="size-8"
            aria-label="Toggle properties panel"
          >
            <PanelRight class="size-4" />
          </Button>
        </div>
      </header>

      <div class="flex-1 overflow-y-auto p-4 md:p-6">
        {#if Editor}
          <Editor
            bind:this={editorRef}
            content={currentEntry.content}
            onchange={handleContentChange}
            placeholder="Start writing..."
          />
        {:else}
          <div class="flex items-center justify-center h-full">
            <div
              class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"
            ></div>
          </div>
        {/if}
      </div>
    {:else}
      <!-- Empty state with sidebar toggles -->
      <header
        class="flex items-center justify-between px-4 py-3 border-b border-border bg-card shrink-0 md:hidden"
      >
        <Button
          variant="ghost"
          size="icon"
          onclick={toggleLeftSidebar}
          class="size-8"
          aria-label="Toggle navigation"
        >
          <Menu class="size-4" />
        </Button>
        <span class="text-lg font-semibold">Diaryx</span>
        <div class="size-8"></div>
      </header>

      <div class="flex-1 flex items-center justify-center">
        <div class="text-center max-w-md px-4">
          <!-- Desktop sidebar toggle when no entry -->
          <div class="hidden md:flex justify-center mb-4">
            {#if leftSidebarCollapsed}
              <Button
                variant="outline"
                size="sm"
                onclick={toggleLeftSidebar}
                class="gap-2"
              >
                <PanelLeft class="size-4" />
                Show Sidebar
              </Button>
            {/if}
          </div>
          <h2 class="text-2xl font-semibold text-foreground mb-2">
            Welcome to Diaryx
          </h2>
          <p class="text-muted-foreground">
            Select an entry from the sidebar to start editing, or create a new
            one.
          </p>
        </div>
      </div>
    {/if}
  </main>

  <!-- Right Sidebar (Properties) -->
  <RightSidebar
    entry={currentEntry}
    collapsed={rightSidebarCollapsed}
    onToggleCollapse={toggleRightSidebar}
  />
</div>
