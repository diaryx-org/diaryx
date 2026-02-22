<script lang="ts">
  /**
   * TemplateVariableNodeView — Inline pill display for {{ variable }} expressions.
   *
   * Renders as a styled chip showing the variable name with a "{ }" prefix icon.
   */

  interface Props {
    name: string;
    readonly?: boolean;
  }

  let { name, readonly = false }: Props = $props();
</script>

<span class="template-variable-pill" class:readonly title="Template variable: {name}">
  <span class="template-variable-braces">{"{}"}</span>
  <span class="template-variable-name">{name}</span>
</span>

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
</style>
