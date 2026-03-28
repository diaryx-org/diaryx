<script lang="ts">
  let {
    value = $bindable(""),
    ...rest
  }: {
    value?: string;
    [key: string]: unknown;
  } = $props();
</script>

<input bind:value {...rest} />
