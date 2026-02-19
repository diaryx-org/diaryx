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
  import { PanelLeft, Menu, Search, FolderPlus, Upload } from "@lucide/svelte";

  interface Props {
    leftSidebarCollapsed: boolean;
    onToggleLeftSidebar: () => void;
    onOpenCommandPalette: () => void;
    hasWorkspaceTree?: boolean;
    onCreateRootIndex?: () => void;
    onImportFromZip?: () => void;
  }

  let {
    leftSidebarCollapsed,
    onToggleLeftSidebar,
    onOpenCommandPalette,
    hasWorkspaceTree = true,
    onCreateRootIndex,
    onImportFromZip,
  }: Props = $props();
</script>

<!-- Mobile header -->
<header
  class="flex items-center justify-between px-4 py-3 border-b border-border bg-card shrink-0 md:hidden"
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
<div class="flex-1 flex items-center justify-center">
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
    {#if !hasWorkspaceTree && onCreateRootIndex}
      <h2 class="text-2xl font-semibold text-foreground mb-2">
        Your workspace is empty
      </h2>
      <p class="text-muted-foreground mb-4">
        A root index organizes your workspace into a navigable hierarchy.
      </p>
      <div class="flex flex-col sm:flex-row gap-2 justify-center">
        <Button onclick={onCreateRootIndex} class="gap-2">
          <FolderPlus class="size-4" />
          Create Root Index
        </Button>
        {#if onImportFromZip}
          <Button variant="outline" onclick={onImportFromZip} class="gap-2">
            <Upload class="size-4" />
            Import from ZIP
          </Button>
        {/if}
      </div>
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
