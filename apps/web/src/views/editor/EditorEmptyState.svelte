<script lang="ts">
  /**
   * EditorEmptyState - Welcome screen shown when no entry is selected
   *
   * A pure presentational component that displays:
   * - Mobile header with menu toggle and command palette access
   * - Desktop sidebar toggle button (when collapsed)
   * - Welcome message
   */

  import { Button } from "$lib/components/ui/button";
  import { PanelLeft, Menu, Search, FolderPlus, Loader2, FolderSearch, Trash2 } from "@lucide/svelte";

  interface Props {
    leftSidebarCollapsed: boolean;
    onToggleLeftSidebar: () => void;
    onOpenCommandPalette: () => void;
    hasWorkspaceTree?: boolean;
    onInitializeWorkspace?: () => void;
    isLoading?: boolean;
    workspaceMissing?: { id: string; name: string } | null;
    onRelocateWorkspace?: () => void;
    onRemoveWorkspace?: () => void;
  }

  let {
    leftSidebarCollapsed,
    onToggleLeftSidebar,
    onOpenCommandPalette,
    hasWorkspaceTree = true,
    onInitializeWorkspace,
    isLoading = false,
    workspaceMissing = null,
    onRelocateWorkspace,
    onRemoveWorkspace,
  }: Props = $props();
</script>

<!-- Mobile header -->
<header
  class="flex items-center justify-between px-4 py-3 border-b border-border bg-card shrink-0 md:hidden select-none"
>
  <Button
    variant="ghost"
    size="icon"
    onclick={onToggleLeftSidebar}
    class="size-8"
    aria-label="Toggle navigation"
  >
    <Menu class="size-4" />
  </Button>
  <span class="text-lg font-semibold">Diaryx</span>
  <Button
    variant="ghost"
    size="icon"
    onclick={onOpenCommandPalette}
    class="size-8"
    aria-label="Open command palette"
  >
    <Search class="size-4" />
  </Button>
</header>

<!-- Welcome content -->
<div class="flex-1 flex items-center justify-center select-none">
  <div class="text-center max-w-md px-4">
    <!-- Desktop sidebar toggle when no entry -->
    <div class="hidden md:flex justify-center mb-4">
      {#if leftSidebarCollapsed}
        <Button
          variant="outline"
          size="sm"
          onclick={onToggleLeftSidebar}
          class="gap-2"
        >
          <PanelLeft class="size-4" />
          Show Sidebar
        </Button>
      {/if}
    </div>
    {#if workspaceMissing}
      <h2 class="text-2xl font-semibold text-foreground mb-2">
        Workspace not found
      </h2>
      <p class="text-muted-foreground mb-4">
        "{workspaceMissing.name}" could not be found. It may have been moved or deleted.
      </p>
      <div class="flex gap-2 justify-center">
        {#if onRelocateWorkspace}
          <Button variant="outline" onclick={onRelocateWorkspace} class="gap-2">
            <FolderSearch class="size-4" />
            Locate folder
          </Button>
        {/if}
        {#if onRemoveWorkspace}
          <Button variant="ghost" onclick={onRemoveWorkspace} class="gap-2 text-muted-foreground">
            <Trash2 class="size-4" />
            Remove workspace
          </Button>
        {/if}
      </div>
    {:else if isLoading}
      <Loader2 class="size-8 text-muted-foreground animate-spin mb-4" />
      <p class="text-muted-foreground">Loading workspace…</p>
    {:else if !hasWorkspaceTree && onInitializeWorkspace}
      <h2 class="text-2xl font-semibold text-foreground mb-2">
        Your workspace is empty
      </h2>
      <p class="text-muted-foreground mb-4">
        Initialize this workspace to choose how you want to set it up.
      </p>
      <Button onclick={onInitializeWorkspace} class="gap-2">
        <FolderPlus class="size-4" />
        Initialize workspace
      </Button>
    {:else}
      <h2 class="text-2xl font-semibold text-foreground mb-2">
        Welcome to Diaryx
      </h2>
      <p class="text-muted-foreground">
        Select an entry from the sidebar to start editing, or create a new one.
      </p>
    {/if}
  </div>
</div>
