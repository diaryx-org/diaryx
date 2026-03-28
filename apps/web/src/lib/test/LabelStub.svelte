<script lang="ts">
  let {
    children,
    ...rest
  }: {
    children?: import("svelte").Snippet;
    [key: string]: unknown;
  } = $props();
</script>

<label {...rest}>
  {@render children?.()}
</label>
