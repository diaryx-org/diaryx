<script lang="ts">
  let { ...rest }: { [key: string]: unknown } = $props();
</script>

<span {...rest}></span>
