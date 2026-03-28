<script lang="ts">
  let {
    children,
  }: {
    children?: import("svelte").Snippet;
    [key: string]: unknown;
  } = $props();
</script>

{@render children?.()}
