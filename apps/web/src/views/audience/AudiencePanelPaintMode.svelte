<script lang="ts">
  /**
   * AudiencePanelPaintMode — Brush selector for painting audiences onto
   * sidebar entries and editor text selections.
   *
   * User selects a brush (audience or "clear"), then:
   * - Clicking entries in LeftSidebar assigns/removes the audience
   * - Select text in editor, then click "Apply to selection" button
   */
  import { getAudiencePanelStore, CLEAR_BRUSH } from "$lib/stores/audiencePanelStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";
  import { getAudienceColor, AUDIENCE_PALETTE } from "$lib/utils/audienceDotColor";
  import { Check, Eraser, Loader2, Paintbrush, Pencil, Plus, X } from "@lucide/svelte";
  import { toast } from "svelte-sonner";
  import { tick } from "svelte";
  import * as ContextMenu from "$lib/components/ui/context-menu";
  import type { Api } from "$lib/backend";
  import type { TreeNode } from "$lib/backend/interface";

  interface Props {
    audiences: string[];
    api: Api | null;
    rootPath: string;
  }

  let { audiences, api, rootPath }: Props = $props();

  const panelStore = getAudiencePanelStore();
  const colorStore = getAudienceColorStore();
  const templateContextStore = getTemplateContextStore();

  let isAdding = $state(false);
  let addValue = $state("");
  let addInputEl = $state<HTMLInputElement | null>(null);

  // Inline rename state — entered via the "Rename" item in the row's context menu.
  let editingAudience = $state<string | null>(null);
  let editValue = $state("");
  let editInputEl = $state<HTMLInputElement | null>(null);
  let renaming = $state(false);

  // Merge the active transient brush into the rendered list so it shows up as
  // a paintable swatch even though it isn't yet in any file's frontmatter.
  let displayedAudiences = $derived.by(() => {
    const t = panelStore.transientAudience;
    if (t && !audiences.includes(t)) return [...audiences, t];
    return audiences;
  });

  function selectBrush(name: string) {
    if (editingAudience === name) return;
    if (editingAudience !== null) cancelEdit();
    panelStore.toggleBrush(name);
  }

  function selectClearBrush() {
    if (editingAudience !== null) cancelEdit();
    panelStore.toggleBrush(CLEAR_BRUSH);
  }

  let audienceBrushes = $derived(
    panelStore.paintBrushes.filter((b) => b !== CLEAR_BRUSH),
  );
  let clearBrushActive = $derived(
    panelStore.paintBrushes.length === 1 && panelStore.paintBrushes[0] === CLEAR_BRUSH,
  );

  function handleApplyToSelection() {
    const applied = panelStore.applyBrushToSelection();
    if (!applied) {
      toast.info("Select some text in the editor first");
    }
  }

  async function startAdd() {
    isAdding = true;
    addValue = "";
    await tick();
    addInputEl?.focus();
  }

  function cancelAdd() {
    isAdding = false;
    addValue = "";
  }

  function confirmAdd() {
    const name = addValue.trim();
    if (!name) {
      cancelAdd();
      return;
    }
    const existing = audiences.find((a) => a.toLowerCase() === name.toLowerCase());
    if (existing) {
      // Already a real audience — just add it to the picked set if missing.
      if (!panelStore.paintBrushes.includes(existing)) {
        panelStore.toggleBrush(existing);
      }
      cancelAdd();
      return;
    }
    colorStore.assignColor(name);
    panelStore.createTransientBrush(name);
    cancelAdd();
  }

  // ── Rename ────────────────────────────────────────────────────────────

  async function startEdit(name: string) {
    editingAudience = name;
    editValue = name;
    cancelAdd();
    await tick();
    editInputEl?.focus();
    editInputEl?.select();
  }

  function cancelEdit() {
    editingAudience = null;
    editValue = "";
  }

  function collectPaths(node: TreeNode): string[] {
    const out: string[] = [node.path];
    for (const child of node.children) out.push(...collectPaths(child));
    return out;
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
    if (!oldName || !newName) {
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

    // Transient case: nothing on disk yet — just rename in memory.
    if (panelStore.transientAudience === oldName) {
      colorStore.renameColor(oldName, newName);
      panelStore.createTransientBrush(newName);
      cancelEdit();
      return;
    }

    // Real audience: rewrite frontmatter + content markers across the workspace.
    if (!api) {
      cancelEdit();
      return;
    }
    renaming = true;
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
      if (panelStore.paintBrushes.includes(oldName)) {
        panelStore.setBrushes(
          panelStore.paintBrushes.map((b) => (b === oldName ? newName : b)),
        );
      }
      templateContextStore.bumpAudiencesVersion();
      toast.success(
        `Renamed "${oldName}" \u2192 "${newName}" across ${paths.length} entr${paths.length === 1 ? "y" : "ies"}`,
      );
    } catch (e) {
      console.error("[AudiencePanel] Rename failed:", e);
      toast.error("Rename failed \u2014 check console");
    } finally {
      renaming = false;
      cancelEdit();
    }
  }

  // ── Recolor ───────────────────────────────────────────────────────────

  function pickColor(name: string, color: string) {
    colorStore.setColor(name, color);
  }
</script>

<div class="paint-mode">
  <div class="brush-hint">
    {#if clearBrushActive}
      Click entries in sidebar, or select text and apply.
    {:else if audienceBrushes.length > 0}
      Click entries or select text to toggle
      <span class="brush-list">
        {#each audienceBrushes as brush, i}
          <span class="brush-name">
            <span class="dot {getAudienceColor(brush, colorStore.audienceColors)}"></span>
            {brush}
          </span>
          {#if i < audienceBrushes.length - 1}<span class="brush-sep">,</span>{/if}
        {/each}
      </span>
    {:else if displayedAudiences.length === 0}
      Add an audience below, then click entries or select text.
    {:else}
      Pick brushes below, then click entries or select text.
    {/if}
  </div>

  <!-- Apply to selection button — only when brushes picked AND a non-empty editor selection -->
  {#if panelStore.paintBrushes.length > 0 && panelStore.hasEditorSelection}
    <button
      type="button"
      class="apply-btn"
      onclick={handleApplyToSelection}
    >
      <Paintbrush class="size-3.5" />
      Apply to selection
    </button>
  {/if}

  <!-- Clear/eraser brush -->
  {#if displayedAudiences.length > 0}
    <button
      type="button"
      class="brush-row"
      class:active={clearBrushActive}
      onclick={selectClearBrush}
    >
      <Eraser class="size-3.5 text-muted-foreground" />
      <span class="brush-name-text">Clear audience</span>
    </button>
  {/if}

  <!-- Audience brushes (real + transient) -->
  {#each displayedAudiences as audience}
    {#if editingAudience === audience}
      <div class="add-row-edit">
        <span class="dot {getAudienceColor(audience, colorStore.audienceColors)}"></span>
        <input
          type="text"
          class="add-input"
          bind:value={editValue}
          bind:this={editInputEl}
          disabled={renaming}
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
        />
        <button
          type="button"
          class="icon-btn confirm"
          onclick={confirmRename}
          disabled={renaming}
          aria-label="Confirm rename"
        >
          {#if renaming}
            <Loader2 class="size-3.5 animate-spin" />
          {:else}
            <Check class="size-3.5" />
          {/if}
        </button>
        <button
          type="button"
          class="icon-btn"
          onclick={cancelEdit}
          disabled={renaming}
          aria-label="Cancel rename"
        >
          <X class="size-3.5" />
        </button>
      </div>
    {:else}
      <ContextMenu.Root>
        <ContextMenu.Trigger>
          {#snippet child({ props })}
            <button
              type="button"
              class="brush-row"
              class:active={panelStore.paintBrushes.includes(audience)}
              onclick={() => selectBrush(audience)}
              {...props}
            >
              <span class="dot {getAudienceColor(audience, colorStore.audienceColors)}"></span>
              <span class="brush-name-text">{audience}</span>
              {#if panelStore.paintBrushes.includes(audience)}
                {@const pickOrder = panelStore.paintBrushes.indexOf(audience) + 1}
                {#if audienceBrushes.length > 1}
                  <span class="pick-order">{pickOrder}</span>
                {/if}
              {/if}
            </button>
          {/snippet}
        </ContextMenu.Trigger>
        <ContextMenu.Content class="w-48">
          <ContextMenu.Item onclick={() => startEdit(audience)}>
            <Pencil class="size-4 mr-2" />
            Rename
          </ContextMenu.Item>
          <ContextMenu.Separator />
          <div class="px-2 py-1.5">
            <div class="text-muted-foreground mb-1.5 text-xs">Color</div>
            <div class="color-picker-grid">
              {#each AUDIENCE_PALETTE as color}
                <button
                  type="button"
                  class="color-option {color}"
                  class:selected={getAudienceColor(audience, colorStore.audienceColors) === color}
                  onclick={() => pickColor(audience, color)}
                  aria-label="Set color {color}"
                ></button>
              {/each}
            </div>
          </div>
        </ContextMenu.Content>
      </ContextMenu.Root>
    {/if}
  {/each}

  <!-- Add audience -->
  {#if isAdding}
    <div class="add-row-edit">
      <input
        type="text"
        class="add-input"
        placeholder="New audience name"
        bind:value={addValue}
        bind:this={addInputEl}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            confirmAdd();
          }
          if (e.key === "Escape") {
            e.preventDefault();
            cancelAdd();
          }
        }}
      />
      <button
        type="button"
        class="icon-btn confirm"
        onclick={confirmAdd}
        aria-label="Confirm new audience"
      >
        <Check class="size-3.5" />
      </button>
      <button
        type="button"
        class="icon-btn"
        onclick={cancelAdd}
        aria-label="Cancel"
      >
        <X class="size-3.5" />
      </button>
    </div>
  {:else}
    <button
      type="button"
      class="brush-row add-row"
      onclick={startAdd}
    >
      <Plus class="size-3.5 text-muted-foreground" />
      <span class="brush-name-text">Add audience</span>
    </button>
  {/if}
</div>

<style>
  .paint-mode {
    padding: 4px;
  }

  .empty-state {
    padding: 16px;
    text-align: center;
    font-size: 12px;
    color: var(--muted-foreground);
  }

  .brush-hint {
    padding: 6px 8px;
    font-size: 11px;
    color: var(--muted-foreground);
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }

  .brush-name {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 500;
    color: var(--foreground);
  }

  .brush-list {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }

  .brush-sep {
    color: var(--muted-foreground);
    margin-right: 2px;
  }

  .pick-order {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    font-size: 10px;
    font-weight: 600;
    color: var(--primary-foreground);
    background: var(--primary);
    border-radius: 8px;
    flex-shrink: 0;
  }

  .apply-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    width: calc(100% - 8px);
    margin: 4px 4px 2px;
    padding: 6px 10px;
    font-size: 12px;
    font-weight: 500;
    color: var(--primary-foreground);
    background: var(--primary);
    border: none;
    border-radius: 6px;
    cursor: pointer;
    transition: opacity 0.15s ease;
  }

  .apply-btn:hover {
    opacity: 0.9;
  }

  .apply-btn:active {
    opacity: 0.8;
  }

  .brush-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    font-size: 13px;
    color: var(--foreground);
    background: transparent;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.1s ease;
    text-align: left;
  }

  .brush-row:hover {
    background: var(--muted);
  }

  .brush-row.active {
    background: color-mix(in oklch, var(--primary) 12%, transparent);
    outline: 1.5px solid color-mix(in oklch, var(--primary) 30%, transparent);
  }

  .brush-name-text {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .add-row {
    color: var(--muted-foreground);
  }

  .add-row:hover {
    color: var(--foreground);
  }

  .add-row-edit {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
  }

  .add-input {
    flex: 1;
    min-width: 0;
    padding: 4px 8px;
    font-size: 13px;
    background: var(--input);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--foreground);
    outline: none;
  }

  .add-input:focus {
    border-color: var(--primary);
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--muted-foreground);
    cursor: pointer;
    transition: all 0.1s ease;
    flex-shrink: 0;
  }

  .icon-btn:hover {
    background: var(--muted);
    color: var(--foreground);
  }

  .icon-btn.confirm {
    color: var(--primary);
  }

  .color-picker-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .color-option {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 2px solid transparent;
    cursor: pointer;
    transition: transform 0.1s ease;
  }

  .color-option:hover {
    transform: scale(1.15);
  }

  .color-option.selected {
    border-color: var(--foreground);
  }
</style>
