<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Globe, Loader2, Trash2, Plus, Check, Copy } from '@lucide/svelte';
  import { showError, showSuccess } from '@/models/services/toastService';
  import * as namespaceService from './namespaceService';
  import type { DomainInfo } from './namespaceService';

  interface Props {
    namespaceId: string;
  }

  let { namespaceId }: Props = $props();

  let cnameTarget = 'publish.diaryx.org';

  let domains = $state<DomainInfo[]>([]);
  let isLoading = $state(false);
  let showAddForm = $state(false);
  let domainInput = $state('');
  let audienceInput = $state('');
  let isAdding = $state(false);
  let copiedCname = $state(false);

  $effect(() => {
    if (namespaceId) loadDomains();
  });

  async function loadDomains() {
    isLoading = true;
    try {
      domains = await namespaceService.listDomains(namespaceId);
    } catch {
      domains = [];
    } finally {
      isLoading = false;
    }
  }

  async function handleAdd() {
    const domain = domainInput.trim().toLowerCase();
    const audience = audienceInput.trim();
    if (!domain || !audience) return;
    isAdding = true;
    try {
      await namespaceService.registerDomain(namespaceId, domain, audience);
      showAddForm = false;
      domainInput = '';
      audienceInput = '';
      showSuccess(`Domain registered: ${domain}`);
      await loadDomains();
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to register domain', 'Publishing');
    } finally {
      isAdding = false;
    }
  }

  async function handleRemove(domain: string) {
    try {
      await namespaceService.removeDomain(namespaceId, domain);
      showSuccess(`Domain removed: ${domain}`);
      await loadDomains();
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to remove domain', 'Publishing');
    }
  }

  async function copyCnameTarget() {
    if (!cnameTarget) return;
    try {
      await navigator.clipboard.writeText(cnameTarget);
      copiedCname = true;
      setTimeout(() => { copiedCname = false; }, 1800);
    } catch { /* ignore */ }
  }
</script>

<div class="space-y-2">
  <div class="flex items-center justify-between">
    <p class="text-xs font-medium text-muted-foreground">Custom domains</p>
    <Button
      variant="ghost"
      size="icon"
      class="size-6"
      onclick={() => { showAddForm = !showAddForm; }}
      aria-label="Add custom domain"
    >
      <Plus class="size-3.5" />
    </Button>
  </div>

  {#if isLoading}
    <div class="flex items-center justify-center py-2">
      <Loader2 class="size-4 animate-spin text-muted-foreground" />
    </div>
  {:else if domains.length === 0 && !showAddForm}
    <p class="text-xs text-muted-foreground">No custom domains configured.</p>
  {:else}
    <div class="space-y-1">
      {#each domains as domain}
        <div class="flex items-center gap-2 px-2.5 py-1.5 rounded-md border border-border bg-background text-xs">
          <Globe class="size-3 text-muted-foreground shrink-0" />
          <span class="font-mono flex-1 truncate">{domain.domain}</span>
          <span class="text-muted-foreground">{domain.audience_name}</span>
          <Button
            variant="ghost"
            size="icon"
            class="size-5 text-muted-foreground"
            onclick={() => handleRemove(domain.domain)}
            aria-label="Remove domain"
          >
            <Trash2 class="size-3" />
          </Button>
        </div>
      {/each}
    </div>
  {/if}

  {#if showAddForm}
    <div class="space-y-2 p-2.5 rounded-md border border-border bg-secondary">
      <Input
        type="text"
        bind:value={domainInput}
        placeholder="example.com"
        class="h-7 text-xs font-mono"
      />
      <Input
        type="text"
        bind:value={audienceInput}
        placeholder="Audience name"
        class="h-7 text-xs"
      />

      <!-- DNS instructions -->
      <div class="text-xs space-y-1.5 text-muted-foreground">
        <p>Add a <span class="font-medium text-foreground">CNAME</span> record with your DNS provider:</p>
        <div class="flex items-center gap-1.5 font-mono text-[11px] bg-background rounded px-2 py-1.5 border border-border">
          <span class="min-w-0 break-all flex-1">{domainInput.trim() || 'example.com'} <span class="text-muted-foreground/60">&#8594;</span> {cnameTarget}</span>
          <Button
            variant="ghost"
            size="icon"
            class="size-5 shrink-0"
            onclick={copyCnameTarget}
            aria-label="Copy CNAME target"
          >
            {#if copiedCname}
              <Check class="size-3" />
            {:else}
              <Copy class="size-3" />
            {/if}
          </Button>
        </div>
        <p>DNS changes may take a few minutes to propagate. TLS certificates are provisioned automatically.</p>
      </div>

      <div class="flex gap-2">
        <Button
          variant="default"
          size="sm"
          class="h-7 text-xs"
          onclick={handleAdd}
          disabled={!domainInput.trim() || !audienceInput.trim() || isAdding}
        >
          {#if isAdding}
            <Loader2 class="size-3 mr-1 animate-spin" />
          {/if}
          Add Domain
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="h-7 text-xs"
          onclick={() => { showAddForm = false; }}
        >
          Cancel
        </Button>
      </div>
    </div>
  {/if}
</div>
