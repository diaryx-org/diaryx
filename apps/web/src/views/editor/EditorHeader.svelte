<script lang="ts">
  /**
   * EditorHeader - Header bar for the editor with actions and sidebar toggles
   *
   * A pure presentational component that displays:
   * - Sidebar toggle buttons (only when sidebar is closed)
   * - Entry title and path (configurable via settings)
   * - Save state indicator (icon-only: check=saved, dot=unsaved, spinner=saving)
   * - Command palette button with keyboard shortcut tooltip
   */

  import { Button } from "$lib/components/ui/button";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import * as Kbd from "$lib/components/ui/kbd";
  import type { Api } from "$lib/backend/api";
  import PluginStatusItems from "$lib/components/PluginStatusItems.svelte";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import {
    Check,
    Circle,
    PanelLeft,
    PanelRight,
    Menu,
    Loader2,
    Search,
    Sparkles,
    Plug,
  } from "@lucide/svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";


  // Mobile state for hiding keyboard shortcut tooltips
  const mobileState = getMobileState();

  interface Props {
    title: string;
    path: string;
    isDirty: boolean;
    isSaving: boolean;
    showTitle: boolean;
    showPath: boolean;
    leftSidebarOpen: boolean;
    rightSidebarOpen: boolean;
    focusMode?: boolean;
    readonly?: boolean;
    onSave: () => void;
    onToggleLeftSidebar: () => void;
    onToggleRightSidebar: () => void;
    onOpenCommandPalette: () => void;
    /** API wrapper for plugin status bar commands */
    api?: Api | null;
    /** Plugin toolbar button clicked */
    onPluginToolbarAction?: (pluginId: string, command: string) => void;
  }

  let {
    title,
    path,
    isDirty,
    isSaving,
    showTitle,
    showPath,
    leftSidebarOpen,
    rightSidebarOpen,
    focusMode = false,
    readonly = false,
    onSave,
    onToggleLeftSidebar,
    onToggleRightSidebar,
    onOpenCommandPalette,
    api = null,
    onPluginToolbarAction,
  }: Props = $props();

  const pluginStore = getPluginStore();

  // Map icon names to Lucide components
  const iconMap: Record<string, typeof Sparkles> = {
    sparkles: Sparkles,
    plug: Plug,
  };

  // Focus mode: header is invisible when both sidebars are closed
  let bothSidebarsClosed = $derived(!leftSidebarOpen && !rightSidebarOpen);
  let shouldFade = $derived(focusMode && bothSidebarsClosed);
  let isHovered = $state(false);

  // Detect platform for keyboard shortcut display
  const isMac =
    typeof navigator !== "undefined" &&
    navigator.platform.toUpperCase().indexOf("MAC") >= 0;
  const modKey = isMac ? "⌘" : "Ctrl";
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header
  class="flex items-center justify-between px-4 md:px-6 py-3 md:py-4 border-b border-border bg-background
    transition-opacity duration-300 ease-in-out
    {shouldFade ? 'absolute inset-x-0 top-0 z-10' : 'shrink-0'}
    {shouldFade && !isHovered ? 'opacity-0' : 'opacity-100'}"
  onmouseenter={() => isHovered = true}
  onmouseleave={() => isHovered = false}
>
  <!-- Left side: toggle + title -->
  <div class="flex items-center gap-2 min-w-0 flex-1">
    <!-- Mobile menu button (always show on mobile for navigation) -->
    <Button
      variant="ghost"
      size="icon"
      onclick={onToggleLeftSidebar}
      class="size-8 md:hidden shrink-0"
      aria-label="Toggle navigation"
    >
      <Menu class="size-4" />
    </Button>

    <!-- Desktop left sidebar toggle (only when sidebar is closed) -->
    {#if !leftSidebarOpen}
      <Tooltip.Root>
        <Tooltip.Trigger>
          <Button
            variant="ghost"
            size="icon"
            onclick={onToggleLeftSidebar}
            class="size-8 hidden md:flex shrink-0"
            aria-label="Open navigation sidebar"
          >
            <PanelLeft class="size-4" />
          </Button>
        </Tooltip.Trigger>
        {#if !mobileState.isMobile}
          <Tooltip.Content>
            <div class="flex items-center gap-2">
              Open sidebar
              <Kbd.Group>
                <Kbd.Root>{modKey}</Kbd.Root>
                <span>+</span>
                <Kbd.Root>[</Kbd.Root>
              </Kbd.Group>
            </div>
          </Tooltip.Content>
        {/if}
      </Tooltip.Root>
    {/if}

    <!-- Title and path area -->
    {#if showTitle || showPath}
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-1">
          {#if showTitle}
            <h2 class="text-lg md:text-xl font-semibold text-foreground truncate">
              {title}
            </h2>
          {/if}
        </div>
        {#if showPath}
          <p
            class="text-xs md:text-sm text-muted-foreground truncate hidden sm:block"
          >
            {path}
          </p>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Right side: actions -->
  <div class="flex items-center gap-1 md:gap-2 ml-2 shrink-0">
    {#if api}
      <PluginStatusItems {api} />
    {/if}

    {#if readonly}
      <!-- View-only indicator for read-only mode -->
      <span
        class="inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
      >
        View only
      </span>
    {:else}
      <!-- Save indicator (icon-only) -->
      <Tooltip.Root>
        <Tooltip.Trigger>
          <Button
            onclick={onSave}
            disabled={!isDirty || isSaving}
            variant="ghost"
            size="icon"
            class="size-8"
            aria-label={isSaving ? "Saving" : isDirty ? "Save" : "Saved"}
          >
            {#if isSaving}
              <Loader2 class="size-4 animate-spin text-muted-foreground" />
            {:else if isDirty}
              <Circle class="size-3 fill-amber-500 text-amber-500 dark:fill-amber-400 dark:text-amber-400" />
            {:else}
              <Check class="size-4 text-muted-foreground/50" />
            {/if}
          </Button>
        </Tooltip.Trigger>
        {#if !mobileState.isMobile}
          <Tooltip.Content>
            {#if isSaving}
              Saving...
            {:else if isDirty}
              <div class="flex items-center gap-2">
                Save
                <Kbd.Group>
                  <Kbd.Root>{modKey}</Kbd.Root>
                  <span>+</span>
                  <Kbd.Root>S</Kbd.Root>
                </Kbd.Group>
              </div>
            {:else}
              Saved
            {/if}
          </Tooltip.Content>
        {/if}
      </Tooltip.Root>
    {/if}

    <!-- Plugin toolbar buttons -->
    {#each pluginStore.toolbarButtons as btn}
      {@const BtnIcon = iconMap[btn.contribution.icon ?? ""] ?? Plug}
      <Tooltip.Root>
        <Tooltip.Trigger>
          <Button
            variant="ghost"
            size="icon"
            onclick={() => onPluginToolbarAction?.(btn.pluginId as unknown as string, btn.contribution.plugin_command)}
            class="size-8"
            aria-label={btn.contribution.label}
          >
            <BtnIcon class="size-4" />
          </Button>
        </Tooltip.Trigger>
        {#if !mobileState.isMobile}
          <Tooltip.Content>
            {btn.contribution.label}
          </Tooltip.Content>
        {/if}
      </Tooltip.Root>
    {/each}

    <!-- Command palette button with tooltip -->
    <Tooltip.Root>
      <Tooltip.Trigger>
        <Button
          variant="ghost"
          size="icon"
          onclick={onOpenCommandPalette}
          class="size-8"
          aria-label="Open command palette"
        >
          <Search class="size-4" />
        </Button>
      </Tooltip.Trigger>
      {#if !mobileState.isMobile}
        <Tooltip.Content>
          <div class="flex items-center gap-2">
            Search
            <Kbd.Group>
              <Kbd.Root>{modKey}</Kbd.Root>
              <span>+</span>
              <Kbd.Root>K</Kbd.Root>
            </Kbd.Group>
          </div>
        </Tooltip.Content>
      {/if}
    </Tooltip.Root>

    <!-- Right sidebar toggle (only when sidebar is closed) -->
    {#if !rightSidebarOpen}
      <Tooltip.Root>
        <Tooltip.Trigger>
          <Button
            variant="ghost"
            size="icon"
            onclick={onToggleRightSidebar}
            class="size-8"
            aria-label="Open properties panel"
          >
            <PanelRight class="size-4" />
          </Button>
        </Tooltip.Trigger>
        {#if !mobileState.isMobile}
          <Tooltip.Content>
            <div class="flex items-center gap-2">
              Open properties
              <Kbd.Group>
                <Kbd.Root>{modKey}</Kbd.Root>
                <span>+</span>
                <Kbd.Root>]</Kbd.Root>
              </Kbd.Group>
            </div>
          </Tooltip.Content>
        {/if}
      </Tooltip.Root>
    {/if}
  </div>
</header>
