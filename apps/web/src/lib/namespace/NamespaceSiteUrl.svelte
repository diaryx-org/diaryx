<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import { Check, Copy } from '@lucide/svelte';
  import { showError } from '@/models/services/toastService';

  interface Props {
    namespaceId: string | null;
    subdomain: string | null;
    audienceStates: Record<string, { state: string }>;
    siteBaseUrl?: string | null;
    siteDomain?: string | null;
  }

  let { namespaceId, subdomain, audienceStates, siteBaseUrl = null, siteDomain = null }: Props = $props();
  let copied = $state(false);

  let siteUrl = $derived.by(() => {
    if (!namespaceId) return null;
    const firstPublished = Object.entries(audienceStates).find(([, c]) => c.state !== 'unpublished');
    const audience = firstPublished?.[0];

    if (subdomain && siteDomain) {
      // Subdomain routing
      return audience
        ? `https://${subdomain}.${siteDomain}/${audience}/index.html`
        : `https://${subdomain}.${siteDomain}`;
    }
    if (siteBaseUrl) {
      // Direct serving from sync server
      return audience
        ? `${siteBaseUrl}/sites/${namespaceId}/${audience}/index.html`
        : `${siteBaseUrl}/sites/${namespaceId}`;
    }
    // Fallback (shouldn't normally render)
    return audience
      ? `/sites/${namespaceId}/${audience}/index.html`
      : null;
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
