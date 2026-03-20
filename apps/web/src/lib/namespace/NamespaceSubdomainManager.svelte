<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Globe, Loader2 } from '@lucide/svelte';
  import { showError, showSuccess } from '@/models/services/toastService';
  import * as namespaceService from './namespaceService';

  interface Props {
    namespaceId: string;
    subdomain: string | null;
    firstPublishedAudience?: string;
    onSubdomainChange: (subdomain: string | null) => void;
  }

  let { namespaceId, subdomain, firstPublishedAudience, onSubdomainChange }: Props = $props();

  let showInput = $state(false);
  let inputValue = $state('');
  let isClaiming = $state(false);

  async function handleClaim() {
    const value = inputValue.trim().toLowerCase();
    if (!value) return;
    isClaiming = true;
    try {
      await namespaceService.claimSubdomain(namespaceId, value, firstPublishedAudience);
      onSubdomainChange(value);
      showInput = false;
      inputValue = '';
      showSuccess(`Subdomain claimed: ${value}.diaryx.org`);
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to claim subdomain', 'Publishing');
    } finally {
      isClaiming = false;
    }
  }

  async function handleRelease() {
    try {
      await namespaceService.releaseSubdomain(namespaceId);
      onSubdomainChange(null);
      showSuccess('Subdomain released');
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to release subdomain', 'Publishing');
    }
  }
</script>

{#if subdomain}
  <div class="flex items-center gap-2 text-xs">
    <span class="text-muted-foreground">Subdomain:</span>
    <span class="font-mono font-medium">{subdomain}.diaryx.org</span>
    <Button
      variant="ghost"
      size="sm"
      class="h-6 text-xs text-muted-foreground ml-auto px-2"
      onclick={handleRelease}
    >
      Release
    </Button>
  </div>
{:else if showInput}
  <div class="space-y-1.5">
    <p class="text-xs text-muted-foreground">Choose a subdomain for your site</p>
    <div class="flex gap-2 items-center">
      <Input
        type="text"
        bind:value={inputValue}
        placeholder="my-site"
        class="h-8 text-xs flex-1 font-mono"
        onkeydown={(e) => { if (e.key === 'Enter') handleClaim(); }}
      />
      <span class="text-xs text-muted-foreground shrink-0">.diaryx.org</span>
    </div>
    <div class="flex gap-2">
      <Button
        variant="default"
        size="sm"
        class="h-7 text-xs"
        onclick={handleClaim}
        disabled={inputValue.trim().length < 3 || isClaiming}
      >
        {#if isClaiming}
          <Loader2 class="size-3 mr-1 animate-spin" />
        {/if}
        Claim
      </Button>
      <Button
        variant="ghost"
        size="sm"
        class="h-7 text-xs"
        onclick={() => { showInput = false; }}
      >
        Cancel
      </Button>
    </div>
  </div>
{:else}
  <Button
    variant="outline"
    size="sm"
    class="w-full text-xs"
    onclick={() => { showInput = true; }}
  >
    <Globe class="size-3.5 mr-1.5" />
    Set custom subdomain
  </Button>
{/if}
