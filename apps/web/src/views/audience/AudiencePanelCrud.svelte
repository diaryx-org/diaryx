<script lang="ts">
  /**
   * AudiencePanelCrud — Inline create/rename/delete/color-pick for audiences.
   *
   * Collapsible footer section in the Audience Panel, available in both modes.
   * CRUD logic extracted from the former AudienceManager.
   */
  import type { Api } from "$lib/backend/api";
  import type { TreeNode } from "$lib/backend/interface";
  import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor, AUDIENCE_PALETTE } from "$lib/utils/audienceDotColor";
  import { getAudiencePanelStore } from "$lib/stores/audiencePanelStore.svelte";
  import { toast } from "svelte-sonner";
  import {
    Plus,
    Pencil,
    Trash2,
    Check,
    X,
    Loader2,
    AlertTriangle,
    ChevronDown,
    ChevronUp,
  } from "@lucide/svelte";

  interface Props {
    audiences: string[];
    api: Api | null;
    rootPath: string;
    onCreated?: (name: string) => void;
    onRenamed?: (oldName: string, newName: string) => void;
    onDeleted?: (name: string) => void;
  }

  let { audiences, api, rootPath, onCreated, onRenamed, onDeleted }: Props = $props();

  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();
  const panelStore = getAudiencePanelStore();

  let expanded = $state(false);

  // Create state
  let isCreating = $state(false);
  let createValue = $state("");

  // Rename state
  let editingAudience = $state<string | null>(null);
  let editValue = $state("");
  let working = $state(false);

  // Delete state
  let deleteCandidate = $state<{ name: string; count: number } | null>(null);
  let deleteWorking = $state(false);

  // Color picker state
  let colorPickerOpen = $state<string | null>(null);

  // ── Helpers ────────────────────────────────────────────────────────────

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

  // ── Create ────────────────────────────────────────────────────────────

  function startCreate() {
    isCreating = true;
    createValue = "";
    editingAudience = null;
    deleteCandidate = null;
    colorPickerOpen = null;
    expanded = true;
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
    templateContextStore.bumpAudiencesVersion();
    onCreated?.(name);
    cancelCreate();
    // Auto-select as brush in paint mode
    if (panelStore.mode === "paint") {
      panelStore.setBrush(name);
    }
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
      if (panelStore.paintBrush === oldName) panelStore.setBrush(newName);
      templateContextStore.bumpAudiencesVersion();
      onRenamed?.(oldName, newName);
      toast.success(
        `Renamed "${oldName}" \u2192 "${newName}" across ${paths.length} entr${paths.length === 1 ? "y" : "ies"}`,
      );
    } catch (e) {
      console.error("[AudiencePanel] Rename failed:", e);
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
      if (panelStore.paintBrush === name) panelStore.setBrush(null);
      templateContextStore.bumpAudiencesVersion();
      onDeleted?.(name);
      toast.success(
        `Deleted "${name}" from ${paths.length} entr${paths.length === 1 ? "y" : "ies"}`,
      );
    } catch (e) {
      console.error("[AudiencePanel] Delete failed:", e);
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
</script>

<div class="crud-section">
  <!-- Toggle bar -->
  <button
    type="button"
    class="crud-toggle"
    onclick={() => (expanded = !expanded)}
  >
    <span class="crud-toggle-label">Manage audiences</span>
    {#if expanded}
      <ChevronDown class="size-3.5" />
    {:else}
      <ChevronUp class="size-3.5" />
    {/if}
  </button>

  {#if expanded}
    <div class="crud-body">
      <!-- Create -->
      {#if isCreating}
        <div class="crud-row">
          <input
            type="text"
            class="crud-input"
            placeholder="New audience name"
            bind:value={createValue}
            onkeydown={(e) => {
              if (e.key === "Enter") confirmCreate();
              if (e.key === "Escape") cancelCreate();
            }}
          />
          <button type="button" class="icon-btn confirm" onclick={confirmCreate} aria-label="Confirm">
            <Check class="size-3.5" />
          </button>
          <button type="button" class="icon-btn" onclick={cancelCreate} aria-label="Cancel">
            <X class="size-3.5" />
          </button>
        </div>
      {:else}
        <button
          type="button"
          class="crud-row add-row"
          onclick={startCreate}
        >
          <Plus class="size-3.5" />
          <span>New audience</span>
        </button>
      {/if}

      <!-- Delete confirmation -->
      {#if deleteCandidate}
        <div class="delete-confirm">
          <div class="delete-warn">
            <AlertTriangle class="size-3.5" />
            Delete "{deleteCandidate.name}"? Will remove from {deleteCandidate.count}
            {deleteCandidate.count === 1 ? "entry" : "entries"}.
          </div>
          <div class="delete-actions">
            <button
              type="button"
              class="icon-btn danger"
              onclick={confirmDelete}
              disabled={deleteWorking}
            >
              {#if deleteWorking}
                <Loader2 class="size-3.5 animate-spin" />
              {:else}
                Delete
              {/if}
            </button>
            <button type="button" class="icon-btn" onclick={cancelDelete}>Cancel</button>
          </div>
        </div>
      {/if}

      <!-- Audience list with edit/delete/color actions -->
      {#each audiences as audience}
        <div class="crud-audience-row">
          {#if editingAudience === audience}
            <input
              type="text"
              class="crud-input"
              bind:value={editValue}
              onkeydown={(e) => {
                if (e.key === "Enter") confirmRename();
                if (e.key === "Escape") cancelEdit();
              }}
            />
            <button
              type="button"
              class="icon-btn confirm"
              onclick={confirmRename}
              disabled={working}
              aria-label="Confirm rename"
            >
              {#if working}
                <Loader2 class="size-3.5 animate-spin" />
              {:else}
                <Check class="size-3.5" />
              {/if}
            </button>
            <button type="button" class="icon-btn" onclick={cancelEdit} aria-label="Cancel">
              <X class="size-3.5" />
            </button>
          {:else}
            <!-- Color swatch (clickable) -->
            <button
              type="button"
              class="color-swatch {getAudienceColor(audience, colorStore.audienceColors)}"
              onclick={() => toggleColorPicker(audience)}
              aria-label="Change color for {audience}"
            ></button>
            <span class="audience-label">{audience}</span>
            <button
              type="button"
              class="icon-btn"
              onclick={() => startEdit(audience)}
              aria-label="Rename {audience}"
            >
              <Pencil class="size-3" />
            </button>
            <button
              type="button"
              class="icon-btn danger-hover"
              onclick={() => requestDelete(audience)}
              aria-label="Delete {audience}"
            >
              <Trash2 class="size-3" />
            </button>
          {/if}
        </div>

        <!-- Color picker dropdown -->
        {#if colorPickerOpen === audience}
          <div class="color-picker">
            {#each AUDIENCE_PALETTE as color}
              <button
                type="button"
                class="color-option {color}"
                class:selected={getAudienceColor(audience, colorStore.audienceColors) === color}
                onclick={() => pickColor(audience, color)}
                aria-label="Select {color}"
              ></button>
            {/each}
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .crud-section {
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }

  .crud-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 6px 12px;
    font-size: 11px;
    font-weight: 500;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: color 0.15s ease;
  }

  .crud-toggle:hover {
    color: var(--foreground);
  }

  .crud-toggle-label {
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .crud-body {
    padding: 4px 8px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .crud-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 6px;
    border-radius: 6px;
  }

  .add-row {
    font-size: 12px;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: color 0.15s ease;
  }

  .add-row:hover {
    color: var(--foreground);
  }

  .crud-input {
    flex: 1;
    min-width: 0;
    padding: 3px 6px;
    font-size: 12px;
    background: var(--input);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--foreground);
    outline: none;
  }

  .crud-input:focus {
    border-color: var(--primary);
  }

  .crud-audience-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 6px;
    border-radius: 6px;
  }

  .audience-label {
    flex: 1;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .color-swatch {
    width: 14px;
    height: 14px;
    border-radius: 4px;
    border: 1px solid color-mix(in oklch, var(--foreground) 15%, transparent);
    cursor: pointer;
    flex-shrink: 0;
    transition: transform 0.1s ease;
  }

  .color-swatch:hover {
    transform: scale(1.15);
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 3px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--muted-foreground);
    cursor: pointer;
    font-size: 11px;
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

  .icon-btn.danger {
    color: hsl(var(--destructive));
  }

  .icon-btn.danger-hover:hover {
    color: hsl(var(--destructive));
  }

  .delete-confirm {
    padding: 8px;
    margin: 4px 0;
    background: color-mix(in oklch, hsl(var(--destructive)) 8%, transparent);
    border: 1px solid color-mix(in oklch, hsl(var(--destructive)) 20%, transparent);
    border-radius: 6px;
    font-size: 12px;
  }

  .delete-warn {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    color: hsl(var(--destructive));
    margin-bottom: 8px;
  }

  .delete-actions {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }

  .color-picker {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 4px 6px 6px 26px;
  }

  .color-option {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid transparent;
    cursor: pointer;
    transition: transform 0.1s ease;
  }

  .color-option:hover {
    transform: scale(1.2);
  }

  .color-option.selected {
    border-color: var(--foreground);
  }
</style>
