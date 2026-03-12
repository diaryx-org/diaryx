<script lang="ts">
  /**
   * EditorFooter - Bottom bar for the editor with audience pill, save state, and actions
   */

  import { Button } from "$lib/components/ui/button";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import * as Kbd from "$lib/components/ui/kbd";
  import type { Api } from "$lib/backend/api";
  import PluginStatusItems from "$lib/components/PluginStatusItems.svelte";
  import DocumentAudiencePill from "$lib/components/DocumentAudiencePill.svelte";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import {
    Check,
    Circle,
    Loader2,
    Search,
    Sparkles,
    Plug,
  } from "@lucide/svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";

  const mobileState = getMobileState();

  interface Props {
    isDirty: boolean;
    isSaving: boolean;
    focusMode?: boolean;
    leftSidebarOpen: boolean;
    rightSidebarOpen: boolean;
    readonly?: boolean;
    commandPaletteOpen?: boolean;
    onSave: () => void | Promise<void>;
    onOpenCommandPalette: () => void;
    /** API wrapper for plugin status bar commands */
    api?: Api | null;
    /** Plugin toolbar button clicked */
    onPluginToolbarAction?: (pluginId: string, command: string) => void;
    /** Audience pill props */
    audience: string[] | null;
    entryPath: string;
    rootPath: string;
    onAudienceChange: (value: string[] | null) => void;
  }

  let {
    isDirty,
    isSaving,
    focusMode = false,
    leftSidebarOpen,
    rightSidebarOpen,
    readonly = false,
    commandPaletteOpen = false,
    onSave,
    onOpenCommandPalette,
    api = null,
    onPluginToolbarAction,
    audience,
    entryPath,
    rootPath,
    onAudienceChange,
  }: Props = $props();

  const pluginStore = getPluginStore();

  const iconMap: Record<string, typeof Sparkles> = {
    sparkles: Sparkles,
    plug: Plug,
  };

  let bothSidebarsClosed = $derived(!leftSidebarOpen && !rightSidebarOpen);
  let shouldFade = $derived(focusMode && bothSidebarsClosed);
  let isHovered = $state(false);
  let commandPaletteTooltipSuppressed = $state(false);

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

<!-- svelte-ignore a11y_no_static_element_interactions -->
<footer
  class="flex items-center justify-between px-4 md:px-6 py-2 border-t border-border bg-background
    transition-opacity duration-300 ease-in-out
    {shouldFade ? 'absolute inset-x-0 bottom-0 z-10' : 'shrink-0'}
    {shouldFade && !isHovered ? 'opacity-0' : 'opacity-100'}
    pb-[calc(env(safe-area-inset-bottom)+0.5rem)]"
  onmouseenter={() => isHovered = true}
  onmouseleave={() => isHovered = false}
>
  <!-- Left side: audience pill -->
  <div class="flex items-center gap-2 min-w-0 flex-1">
    <div class="min-w-0 flex-1">
      <DocumentAudiencePill
        {audience}
        {entryPath}
        {rootPath}
        {api}
        onChange={onAudienceChange}
      />
    </div>
  </div>

  <!-- Right side: actions -->
  <div class="flex items-center gap-1 md:gap-2 ml-2 shrink-0">
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
        {#if !mobileState.isMobile}
          <Tooltip.Content>
            {btn.contribution.label}
          </Tooltip.Content>
        {/if}
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
      {#if !mobileState.isMobile && !commandPaletteOpen && !commandPaletteTooltipSuppressed}
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
</footer>
