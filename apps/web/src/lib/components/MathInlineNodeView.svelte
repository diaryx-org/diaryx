<script lang="ts">
  interface Props {
    source: string;
    readonly: boolean;
    onUpdate: (source: string) => void;
    renderFn: (source: string, displayMode: boolean) => Promise<{ html?: string; error?: string }>;
  }

  let { source, readonly, onUpdate, renderFn }: Props = $props();

  let editing = $state(false);
  let editText = $state("");
  let renderedHtml = $state("");
  let renderError = $state("");
  let loading = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();

  // Render when source changes
  $effect(() => {
    renderMath(source);
  });

  // Focus input when editing starts
  $effect(() => {
    if (editing && inputEl) {
      inputEl.focus();
      inputEl.select();
    }
  });

  async function renderMath(src: string) {
    if (!src.trim()) {
      renderedHtml = "";
      renderError = "";
      return;
    }
    loading = true;
    try {
      const result = await renderFn(src, false);
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

  function startEditing() {
    if (readonly) return;
    editText = source;
    editing = true;
  }

  function commitEdit() {
    if (editText !== source) {
      onUpdate(editText);
    }
    editing = false;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      commitEdit();
    } else if (e.key === "Escape") {
      editing = false;
    }
  }
</script>

{#if editing}
  <span class="math-inline-editing">
    <span class="math-inline-dollar">$</span>
    <input
      type="text"
      class="math-inline-input"
      bind:this={inputEl}
      bind:value={editText}
      onblur={commitEdit}
      onkeydown={handleKeydown}
      spellcheck="false"
    />
    <span class="math-inline-dollar">$</span>
  </span>
{:else}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <span
    class="math-inline-rendered"
    class:math-inline-error={!!renderError}
    onclick={startEditing}
    title={renderError || source}
  >
    {#if loading}
      <span class="math-inline-loading">$…$</span>
    {:else if renderError}
      <code class="math-inline-error-code">${source}$</code>
    {:else if renderedHtml}
      {@html renderedHtml}
    {:else}
      <span class="math-inline-empty">$\cdot$</span>
    {/if}
  </span>
{/if}

<style>
  .math-inline-rendered {
    cursor: pointer;
    border-radius: 3px;
    padding: 0 2px;
    transition: background 0.15s;
  }

  .math-inline-rendered:hover {
    background: var(--accent);
  }

  .math-inline-error {
    border-bottom: 1px wavy var(--destructive);
  }

  .math-inline-error-code {
    font-size: 0.9em;
    color: var(--destructive);
  }

  .math-inline-loading {
    color: var(--muted-foreground);
    font-style: italic;
  }

  .math-inline-empty {
    color: var(--muted-foreground);
  }

  .math-inline-editing {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    background: var(--muted);
    border-radius: 3px;
    padding: 0 2px;
  }

  .math-inline-dollar {
    color: var(--muted-foreground);
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 0.85em;
  }

  .math-inline-input {
    border: none;
    background: transparent;
    color: var(--foreground);
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 0.85em;
    outline: none;
    min-width: 3em;
    width: auto;
    padding: 0;
  }
</style>
