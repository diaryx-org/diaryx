<script lang="ts">
  let { open = false }: { open?: boolean } = $props();
</script>

{#if open}
  <div>Sign out dialog</div>
{/if}
