<script lang="ts">
  let {
    children,
    child,
    ...rest
  }: {
    children?: import("svelte").Snippet;
    child?: import("svelte").Snippet<[{ props: Record<string, unknown> }]>;
    [key: string]: unknown;
  } = $props();
</script>

<div {...rest}>
  {#if child}
    {@render child({ props: {} })}
  {:else}
    {@render children?.()}
  {/if}
</div>
