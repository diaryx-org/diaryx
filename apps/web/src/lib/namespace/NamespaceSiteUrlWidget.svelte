<script lang="ts">
  import { getNamespaceContext } from './namespaceContext.svelte';
  import NamespaceSiteUrl from './NamespaceSiteUrl.svelte';

  const ctx = getNamespaceContext();
</script>

{#if ctx.isReady}
  <NamespaceSiteUrl
    namespaceId={ctx.namespaceId}
    subdomain={ctx.subdomain}
    audienceStates={ctx.audienceStates}
  />
{/if}
