<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import { Check, Copy } from '@lucide/svelte';
  import { showError } from '@/models/services/toastService';

  interface Props {
    namespaceId: string | null;
    subdomain: string | null;
    audienceStates: Record<string, { state: string }>;
  }

  let { namespaceId, subdomain, audienceStates }: Props = $props();
  let copied = $state(false);

  let siteUrl = $derived.by(() => {
    if (!namespaceId) return null;
    if (subdomain) return `https://${subdomain}.diaryx.org`;
    const base = `https://diaryx.org/ns/${namespaceId}`;
    const firstPublished = Object.entries(audienceStates).find(([, c]) => c.state !== 'unpublished');
    if (firstPublished) return `${base}/${firstPublished[0]}/index.html`;
    return base;
  });

  async function copyUrl() {
    if (!siteUrl) return;
    try {
      await navigator.clipboard.writeText(siteUrl);
      copied = true;
      setTimeout(() => { copied = false; }, 1800);
    } catch {
      showError('Copy failed. Check browser clipboard permissions.', 'Publishing');
    }
  }
</script>

<div class="space-y-1">
  <h3 class="font-medium text-sm">Publish as a site</h3>
  {#if siteUrl}
    <div class="flex items-center gap-1.5">
      <p class="text-xs text-muted-foreground font-mono truncate flex-1">{siteUrl}</p>
      <Button
        variant="ghost"
        size="icon"
        class="size-6 shrink-0"
        onclick={copyUrl}
        aria-label="Copy site URL"
      >
        {#if copied}
          <Check class="size-3" />
        {:else}
          <Copy class="size-3" />
        {/if}
      </Button>
    </div>
  {/if}
</div>
