<script lang="ts">
  let {
    children,
    type = "button",
    onclick,
    disabled = false,
    ...rest
  }: {
    children?: import("svelte").Snippet;
    type?: "button" | "submit" | "reset";
    onclick?: ((event: MouseEvent) => void) | null;
    disabled?: boolean;
    [key: string]: unknown;
  } = $props();
</script>

<button {type} {disabled} onclick={onclick} {...rest}>
  {@render children?.()}
</button>
