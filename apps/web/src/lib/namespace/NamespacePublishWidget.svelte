<script lang="ts">
  import { getNamespaceContext } from './namespaceContext.svelte';
  import NamespacePublishButton from './NamespacePublishButton.svelte';

  const ctx = getNamespaceContext();
</script>

{#if ctx.isReady && ctx.hasAnyAudience}
  <NamespacePublishButton
    namespaceId={ctx.namespaceId}
    canPublish={ctx.canPublish}
    publishedAudienceCount={ctx.publishedAudienceCount}
    isPublishing={ctx.isPublishing}
    isCreatingNamespace={ctx.isCreatingNamespace}
    onPublish={() => ctx.handlePublish()}
  />
{/if}
