<script lang="ts">
  /**
   * ManageAudiencesModal — list, create, rename, and delete audiences.
   *
   * Colors are persistent: each audience gets a Tailwind bg class assigned
   * at creation time and stored in audienceColorStore. Renaming moves the
   * color entry so visual identity never breaks. A per-row inline swatch
   * picker lets users manually choose any of the 8 palette colors.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Plus, Pencil, Trash2, Check, X, AlertTriangle, Loader2 } from "@lucide/svelte";
  import { toast } from "svelte-sonner";
  import type { Api } from "$lib/backend/api";
  import type { TreeNode } from "$lib/backend/interface";
  import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor, AUDIENCE_PALETTE } from "$lib/utils/audienceDotColor";

  interface Props {
    open: boolean;
    api: Api | null;
    rootPath: string;
    onClose: () => void;
  }

  let { open, api, rootPath, onClose }: Props = $props();

  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();

  // ── Modal state ─────────────────────────────────────────────────────────
  let audiences = $state<string[]>([]);
  let loading = $state(false);
  let working = $state(false);

  // Create
  let isCreating = $state(false);
  let createValue = $state("");

  // Rename
  let editingAudience = $state<string | null>(null);
  let editValue = $state("");

  // Delete
  let deleteCandidate = $state<{ name: string; count: number } | null>(null);
  let deleteWorking = $state(false);

  // Color picker — which audience row has its swatch palette open
  let colorPickerOpen = $state<string | null>(null);

  // ── Load ────────────────────────────────────────────────────────────────
  $effect(() => {
    if (open && api && rootPath) loadAudiences();
  });

  async function loadAudiences() {
    loading = true;
    try {
      audiences = await api!.getAvailableAudiences(rootPath);
      // Ensure every existing audience has a persisted color
      for (const name of audiences) colorStore.assignColor(name);
    } catch {
      audiences = [];
    } finally {
      loading = false;
    }
  }

  // ── Tree traversal ──────────────────────────────────────────────────────
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

  // ── Create ──────────────────────────────────────────────────────────────
  function startCreate() {
    isCreating = true;
    createValue = "";
    editingAudience = null;
    deleteCandidate = null;
    colorPickerOpen = null;
  }

  function cancelCreate() {
    isCreating = false;
    createValue = "";
  }

  function confirmCreate() {
    const name = createValue.trim();
    if (!name) { cancelCreate(); return; }
    if (audiences.some((a) => a.toLowerCase() === name.toLowerCase())) {
      toast.error(`"${name}" already exists`);
      return;
    }
    colorStore.assignColor(name);
    audiences = [...audiences, name].sort();
    templateContextStore.bumpAudiencesVersion();
    cancelCreate();
  }

  // ── Rename ──────────────────────────────────────────────────────────────
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
    if (!oldName || !newName || !api) { cancelEdit(); return; }
    if (oldName === newName) { cancelEdit(); return; }
    if (audiences.some((a) => a !== oldName && a.toLowerCase() === newName.toLowerCase())) {
      toast.error(`"${newName}" already exists`);
      return;
    }
    working = true;
    try {
      const paths = await findEntriesWithAudience(oldName);
      await Promise.all(
        paths.map(async (path) => {
          // 1. Update frontmatter array
          const fm = await api!.getFrontmatter(path);
          const updated = (fm.audience as string[]).map((a) => (a === oldName ? newName : a));
          await api!.setFrontmatterProperty(path, "audience", updated, rootPath);
          // 2. Update {{#for-audience "..."}} markers in entry content
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
      // Move the persistent color entry to the new name
      colorStore.renameColor(oldName, newName);
      audiences = audiences.map((a) => (a === oldName ? newName : a));
      templateContextStore.bumpAudiencesVersion();
      toast.success(
        `Renamed "${oldName}" → "${newName}" across ${paths.length} entr${paths.length === 1 ? "y" : "ies"}`,
      );
    } catch (e) {
      console.error("[ManageAudiencesModal] Rename failed:", e);
      toast.error("Rename failed — check console");
    } finally {
      working = false;
      cancelEdit();
    }
  }

  // ── Delete ──────────────────────────────────────────────────────────────
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
            await api!.setFrontmatterProperty(path, "audience", remaining, rootPath);
          }
        }),
      );
      colorStore.deleteColor(name);
      audiences = audiences.filter((a) => a !== name);
      templateContextStore.bumpAudiencesVersion();
      toast.success(
        `Deleted "${name}" from ${paths.length} entr${paths.length === 1 ? "y" : "ies"}`,
      );
    } catch (e) {
      console.error("[ManageAudiencesModal] Delete failed:", e);
      toast.error("Delete failed — check console");
    } finally {
      deleteWorking = false;
      deleteCandidate = null;
    }
  }

  // ── Color picker ─────────────────────────────────────────────────────────
  function toggleColorPicker(name: string) {
    colorPickerOpen = colorPickerOpen === name ? null : name;
  }

  function pickColor(name: string, tailwindClass: string) {
    colorStore.setColor(name, tailwindClass);
    colorPickerOpen = null;
  }

  // ── Close ────────────────────────────────────────────────────────────────
  function handleOpenChange(isOpen: boolean) {
    if (!isOpen) {
      cancelEdit();
      cancelCreate();
      deleteCandidate = null;
      colorPickerOpen = null;
      onClose();
    }
  }
</script>

<Dialog.Root {open} onOpenChange={handleOpenChange}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Manage Audiences</Dialog.Title>
      <Dialog.Description>
        Rename or delete audiences across your entire workspace.
      </Dialog.Description>
    </Dialog.Header>

    <!-- Body -->
    <div class="mt-1 flex flex-col gap-1">
      {#if loading}
        <div class="flex items-center justify-center py-8 text-muted-foreground">
          <Loader2 class="size-4 animate-spin mr-2" />
          <span class="text-sm">Loading…</span>
        </div>
      {:else if audiences.length === 0 && !isCreating}
        <p class="py-6 text-center text-sm text-muted-foreground">
          No audiences yet. Create one below or add tags to entries.
        </p>
      {:else}
        <!-- Audience list -->
        <ul class="rounded-md border border-border divide-y divide-border">
          {#each audiences as name (name)}
            {@const isEditing = editingAudience === name}
            {@const isPendingDelete = deleteCandidate?.name === name}
            {@const dotClass = getAudienceColor(name, colorStore.audienceColors)}
            {@const pickerOpen = colorPickerOpen === name}
            <li
              class="flex flex-col text-sm transition-colors {isPendingDelete
                ? 'bg-destructive/10'
                : 'hover:bg-muted/40'}"
            >
              <!-- Main row -->
              <div class="flex items-center gap-2 px-3 py-2">
                <!-- Colored dot — click to open inline swatch picker -->
                <button
                  class="size-2.5 rounded-full shrink-0 {dotClass} focus:outline-none focus:ring-2 focus:ring-offset-1 focus:ring-foreground/30 hover:scale-125 transition-transform"
                  onclick={() => toggleColorPicker(name)}
                  aria-label="Change color for {name}"
                  title="Click to change color"
                  disabled={working || deleteWorking}
                ></button>

                {#if isEditing}
                  <!-- Edit mode -->
                  <!-- svelte-ignore a11y_autofocus -->
                  <input
                    class="flex-1 min-w-0 bg-transparent border-b border-border focus:border-primary outline-none text-sm py-0.5"
                    bind:value={editValue}
                    onkeydown={(e) => {
                      if (e.key === "Enter") { e.preventDefault(); confirmRename(); }
                      if (e.key === "Escape") { e.preventDefault(); cancelEdit(); }
                    }}
                    autofocus
                    disabled={working}
                  />
                  <button
                    class="text-muted-foreground hover:text-primary transition-colors disabled:opacity-40"
                    onclick={confirmRename}
                    disabled={working}
                    aria-label="Save rename"
                  >
                    {#if working}
                      <Loader2 class="size-3.5 animate-spin" />
                    {:else}
                      <Check class="size-3.5" />
                    {/if}
                  </button>
                  <button
                    class="text-muted-foreground hover:text-foreground transition-colors"
                    onclick={cancelEdit}
                    disabled={working}
                    aria-label="Cancel rename"
                  >
                    <X class="size-3.5" />
                  </button>
                {:else}
                  <!-- Normal view -->
                  <span
                    class="flex-1 truncate {isPendingDelete
                      ? 'text-muted-foreground line-through'
                      : ''}"
                  >{name}</span>
                  <div
                    class="flex items-center gap-1 opacity-0 focus-within:opacity-100 [li:hover_&]:opacity-100"
                  >
                    <button
                      class="text-muted-foreground hover:text-foreground transition-colors p-0.5 rounded"
                      onclick={() => startEdit(name)}
                      disabled={working || deleteWorking || !!deleteCandidate}
                      aria-label="Rename {name}"
                    >
                      <Pencil class="size-3.5" />
                    </button>
                    <button
                      class="text-muted-foreground hover:text-destructive transition-colors p-0.5 rounded"
                      onclick={() => requestDelete(name)}
                      disabled={working || deleteWorking || !!deleteCandidate}
                      aria-label="Delete {name}"
                    >
                      {#if working && !deleteCandidate}
                        <Loader2 class="size-3.5 animate-spin" />
                      {:else}
                        <Trash2 class="size-3.5" />
                      {/if}
                    </button>
                  </div>
                {/if}
              </div>

              <!-- Inline swatch picker — shown when dot is clicked -->
              {#if pickerOpen}
                <div class="flex items-center gap-2 px-3 pb-2.5 pt-0">
                  <span class="text-[10px] text-muted-foreground mr-0.5">Color:</span>
                  {#each AUDIENCE_PALETTE as swatch}
                    {@const isActive = dotClass === swatch}
                    <button
                      class="size-4 rounded-full {swatch} shrink-0 transition-transform hover:scale-110
                        {isActive ? 'ring-2 ring-offset-1 ring-foreground/60 scale-110' : ''}"
                      onclick={() => pickColor(name, swatch)}
                      aria-label="Set color {swatch}"
                      title={swatch}
                    ></button>
                  {/each}
                </div>
              {/if}
            </li>
          {/each}

          <!-- Inline create row -->
          {#if isCreating}
            <li class="flex items-center gap-2 px-3 py-2 text-sm bg-muted/30">
              <span
                class="size-2.5 rounded-full shrink-0 {getAudienceColor(
                  createValue,
                  colorStore.audienceColors,
                )}"
              ></span>
              <!-- svelte-ignore a11y_autofocus -->
              <input
                class="flex-1 min-w-0 bg-transparent border-b border-border focus:border-primary outline-none text-sm py-0.5 placeholder:text-muted-foreground"
                placeholder="New audience name…"
                bind:value={createValue}
                onkeydown={(e) => {
                  if (e.key === "Enter") { e.preventDefault(); confirmCreate(); }
                  if (e.key === "Escape") { e.preventDefault(); cancelCreate(); }
                }}
                autofocus
              />
              <button
                class="text-muted-foreground hover:text-primary transition-colors"
                onclick={confirmCreate}
                aria-label="Add audience"
              >
                <Check class="size-3.5" />
              </button>
              <button
                class="text-muted-foreground hover:text-foreground transition-colors"
                onclick={cancelCreate}
                aria-label="Cancel"
              >
                <X class="size-3.5" />
              </button>
            </li>
          {/if}
        </ul>
      {/if}

      <!-- Delete warning banner -->
      {#if deleteCandidate}
        <div
          class="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2.5 flex flex-col gap-2"
        >
          <div class="flex items-start gap-2 text-sm">
            <AlertTriangle class="size-4 text-destructive shrink-0 mt-0.5" />
            <p class="text-foreground leading-snug">
              <strong>"{deleteCandidate.name}"</strong> is used in
              <strong
                >{deleteCandidate.count}
                {deleteCandidate.count === 1 ? "entry" : "entries"}</strong
              >. Deleting will remove this tag from all of them.
            </p>
          </div>
          <div class="flex items-center gap-2 justify-end">
            <Button variant="ghost" size="sm" onclick={cancelDelete} disabled={deleteWorking}>
              Cancel
            </Button>
            <Button variant="destructive" size="sm" onclick={confirmDelete} disabled={deleteWorking}>
              {#if deleteWorking}
                <Loader2 class="size-3 animate-spin mr-1" />
                Deleting…
              {:else}
                Delete
              {/if}
            </Button>
          </div>
        </div>
      {/if}
    </div>

    <!-- Footer -->
    <Dialog.Footer class="mt-2 flex items-center justify-between sm:justify-between">
      <Button
        variant="outline"
        size="sm"
        onclick={startCreate}
        disabled={isCreating || loading || working || deleteWorking}
      >
        <Plus class="size-3.5 mr-1.5" />
        Add audience
      </Button>
      <Dialog.Close>
        <Button variant="ghost" size="sm">Close</Button>
      </Dialog.Close>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
