<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import { Loader2, Upload } from '@lucide/svelte';

  interface Props {
    namespaceId: string | null;
    canPublish: boolean;
    publishedAudienceCount: number;
    isPublishing: boolean;
    isCreatingNamespace: boolean;
    onPublish: () => void;
  }

  let { canPublish, publishedAudienceCount, isPublishing, isCreatingNamespace, onPublish }: Props = $props();
</script>

<Button
  class="w-full"
  variant="default"
  onclick={onPublish}
  disabled={!canPublish}
>
  {#if isPublishing}
    <Loader2 class="size-4 mr-2 animate-spin" />
    Publishing...
  {:else if isCreatingNamespace}
    <Loader2 class="size-4 mr-2 animate-spin" />
    Setting up namespace...
  {:else}
    <Upload class="size-4 mr-2" />
    Publish
    {#if publishedAudienceCount > 0}
      ({publishedAudienceCount} {publishedAudienceCount === 1 ? 'audience' : 'audiences'})
    {/if}
  {/if}
</Button>
