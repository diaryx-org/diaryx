<script lang="ts">
  /**
   * TemplateVariableNodeView — Inline display for {{ variable }} expressions.
   *
   * Shows the resolved value from frontmatter by default (looks like normal text).
   * Shows the variable name in a pill when the node is selected.
   * Falls back to pill style when the variable is unresolved.
   */

  import { getTemplateContextStore } from "../stores/templateContextStore.svelte";

  interface Props {
    name: string;
    readonly?: boolean;
    selected?: boolean;
  }

  let { name, readonly = false, selected = false }: Props = $props();

  const templateContextStore = getTemplateContextStore();

  let resolvedValue = $derived.by(() => {
    const preview = templateContextStore.previewAudience;
    // In preview mode, override "audience" variable with the preview value
    if (preview !== null && name === "audience") {
      return preview;
    }
    return templateContextStore.resolve(name);
  });
  let hasValue = $derived(resolvedValue !== null && resolvedValue !== "");

  // Show pill when selected or when there's no resolved value
  let showAsPill = $derived(selected || !hasValue);
</script>

{#if showAsPill}
  <span
    class="template-variable-pill"
    class:readonly
    class:selected
    title="Template variable: {name}"
  >
    <span class="template-variable-braces">{"{}"}</span>
    <span class="template-variable-name">{selected ? name : `{{ ${name} }}`}</span>
  </span>
{:else}
  <span class="template-variable-resolved" title="Template variable: {name}">
    {resolvedValue}
  </span>
{/if}

<style>
  .template-variable-pill {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px;
    border-radius: 4px;
    background: color-mix(in oklch, var(--primary) 12%, transparent);
    border: 1px solid color-mix(in oklch, var(--primary) 25%, transparent);
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 0.85em;
    line-height: 1.4;
    vertical-align: baseline;
    white-space: nowrap;
    cursor: default;
    user-select: none;
  }

  .template-variable-braces {
    color: var(--primary);
    font-weight: 600;
    opacity: 0.7;
    font-size: 0.9em;
  }

  .template-variable-name {
    color: var(--primary);
    font-weight: 500;
  }

  .readonly {
    opacity: 0.8;
  }

  .selected {
    background: color-mix(in oklch, var(--primary) 20%, transparent);
    border-color: color-mix(in oklch, var(--primary) 40%, transparent);
    outline: 2px solid color-mix(in oklch, var(--primary) 30%, transparent);
    outline-offset: 1px;
  }

  .template-variable-resolved {
    cursor: default;
    user-select: none;
  }

  .template-variable-resolved:hover {
    text-decoration: underline dotted;
    text-decoration-color: color-mix(in oklch, var(--primary) 40%, transparent);
    text-underline-offset: 2px;
  }
</style>
