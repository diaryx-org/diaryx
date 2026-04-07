<script lang="ts">
  /**
   * VisibilityPicker — Audience multi-select popover for the bubble menu.
   *
   * Shows available audiences with colored dots. Users can toggle audiences
   * on/off for the current text selection. Supports creating new audiences.
   *
   * Works for both inline (applies VisibilityMark) and block contexts
   * (inserts VisibilityBlock).
   */
  import type { Editor } from "@tiptap/core";
  import { Eye, EyeOff, Plus } from "@lucide/svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor } from "$lib/utils/audienceDotColor";
  import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";
  import type { Api } from "$lib/backend/api";
  import {
    canWrapSelectionInVisibilityBlock,
    getVisibilityBlockForSelection,
  } from "$lib/extensions/VisibilityBlock";

  interface Props {
    editor: Editor | null;
    api?: Api | null;
    rootPath?: string;
    isActive?: boolean;
    open?: boolean;
    onOpen?: () => void;
  }

  let {
    editor,
    api = null,
    rootPath = "",
    isActive = false,
    open = $bindable(false),
    onOpen,
  }: Props = $props();

  const colorStore = getAudienceColorStore();
  const templateContextStore = getTemplateContextStore();

  let audiences = $state<string[]>([]);
  let newAudienceName = $state("");
  // Bumped when TipTap changes selection/doc state so derived reads don't stale-cache the editor.
  let editorVersion = $state(0);

  function readBlockSelection() {
    return editor ? getVisibilityBlockForSelection(editor.state) : null;
  }

  function readInlineAudiences(): string[] {
    if (!editor) return [];

    const attrs = editor.getAttributes("visibilityMark");
    if (attrs?.audiences?.length) return attrs.audiences as string[];

    const { from, to } = editor.state.selection;
    let found: string[] | null = null;
    editor.state.doc.nodesBetween(from, to, (node) => {
      if (found) return false;
      for (const mark of node.marks) {
        if (mark.type.name === "visibilityMark" && mark.attrs.audiences?.length) {
          found = mark.attrs.audiences as string[];
          return false;
        }
      }
    });
    if (found) return found;

    const storedMarks = editor.state.storedMarks ?? [];
    for (const mark of storedMarks) {
      if (mark.type.name === "visibilityMark" && mark.attrs.audiences?.length) {
        return mark.attrs.audiences as string[];
      }
    }

    return [];
  }

  function readShouldUseBlock() {
    if (!editor) return false;
    if (readBlockSelection() !== null) return true;
    if (readInlineAudiences().length > 0) return false;
    return canWrapSelectionInVisibilityBlock(editor.state);
  }

  function readCurrentAudiences(): string[] {
    if (!editor) return [];
    const block = readBlockSelection();
    if (block) return block.open.audiences;
    return readInlineAudiences();
  }

  // Re-read from editor whenever editorVersion bumps
  let blockSelection = $derived.by(() => {
    editorVersion;
    return readBlockSelection();
  });
  let shouldUseBlock = $derived.by(() => {
    editorVersion;
    return readShouldUseBlock();
  });
  let currentAudiences = $derived.by(() => {
    editorVersion;
    return readCurrentAudiences();
  });

  async function loadAudiences() {
    if (!api || !rootPath) return;
    try {
      audiences = await api.getAvailableAudiences(rootPath);
      for (const name of audiences) colorStore.assignColor(name);
    } catch {
      audiences = [];
    }
  }

  $effect(() => {
    if (open && api && rootPath) {
      loadAudiences();
    }
  });

  $effect(() => {
    if (!editor) return;

    const ed = editor;
    let disposed = false;
    const refreshEditorState = () => {
      queueMicrotask(() => {
        if (!disposed && editor === ed) editorVersion++;
      });
    };

    ed.on("selectionUpdate", refreshEditorState);
    ed.on("transaction", refreshEditorState);
    refreshEditorState();

    return () => {
      disposed = true;
      ed.off("selectionUpdate", refreshEditorState);
      ed.off("transaction", refreshEditorState);
    };
  });

  function toggleAudience(audience: string) {
    if (!editor) return;

    const current = currentAudiences;
    const isCurrentlyIncluded = current.some(
      (a) => a.toLowerCase() === audience.toLowerCase(),
    );

    if (isCurrentlyIncluded) {
      // Remove this audience
      const newAudiences = current.filter(
        (a) => a.toLowerCase() !== audience.toLowerCase(),
      );
      if (newAudiences.length === 0) {
        // No audiences left — remove the mark entirely
        if (shouldUseBlock) {
          editor.chain().focus().unsetVisibilityBlock().run();
        } else {
          editor.chain().focus().unsetVisibility().run();
        }
      } else {
        if (shouldUseBlock) {
          editor.chain().focus().setVisibilityBlock({ audiences: newAudiences }).run();
        } else {
          editor.chain().focus().setVisibility({ audiences: newAudiences }).run();
        }
      }
    } else {
      // Add this audience
      const newAudiences = [...current, audience];
      if (shouldUseBlock) {
        editor.chain().focus().setVisibilityBlock({ audiences: newAudiences }).run();
      } else {
        editor.chain().focus().setVisibility({ audiences: newAudiences }).run();
      }
    }
    editorVersion++;
  }

  function handleCreateAudience() {
    const name = newAudienceName.trim();
    if (!name || !editor) return;

    colorStore.assignColor(name);
    templateContextStore.bumpAudiencesVersion();

    // Apply with the new audience
    const alreadyPresent = currentAudiences.some(
      (a) => a.toLowerCase() === name.toLowerCase(),
    );
    const newAudiences = alreadyPresent ? currentAudiences : [...currentAudiences, name];
    if (shouldUseBlock) {
      editor.chain().focus().setVisibilityBlock({ audiences: newAudiences }).run();
    } else {
      editor.chain().focus().setVisibility({ audiences: newAudiences }).run();
    }

    newAudienceName = "";
    open = false;
  }

  function handleRemoveAll() {
    if (!editor) return;
    if (shouldUseBlock) {
      editor.chain().focus().unsetVisibilityBlock().run();
    } else {
      editor.chain().focus().unsetVisibility().run();
    }
    open = false;
  }

  function handleButtonClick() {
    // Always open the picker so existing block/inline audiences can be edited
    // in place. Removal is still available from the popover itself.
    onOpen?.();
    open = true;
  }
</script>

<div class="vis-picker-wrapper">
  <button
    type="button"
    class="toolbar-button"
    class:active={isActive}
    onmousedown={(e) => {
      e.preventDefault();
      e.stopPropagation();
      handleButtonClick();
    }}
    title={isActive ? "Edit visibility" : "Set visibility"}
    aria-pressed={isActive}
  >
    {#if isActive}
      <Eye class="size-4" />
    {:else}
      <EyeOff class="size-4" />
    {/if}
  </button>

  {#if open}
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div
      class="vis-picker-popover"
      role="listbox"
      aria-label="Select audiences"
      onmousedown={(e) => e.preventDefault()}
    >
      {#if audiences.length === 0}
        <div class="vis-picker-empty">No audiences defined yet</div>
      {:else}
        <div class="vis-picker-mode">
          {#if blockSelection}
            Editing block visibility
          {:else if shouldUseBlock}
            Wrap selection as block visibility
          {:else}
            Apply inline visibility
          {/if}
        </div>
        {#each audiences as audience (audience)}
          {@const isSelected = currentAudiences.some(
            (a) => a.toLowerCase() === audience.toLowerCase(),
          )}
          <button
            type="button"
            class="vis-picker-item"
            class:selected={isSelected}
            role="option"
            aria-selected={isSelected}
            onmousedown={(e) => {
              e.preventDefault();
              e.stopPropagation();
              toggleAudience(audience);
            }}
          >
            <span
              class="vis-picker-dot"
              style:background-color={(() => {
                const cls = getAudienceColor(
                  audience,
                  colorStore.audienceColors,
                );
                // Convert Tailwind class to inline color for the dot
                const map: Record<string, string> = {
                  "bg-indigo-500": "oklch(0.585 0.233 277.117)",
                  "bg-teal-500": "oklch(0.704 0.14 180.72)",
                  "bg-rose-500": "oklch(0.645 0.246 16.439)",
                  "bg-amber-500": "oklch(0.769 0.188 70.08)",
                  "bg-emerald-500": "oklch(0.696 0.17 162.48)",
                  "bg-violet-500": "oklch(0.606 0.25 292.717)",
                  "bg-cyan-500": "oklch(0.715 0.143 215.221)",
                  "bg-orange-500": "oklch(0.702 0.209 41.348)",
                  "bg-slate-500": "oklch(0.554 0.022 257.417)",
                };
                return map[cls] ?? "oklch(0.554 0.022 257.417)";
              })()}
            ></span>
            <span class="vis-picker-label">{audience}</span>
            {#if isSelected}
              <span class="vis-picker-check">✓</span>
            {/if}
          </button>
        {/each}
      {/if}

      <div class="vis-picker-divider"></div>

      <div class="vis-picker-create">
        <input
          type="text"
          class="vis-picker-input"
          placeholder="New audience..."
          bind:value={newAudienceName}
          onkeydown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              handleCreateAudience();
            }
          }}
        />
        {#if newAudienceName.trim()}
          <button
            type="button"
            class="vis-picker-create-btn"
            onmousedown={(e) => {
              e.preventDefault();
              e.stopPropagation();
              handleCreateAudience();
            }}
          >
            <Plus class="size-3" />
          </button>
        {/if}
      </div>

      {#if isActive}
        <button
          type="button"
          class="vis-picker-remove"
          onmousedown={(e) => {
            e.preventDefault();
            e.stopPropagation();
            handleRemoveAll();
          }}
        >
          Remove visibility filter
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .vis-picker-wrapper {
    position: relative;
    display: inline-flex;
  }

  .toolbar-button {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 6px;
    border-radius: 4px;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .toolbar-button:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .toolbar-button:active {
    transform: scale(0.95);
    background: var(--accent);
  }

  .toolbar-button.active {
    background: var(--accent);
    color: var(--accent-foreground);
    box-shadow: inset 0 -2px 0 var(--ring);
  }

  .vis-picker-popover {
    position: absolute;
    top: calc(100% + 4px);
    left: 50%;
    transform: translateX(-50%);
    min-width: 180px;
    max-width: 240px;
    padding: 4px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 100;
  }

  .vis-picker-empty {
    padding: 8px 12px;
    font-size: 12px;
    color: var(--muted-foreground);
    text-align: center;
  }

  .vis-picker-mode {
    padding: 6px 8px 4px;
    font-size: 11px;
    color: var(--muted-foreground);
  }

  .vis-picker-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    font-size: 13px;
    color: var(--foreground);
    background: transparent;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    transition: background 0.1s ease;
    text-align: left;
  }

  .vis-picker-item:hover {
    background: var(--accent);
  }

  .vis-picker-item.selected {
    background: color-mix(in oklch, var(--primary) 10%, transparent);
  }

  .vis-picker-dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .vis-picker-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .vis-picker-check {
    font-size: 11px;
    color: var(--primary);
    flex-shrink: 0;
  }

  .vis-picker-divider {
    height: 1px;
    background: var(--border);
    margin: 4px 0;
  }

  .vis-picker-create {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 4px;
  }

  .vis-picker-input {
    flex: 1;
    padding: 4px 8px;
    font-size: 12px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--foreground);
    outline: none;
  }

  .vis-picker-input:focus {
    border-color: var(--ring);
  }

  .vis-picker-input::placeholder {
    color: var(--muted-foreground);
  }

  .vis-picker-create-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    border-radius: 4px;
    color: var(--primary);
    background: transparent;
    border: none;
    cursor: pointer;
  }

  .vis-picker-create-btn:hover {
    background: var(--accent);
  }

  .vis-picker-remove {
    width: 100%;
    padding: 6px 8px;
    font-size: 12px;
    color: var(--destructive, oklch(0.577 0.245 27.325));
    background: transparent;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    text-align: left;
    transition: background 0.1s ease;
  }

  .vis-picker-remove:hover {
    background: color-mix(
      in oklch,
      var(--destructive, oklch(0.577 0.245 27.325)) 10%,
      transparent
    );
  }

  @media (max-width: 767px) {
    .toolbar-button {
      padding: 10px;
      min-width: 44px;
      min-height: 44px;
    }
  }
</style>
