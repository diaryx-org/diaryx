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
  import PluginStatusItems from "$lib/components/PluginStatusItems.svelte";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { maybeStartWindowDrag } from "$lib/windowDrag";
  import {
    Check,
    Circle,
    Loader2,
    Search,
    ChevronUp,
    Sparkles,
    Plug,
    Save,
    Ellipsis,
  } from "@lucide/svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor } from "$lib/utils/audienceDotColor";
  import { getFavoritesStore } from "$lib/stores/favoritesStore.svelte";
  import type { CommandDefinition } from "$lib/commandRegistry";

  const mobileState = getMobileState();

  interface Props {
    isDirty: boolean;
    isSaving: boolean;
    focusMode?: boolean;
    mobileFocusChromeVisible?: boolean;
    leftSidebarOpen: boolean;
    rightSidebarOpen: boolean;
    readonly?: boolean;
    commandPaletteOpen?: boolean;
    onSave: () => void | Promise<void>;
    onOpenCommandPalette: () => void;
    onRevealMobileFocusChrome?: () => void;
    /** API wrapper for plugin status bar commands */
    api?: Api | null;
    /** Plugin toolbar button clicked */
    onPluginToolbarAction?: (pluginId: string, command: string) => void;
    /** Resolved effective audience tags for the current entry */
    audienceTags?: string[];
    onOpenAudienceManager?: () => void;
    /** Callback to expose the FAB element to the parent (for swipe gesture targeting) */
    onFabMount?: (el: HTMLElement | null) => void;
    /** Command registry for favorites toolbar */
    commandRegistry?: Map<string, CommandDefinition>;
    /** Whether the editor is loaded (insert commands need it) */
    hasEditor?: boolean;
  }

  let {
    isDirty,
    isSaving,
    focusMode = false,
    mobileFocusChromeVisible = false,
    leftSidebarOpen,
    rightSidebarOpen,
    readonly = false,
    commandPaletteOpen = false,
    onSave,
    onOpenCommandPalette,
    onRevealMobileFocusChrome,
    api = null,
    onPluginToolbarAction,
    audienceTags = [],
    onOpenAudienceManager,
    onFabMount,
    commandRegistry,
    hasEditor = false,
  }: Props = $props();

  const pluginStore = getPluginStore();
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

  const iconMap: Record<string, typeof Sparkles> = {
    sparkles: Sparkles,
    plug: Plug,
  };

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

  function handleSaveClick(event: MouseEvent): void {
    blurEventTarget(event.currentTarget);
    onSave();
  }

  function handlePluginToolbarClick(
    event: MouseEvent,
    pluginId: string,
    command: string,
  ): void {
    blurEventTarget(event.currentTarget);
    onPluginToolbarAction?.(pluginId, command);
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
      <!-- Save state badge -->
      {#if !readonly}
        <span class="absolute top-0.5 right-0.5 flex items-center justify-center size-4 rounded-full
          {isSaving ? 'bg-muted-foreground' : isDirty ? 'bg-amber-500 dark:bg-amber-400' : 'bg-emerald-500 dark:bg-emerald-400'}">
          {#if isSaving}
            <Loader2 class="size-2.5 animate-spin text-white" />
          {:else if isDirty}
            <Circle class="size-2 fill-white text-white" />
          {:else}
            <Check class="size-2.5 text-white" />
          {/if}
        </span>
      {/if}
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
          <!-- Save -->
          {#if !readonly}
            <button
              type="button"
              class="flex items-center gap-4 px-6 py-4 hover:bg-muted active:bg-muted/80 transition-colors text-left
                {!isDirty && !isSaving ? 'text-muted-foreground' : ''}"
              disabled={!isDirty && !isSaving}
              onclick={() => handleDrawerAction(onSave)}
            >
              {#if isSaving}
                <Loader2 class="size-5 animate-spin text-muted-foreground" />
                <span class="text-base">Saving...</span>
              {:else if isDirty}
                <Save class="size-5 text-muted-foreground" />
                <span class="text-base">Save</span>
              {:else}
                <Check class="size-5 text-muted-foreground/50" />
                <span class="text-base">Saved</span>
              {/if}
            </button>
          {/if}

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

          <!-- Plugin toolbar buttons -->
          {#each pluginStore.toolbarButtons as btn}
            {@const BtnIcon = iconMap[btn.contribution.icon ?? ""] ?? Plug}
            <button
              type="button"
              class="flex items-center gap-4 px-6 py-4 hover:bg-muted active:bg-muted/80 transition-colors text-left"
              onclick={() => handleDrawerAction(() => onPluginToolbarAction?.(btn.pluginId as unknown as string, btn.contribution.plugin_command))}
            >
              <BtnIcon class="size-5 text-muted-foreground" />
              <span class="text-base">{btn.contribution.label}</span>
            </button>
          {/each}
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
    class="flex items-center justify-between px-4 md:px-6 py-2 border-t border-sidebar-border bg-sidebar-accent select-none
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
      <!-- Left side: audience dots -->
      <div class="flex items-center gap-2 min-w-0 flex-1">
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
        {#if api}
          <PluginStatusItems {api} />
        {/if}

        {#if readonly}
          <span
            class="inline-flex items-center gap-1 px-2 py-1 text-xs font-medium rounded-md bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400"
          >
            View only
          </span>
        {:else}
          <!-- Save indicator -->
          <Tooltip.Root>
            <Tooltip.Trigger>
              <Button
                onclick={handleSaveClick}
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
          </Tooltip.Root>
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

        <!-- Plugin toolbar buttons -->
        {#each pluginStore.toolbarButtons as btn}
          {@const BtnIcon = iconMap[btn.contribution.icon ?? ""] ?? Plug}
          <Tooltip.Root>
            <Tooltip.Trigger>
              <Button
                variant="ghost"
                size="icon"
                onclick={(event) =>
                  handlePluginToolbarClick(
                    event,
                    btn.pluginId as unknown as string,
                    btn.contribution.plugin_command
                  )}
                class="size-8"
                aria-label={btn.contribution.label}
              >
                <BtnIcon class="size-4" />
              </Button>
            </Tooltip.Trigger>
            <Tooltip.Content>
              {btn.contribution.label}
            </Tooltip.Content>
          </Tooltip.Root>
        {/each}

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
      </div>
    </div>
  </footer>
{/if}
