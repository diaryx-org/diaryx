<script lang="ts">
  import * as Command from "$lib/components/ui/command";
  import type { Api } from "./backend/api";
  import { getMobileState } from "./hooks/useMobile.svelte";
  import { getPluginStore } from "../models/stores/pluginStore.svelte";
  import PluginSidebarPanel from "$lib/components/PluginSidebarPanel.svelte";
  import { getFavoritesStore } from "$lib/stores/favoritesStore.svelte";
  import type { CommandDefinition } from "$lib/commandRegistry";
  import {
    getFavoriteCommands,
    getGroupedCommands,
    shouldDismissPalette,
  } from "$lib/commandPalette";
  import {
    Star,
    GripVertical,
    ChevronUp,
    ChevronDown,
  } from "@lucide/svelte";

  interface Props {
    open: boolean;
    /** 0-1 during an interactive swipe-up gesture, null otherwise */
    swipeProgress?: number | null;
    api: Api | null;
    commandRegistry: Map<string, CommandDefinition>;
  }

  let {
    open = $bindable(),
    swipeProgress = null,
    api,
    commandRegistry,
  }: Props = $props();

  const pluginStore = getPluginStore();
  const commandPaletteOwner = $derived(pluginStore.commandPaletteOwner);
  const favoritesStore = getFavoritesStore();
  const mobileState = getMobileState();

  let searchValue = $state("");

  // ── Derived command lists ────────────────────────────────────────────

  const favoriteCommands = $derived(
    getFavoriteCommands(commandRegistry, favoritesStore.ids),
  );

  const groupedCommands = $derived(
    getGroupedCommands(commandRegistry),
  );

  // ── Command execution ────────────────────────────────────────────────

  function handleCommand(action: () => void | Promise<void>) {
    open = false;
    searchValue = "";
    action();
  }

  // ── Drag-and-drop reorder (desktop) ──────────────────────────────────

  let draggedId = $state<string | null>(null);
  let dragOverId = $state<string | null>(null);

  function handleDragStart(e: DragEvent, id: string) {
    draggedId = id;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", id);
    }
  }

  function handleDragOver(e: DragEvent, id: string) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dragOverId = id;
  }

  function handleDrop(e: DragEvent, targetId: string) {
    e.preventDefault();
    if (!draggedId || draggedId === targetId) {
      draggedId = null;
      dragOverId = null;
      return;
    }
    const fromIndex = favoritesStore.ids.indexOf(draggedId);
    const toIndex = favoritesStore.ids.indexOf(targetId);
    if (fromIndex >= 0 && toIndex >= 0) {
      favoritesStore.reorder(fromIndex, toIndex);
    }
    draggedId = null;
    dragOverId = null;
  }

  function handleDragEnd() {
    draggedId = null;
    dragOverId = null;
  }

  // ── Mobile reorder (up/down buttons) ─────────────────────────────────

  function moveUp(id: string) {
    const idx = favoritesStore.ids.indexOf(id);
    if (idx > 0) favoritesStore.reorder(idx, idx - 1);
  }

  function moveDown(id: string) {
    const idx = favoritesStore.ids.indexOf(id);
    if (idx >= 0 && idx < favoritesStore.ids.length - 1) {
      favoritesStore.reorder(idx, idx + 1);
    }
  }

  // ── Mobile sheet animation ───────────────────────────────────────────

  const swiping = $derived(swipeProgress != null && swipeProgress > 0);
  let closing = $state(false);
  let opening = $state(false);
  const showMobileSheet = $derived(open || swiping || closing);

  $effect(() => {
    if (open && mobileState.isMobile) {
      opening = true;
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          opening = false;
        });
      });
    }
  });

  function closeWithAnimation() {
    if (closing) return;
    closing = true;
    setTimeout(() => {
      open = false;
      searchValue = "";
      closing = false;
    }, 300);
  }

  // ── Drag-to-dismiss ──────────────────────────────────────────────────

  let dismissDragY = $state(0);
  let dismissDragging = $state(false);
  let dismissStartY = 0;

  function handleHandleTouchStart(e: TouchEvent) {
    if (!open || e.touches.length !== 1) return;
    dismissStartY = e.touches[0].clientY;
    dismissDragY = 0;
    dismissDragging = true;
  }

  function handleHandleTouchMove(e: TouchEvent) {
    if (!dismissDragging || e.touches.length !== 1) return;
    const delta = e.touches[0].clientY - dismissStartY;
    dismissDragY = Math.max(0, delta);
  }

  function handleHandleTouchEnd() {
    if (!dismissDragging) return;
    if (shouldDismissPalette(dismissDragY)) {
      closeWithAnimation();
    }
    dismissDragY = 0;
    dismissDragging = false;
  }
</script>

{#snippet commandContent()}
  {#if commandPaletteOwner && api}
    <div class="h-[60vh] max-h-[640px] overflow-hidden">
      <PluginSidebarPanel
        pluginId={commandPaletteOwner.pluginId}
        component={commandPaletteOwner.contribution.component}
        {api}
      />
    </div>
  {:else}
    <Command.Input
      placeholder="Search commands..."
      bind:value={searchValue}
    />
    <Command.List>
      <Command.Empty>No results found.</Command.Empty>

      <!-- Favorites group -->
      {#if favoriteCommands.length > 0}
        <Command.Group heading="Favorites">
          {#each favoriteCommands as cmd (cmd.id)}
            {@const CmdIcon = cmd.icon}
            <Command.Item
              onSelect={() => handleCommand(cmd.execute)}
              class="group relative {dragOverId === cmd.id ? 'border-t-2 border-primary' : ''}"
              draggable={!mobileState.isMobile}
              ondragstart={(e: DragEvent) => handleDragStart(e, cmd.id)}
              ondragover={(e: DragEvent) => handleDragOver(e, cmd.id)}
              ondrop={(e: DragEvent) => handleDrop(e, cmd.id)}
              ondragend={handleDragEnd}
            >
              {#if !mobileState.isMobile}
                <GripVertical class="mr-1 size-3 text-muted-foreground/50 cursor-grab" />
              {/if}
              <CmdIcon class="mr-2 size-4" />
              <span class="flex-1">{cmd.label}</span>
              {#if cmd.shortcut}
                <Command.Shortcut>{cmd.shortcut}</Command.Shortcut>
              {/if}
              {#if mobileState.isMobile}
                <button
                  type="button"
                  class="p-1 hover:bg-accent rounded"
                  onclick={(e: MouseEvent) => { e.stopPropagation(); moveUp(cmd.id); }}
                  aria-label="Move up"
                >
                  <ChevronUp class="size-3 text-muted-foreground" />
                </button>
                <button
                  type="button"
                  class="p-1 hover:bg-accent rounded"
                  onclick={(e: MouseEvent) => { e.stopPropagation(); moveDown(cmd.id); }}
                  aria-label="Move down"
                >
                  <ChevronDown class="size-3 text-muted-foreground" />
                </button>
              {/if}
              <button
                type="button"
                class="p-1 hover:bg-accent rounded"
                onclick={(e: MouseEvent) => { e.stopPropagation(); favoritesStore.removeFavorite(cmd.id); }}
                aria-label="Remove from favorites"
              >
                <Star class="size-3.5 fill-yellow-400 text-yellow-400" />
              </button>
            </Command.Item>
          {/each}
        </Command.Group>
      {/if}

      <!-- Dynamic groups -->
      {#each groupedCommands as group (group.key)}
        <Command.Group heading={group.label}>
          {#each group.commands as cmd (cmd.id)}
            {@const CmdIcon = cmd.icon}
            <Command.Item class="group" onSelect={() => handleCommand(cmd.execute)}>
              <CmdIcon class="mr-2 size-4" />
              <span class="flex-1">{cmd.label}</span>
              {#if cmd.shortcut}
                <Command.Shortcut>{cmd.shortcut}</Command.Shortcut>
              {/if}
              {#if cmd.favoritable}
                <button
                  type="button"
                  class="p-1 hover:bg-accent rounded opacity-0 group-hover:opacity-100 transition-opacity {mobileState.isMobile ? '!opacity-100' : ''}"
                  onclick={(e: MouseEvent) => { e.stopPropagation(); favoritesStore.toggleFavorite(cmd.id); }}
                  aria-label={favoritesStore.isFavorite(cmd.id) ? "Remove from favorites" : "Add to favorites"}
                >
                  {#if favoritesStore.isFavorite(cmd.id)}
                    <Star class="size-3.5 fill-yellow-400 text-yellow-400" />
                  {:else}
                    <Star class="size-3.5 text-muted-foreground" />
                  {/if}
                </button>
              {/if}
            </Command.Item>
          {/each}
        </Command.Group>
      {/each}
    </Command.List>
  {/if}
{/snippet}

{#if mobileState.isMobile}
  {#if showMobileSheet}
    <!-- Backdrop -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-50 {swiping ? 'pointer-events-none' : ''}"
      style="background: rgba(0,0,0,{closing || opening
        ? 0
        : open
          ? (dismissDragging ? Math.max(0, 0.5 - dismissDragY / 600) : 0.5)
          : (swipeProgress ?? 0) * 0.5});
             {!swiping && !dismissDragging ? 'transition: background 0.3s ease-in-out;' : ''}"
      onclick={closeWithAnimation}
      onkeydown={(e) => { if (e.key === 'Escape') closeWithAnimation(); }}
    ></div>

    <!-- Sheet -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-x-0 bottom-0 z-50 rounded-t-lg border-t bg-background max-h-[80vh] overflow-hidden {swiping ? 'pointer-events-none' : ''}"
      style="transform: translateY({closing || opening
        ? '100%'
        : open
          ? (dismissDragging ? dismissDragY + 'px' : '0')
          : (100 - (swipeProgress ?? 0) * 100) + '%'});
             {!swiping && !dismissDragging ? 'transition: transform 0.3s ease-in-out;' : ''}"
    >
      <!-- Drag handle (touch target for dismiss gesture) -->
      <div
        class="flex justify-center py-4 cursor-grab active:cursor-grabbing"
        ontouchstart={handleHandleTouchStart}
        ontouchmove={handleHandleTouchMove}
        ontouchend={handleHandleTouchEnd}
        ontouchcancel={handleHandleTouchEnd}
      >
        <div class="h-2 w-[100px] shrink-0 rounded-full bg-muted"></div>
      </div>

      <div class="mx-auto w-full max-w-md px-4 pb-[calc(env(safe-area-inset-bottom)+1rem)]">
        <Command.Root class="rounded-lg border-none shadow-none">
          {@render commandContent()}
        </Command.Root>
      </div>
    </div>
  {/if}
{:else}
  <!-- Desktop: Use Dialog -->
  <Command.Dialog bind:open title="Command Palette" description="Search or run a command">
    {@render commandContent()}
  </Command.Dialog>
{/if}
