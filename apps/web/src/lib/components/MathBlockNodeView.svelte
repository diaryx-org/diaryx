<script lang="ts">
  import { Code, Check } from "@lucide/svelte";

  interface Props {
    source: string;
    readonly: boolean;
    onUpdate: (source: string) => void;
    renderFn: (source: string, displayMode: boolean) => Promise<{ html?: string; error?: string }>;
  }

  let { source, readonly, onUpdate, renderFn }: Props = $props();

  let mode = $state<"preview" | "source">("preview");
  let sourceText = $state("");
  let renderedHtml = $state("");
  let renderError = $state("");
  let loading = $state(false);

  // Sync sourceText when prop changes (undo/redo)
  $effect(() => {
    sourceText = source;
  });

  // Render math when source changes
  $effect(() => {
    renderMath(source);
  });

  async function renderMath(src: string) {
    if (!src.trim()) {
      renderedHtml = "";
      renderError = "";
      return;
    }
    loading = true;
    try {
      const result = await renderFn(src, true);
      if (result.html) {
        renderedHtml = result.html;
        renderError = "";
      } else {
        renderedHtml = "";
        renderError = result.error || "Render failed";
      }
    } catch (e) {
      renderedHtml = "";
      renderError = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function commitSource() {
    if (sourceText !== source) {
      onUpdate(sourceText);
    }
    mode = "preview";
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      sourceText = source;
      mode = "preview";
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      commitSource();
    }
  }
</script>

<div class="math-block-container">
  <div class="math-block-header">
    <span class="math-block-label">Math</span>
    {#if !readonly}
      {#if mode === "preview"}
        <button
          type="button"
          class="math-block-toggle"
          onclick={() => (mode = "source")}
          title="Edit source"
        >
          <Code class="size-3.5" />
        </button>
      {:else}
        <button
          type="button"
          class="math-block-toggle"
          onclick={commitSource}
          title="Done editing"
        >
          <Check class="size-3.5" />
        </button>
      {/if}
    {/if}
  </div>

  {#if mode === "source" && !readonly}
    <textarea
      class="math-block-source"
      bind:value={sourceText}
      onblur={commitSource}
      onkeydown={handleKeydown}
      spellcheck="false"
      placeholder="Enter LaTeX math..."
    ></textarea>
  {:else}
    <div class="math-block-preview">
      {#if loading}
        <span class="math-block-loading">Rendering...</span>
      {:else if renderError}
        <div class="math-block-error">
          <code>{source}</code>
          <span class="math-block-error-msg">{renderError}</span>
        </div>
      {:else if renderedHtml}
        {@html renderedHtml}
      {:else}
        <span class="math-block-empty">Empty math block</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .math-block-container {
    border: 1px dashed var(--border);
    border-radius: 6px;
    margin: 0.5em 0;
    overflow: hidden;
    -webkit-user-select: none;
    user-select: none;
  }

  .math-block-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 8px;
    background: var(--muted);
    border-bottom: 1px solid var(--border);
  }

  .math-block-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--muted-foreground);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .math-block-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 2px;
    border: none;
    background: transparent;
    border-radius: 3px;
    cursor: pointer;
    color: var(--muted-foreground);
  }

  .math-block-toggle:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .math-block-preview {
    padding: 12px;
    text-align: center;
    overflow-x: auto;
  }

  .math-block-empty {
    color: var(--muted-foreground);
    font-style: italic;
    font-size: 13px;
  }

  .math-block-loading {
    color: var(--muted-foreground);
    font-style: italic;
    font-size: 13px;
  }

  .math-block-error {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: center;
  }

  .math-block-error code {
    font-size: 13px;
    color: var(--foreground);
  }

  .math-block-error-msg {
    font-size: 11px;
    color: var(--destructive);
  }

  .math-block-source {
    width: 100%;
    min-height: 60px;
    padding: 12px;
    border: none;
    background: var(--card);
    color: var(--foreground);
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 13px;
    line-height: 1.5;
    resize: vertical;
    outline: none;
    field-sizing: content;
  }
</style>
