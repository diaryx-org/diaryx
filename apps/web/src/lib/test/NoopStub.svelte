<script lang="ts">
  let {
    children,
    ...rest
  }: {
    children?: import("svelte").Snippet;
    [key: string]: unknown;
  } = $props();
</script>

<div {...rest}>
  {@render children?.()}
</div>
