<script lang="ts">
  import { getNamespaceContext } from './namespaceContext.svelte';
  import NamespaceSubdomainManager from './NamespaceSubdomainManager.svelte';

  const ctx = getNamespaceContext();
</script>

{#if ctx.isReady && ctx.isConfigured && ctx.namespaceId && ctx.subdomainsAvailable}
  <NamespaceSubdomainManager
    namespaceId={ctx.namespaceId}
    subdomain={ctx.subdomain}
    firstPublishedAudience={ctx.firstPublishedAudience}
    onSubdomainChange={(s) => ctx.handleSubdomainChange(s)}
  />
{/if}
