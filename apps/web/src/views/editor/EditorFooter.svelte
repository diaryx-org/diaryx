<script lang="ts">
  /**
   * EditorFooter - Bottom bar for the editor with save state, actions, and audience dots
   *
   * Mobile: renders a FAB (floating action button) that opens a Drawer bottom sheet.
   * Desktop: renders the traditional footer bar (unchanged).
   */

  import { Button } from "$lib/components/ui/button";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import * as Kbd from "$lib/components/ui/kbd";
  import * as Drawer from "$lib/components/ui/drawer";
  import * as Popover from "$lib/components/ui/popover";
  import type { Api } from "$lib/backend/api";
  // PluginStatusItems removed — sync indicator moved to LeftSidebar
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { maybeStartWindowDrag } from "$lib/windowDrag";
  import {
    PanelLeft,
    PanelRight,
    Search,
    ChevronUp,
    Ellipsis,
  } from "@lucide/svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor } from "$lib/utils/audienceDotColor";
  import { getFavoritesStore } from "$lib/stores/favoritesStore.svelte";
  import type { CommandDefinition } from "$lib/commandRegistry";

  const mobileState = getMobileState();

  interface Props {
    focusMode?: boolean;
    mobileFocusChromeVisible?: boolean;
    leftSidebarOpen: boolean;
    rightSidebarOpen: boolean;
    readonly?: boolean;
    commandPaletteOpen?: boolean;
    onOpenCommandPalette: () => void;
    onRevealMobileFocusChrome?: () => void;
    /** API wrapper for plugin status bar commands */
    api?: Api | null;
    /** Resolved effective audience tags for the current entry */
    audienceTags?: string[];
    onOpenAudienceManager?: () => void;
    /** Callback to expose the FAB element to the parent (for swipe gesture targeting) */
    onFabMount?: (el: HTMLElement | null) => void;
    /** Command registry for favorites toolbar */
    commandRegistry?: Map<string, CommandDefinition>;
    /** Whether the editor is loaded (insert commands need it) */
    hasEditor?: boolean;
    /** Whether to show the account button (when sidebar is collapsed) */
    showAccountButton?: boolean;
    /** Whether the user is authenticated */
    isAuthenticated?: boolean;
    /** Whether the server is offline */
    serverOffline?: boolean;
    /** Callback when account button is clicked */
    onOpenAccount?: () => void;
    /** Callback to open the left sidebar */
    onOpenLeftSidebar?: () => void;
    /** Callback to open the right sidebar */
    onOpenRightSidebar?: () => void;
  }

  let {
    focusMode = false,
    mobileFocusChromeVisible = false,
    leftSidebarOpen,
    rightSidebarOpen,
    readonly = false,
    commandPaletteOpen = false,
    onOpenCommandPalette,
    onRevealMobileFocusChrome,
    api: _api = null,
    audienceTags = [],
    onOpenAudienceManager,
    onFabMount,
    commandRegistry,
    hasEditor = false,
    showAccountButton: _showAccountButton = false,
    isAuthenticated: _isAuthenticated = false,
    serverOffline: _serverOffline = false,
    onOpenAccount: _onOpenAccount,
    onOpenLeftSidebar,
    onOpenRightSidebar,
  }: Props = $props();

  const colorStore = getAudienceColorStore();
  const favoritesStore = getFavoritesStore();

  const MAX_VISIBLE_FAVORITES = 5;

  const favoriteCommands = $derived(
    hasEditor && commandRegistry
      ? favoritesStore.ids
          .map((id) => commandRegistry.get(id))
          .filter((cmd): cmd is CommandDefinition => !!cmd && cmd.available())
      : [],
  );

  const visibleFavorites = $derived(
    favoriteCommands.slice(0, MAX_VISIBLE_FAVORITES),
  );

  const overflowFavorites = $derived(
    favoriteCommands.slice(MAX_VISIBLE_FAVORITES),
  );

  let overflowOpen = $state(false);

  let bothSidebarsClosed = $derived(!leftSidebarOpen && !rightSidebarOpen);
  let shouldFade = $derived(focusMode && bothSidebarsClosed);
  let mobileShouldFade = $derived(shouldFade && mobileState.isMobile);
  let desktopShouldFade = $derived(shouldFade && !mobileState.isMobile);
  let isHovered = $state(false);
  let footerVisible = $derived(
    mobileShouldFade ? mobileFocusChromeVisible : desktopShouldFade ? isHovered : true,
  );
  let commandPaletteTooltipSuppressed = $state(false);

  // Mobile FAB state
  let fabDrawerOpen = $state(false);
  let fabRef: HTMLElement | null = $state(null);

  $effect(() => { onFabMount?.(fabRef); });

  const isMac =
    typeof navigator !== "undefined" &&
    navigator.platform.toUpperCase().indexOf("MAC") >= 0;
  const modKey = isMac ? "⌘" : "Ctrl";

  function blurEventTarget(target: EventTarget | null): void {
    if (target instanceof HTMLElement) {
      target.blur();
    }
  }

  function handleOpenCommandPaletteClick(): void {
    commandPaletteTooltipSuppressed = true;
    onOpenCommandPalette();
  }

  function handleRevealMobileFocusChrome(): void {
    onRevealMobileFocusChrome?.();
  }

  function handleDrawerAction(action: () => void): void {
    action();
    fabDrawerOpen = false;
  }

  $effect(() => {
    if (commandPaletteOpen) {
      commandPaletteTooltipSuppressed = true;
    } else if (commandPaletteTooltipSuppressed) {
      const timeout = setTimeout(() => {
        commandPaletteTooltipSuppressed = false;
      }, 400);
      return () => clearTimeout(timeout);
    }
  });
</script>

{#if mobileState.isMobile}
  <!-- ===== Mobile: FAB + Drawer ===== -->

  <!-- FAB button -->
  {#if !mobileState.keyboardVisible}
    <button
      bind:this={fabRef}
      type="button"
      class="fixed z-30 right-4 bottom-[calc(env(safe-area-inset-bottom)+1rem)]
        size-14 rounded-full shadow-lg bg-primary text-primary-foreground
        flex items-center justify-center
        transition-opacity duration-300
        {mobileShouldFade && !mobileFocusChromeVisible ? 'opacity-40' : 'opacity-100'}
        active:scale-95"
      aria-label="Open editor actions"
      onclick={() => { fabDrawerOpen = true; }}
      onpointerdown={handleRevealMobileFocusChrome}
    >
      <ChevronUp class="size-6" />
    </button>
  {/if}

  <!-- Drawer bottom sheet -->
  <Drawer.Root bind:open={fabDrawerOpen}>
    <Drawer.Content>
      <div class="mx-auto w-full max-w-sm select-none">
        <Drawer.Header>
          <Drawer.Title>Actions</Drawer.Title>
        </Drawer.Header>

        <div class="flex flex-col pb-4">
          <!-- Favorites -->
          {#each favoriteCommands as cmd (cmd.id)}
            {@const FavIcon = cmd.icon}
            <button
              type="button"
              class="flex items-center gap-4 px-6 py-4 hover:bg-muted active:bg-muted/80 transition-colors text-left"
              onclick={() => handleDrawerAction(cmd.execute)}
            >
              <FavIcon class="size-5 text-muted-foreground" />
              <span class="text-base">{cmd.label}</span>
            </button>
          {/each}

          <!-- Search Commands -->
          <button
            type="button"
            class="flex items-center gap-4 px-6 py-4 hover:bg-muted active:bg-muted/80 transition-colors text-left"
            onclick={() => handleDrawerAction(onOpenCommandPalette)}
          >
            <Search class="size-5 text-muted-foreground" />
            <span class="text-base">Search Commands</span>
          </button>
        </div>
      </div>
    </Drawer.Content>
  </Drawer.Root>

{:else}
  <!-- ===== Desktop: traditional footer bar ===== -->

  {#if desktopShouldFade}
    <!-- Invisible hit area at bottom edge to reveal footer on hover (desktop only) -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="absolute inset-x-0 bottom-0 z-10 h-6"
      onmouseenter={() => isHovered = true}
    ></div>
  {/if}
  <footer
    class="flex items-center justify-between py-1.5 border-t border-sidebar-border bg-sidebar-accent select-none
      {!leftSidebarOpen ? 'pl-2' : 'pl-4 md:pl-6'}
      {!rightSidebarOpen ? 'pr-2' : 'pr-4 md:pr-6'}
      transition-[opacity,transform] duration-300 ease-in-out
      {desktopShouldFade ? 'absolute inset-x-0 bottom-0 z-20' : 'relative shrink-0'}
      {footerVisible ? 'opacity-100' : 'opacity-0 pointer-events-none'}
      {footerVisible ? 'translate-y-0' : 'translate-y-full'}
      pb-[calc(env(safe-area-inset-bottom)+0.5rem)]"
    onmouseenter={() => isHovered = true}
    onmouseleave={() => isHovered = false}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="flex items-center justify-between w-full" onmousedown={maybeStartWindowDrag}>
      <!-- Left side: open sidebar button + audience dots -->
      <div class="flex items-center gap-2 min-w-0 flex-1">
        {#if !leftSidebarOpen && onOpenLeftSidebar}
          <Tooltip.Root>
            <Tooltip.Trigger>
              <button
                type="button"
                class="p-1 rounded hover:bg-accent transition-colors shrink-0"
                onclick={onOpenLeftSidebar}
                aria-label="Open sidebar"
              >
                <PanelLeft class="size-4 text-muted-foreground" />
              </button>
            </Tooltip.Trigger>
            <Tooltip.Content>Open sidebar</Tooltip.Content>
          </Tooltip.Root>
        {/if}
        {#if audienceTags.length > 0 && onOpenAudienceManager}
          <Tooltip.Root>
            <Tooltip.Trigger>
              <button
                type="button"
                class="flex items-center gap-1 shrink-0 p-1 rounded hover:bg-accent transition-colors"
                onclick={onOpenAudienceManager}
                aria-label="Manage audiences"
              >
                {#each audienceTags as tag}
                  <span
                    class="size-2 rounded-full {getAudienceColor(tag, colorStore.audienceColors)}"
                    title={tag}
                  ></span>
                {/each}
              </button>
            </Tooltip.Trigger>
            <Tooltip.Content>{audienceTags.join(", ")}</Tooltip.Content>
          </Tooltip.Root>
        {/if}
      </div>

      <!-- Right side: actions -->
      <div class="flex items-center gap-2 ml-2 shrink-0">
        {#if readonly}
          <span
            class="inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
          >
            View only
          </span>
        {/if}

        <!-- Favorite command buttons -->
        {#each visibleFavorites as cmd (cmd.id)}
          {@const FavIcon = cmd.icon}
          <Tooltip.Root>
            <Tooltip.Trigger>
              <Button
                variant="ghost"
                size="icon"
                onclick={(event: MouseEvent) => { blurEventTarget(event.currentTarget); cmd.execute(); }}
                class="size-8"
                aria-label={cmd.label}
              >
                <FavIcon class="size-4" />
              </Button>
            </Tooltip.Trigger>
            <Tooltip.Content>{cmd.label}</Tooltip.Content>
          </Tooltip.Root>
        {/each}

        <!-- Overflow favorites popover -->
        {#if overflowFavorites.length > 0}
          <Popover.Root bind:open={overflowOpen}>
            <Popover.Trigger>
              <Tooltip.Root>
                <Tooltip.Trigger>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="size-8"
                    aria-label="More favorites"
                  >
                    <Ellipsis class="size-4" />
                  </Button>
                </Tooltip.Trigger>
                {#if !overflowOpen}
                  <Tooltip.Content>More favorites</Tooltip.Content>
                {/if}
              </Tooltip.Root>
            </Popover.Trigger>
            <Popover.Content side="top" class="w-48 p-1">
              {#each overflowFavorites as cmd (cmd.id)}
                {@const OverflowIcon = cmd.icon}
                <button
                  type="button"
                  class="flex w-full items-center gap-2 rounded px-2 py-1.5 text-sm hover:bg-accent transition-colors text-left"
                  onclick={() => { overflowOpen = false; cmd.execute(); }}
                >
                  <OverflowIcon class="size-4 text-muted-foreground" />
                  <span>{cmd.label}</span>
                </button>
              {/each}
            </Popover.Content>
          </Popover.Root>
        {/if}

        <!-- Command palette button -->
        <Tooltip.Root>
          <Tooltip.Trigger>
            <Button
              variant="ghost"
              size="icon"
              onclick={handleOpenCommandPaletteClick}
              class="size-8"
              aria-label="Open command palette"
              data-spotlight="command-palette-button"
            >
              <Search class="size-4" />
            </Button>
          </Tooltip.Trigger>
          {#if !commandPaletteOpen && !commandPaletteTooltipSuppressed}
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

        {#if !rightSidebarOpen && onOpenRightSidebar}
          <Tooltip.Root>
            <Tooltip.Trigger>
              <Button
                variant="ghost"
                size="icon"
                onclick={onOpenRightSidebar}
                class="size-8"
                aria-label="Open panel"
              >
                <PanelRight class="size-4" />
              </Button>
            </Tooltip.Trigger>
            <Tooltip.Content>Open panel</Tooltip.Content>
          </Tooltip.Root>
        {/if}
      </div>
    </div>
  </footer>
{/if}
