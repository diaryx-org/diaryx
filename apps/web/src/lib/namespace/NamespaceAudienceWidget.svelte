<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import {
    Globe,
    Settings2,
    ShieldOff,
    Tags,
  } from '@lucide/svelte';
  import { getNamespaceContext } from './namespaceContext.svelte';
  import NamespaceAudienceManager from './NamespaceAudienceManager.svelte';

  interface Props {
    api?: import('$lib/backend/api').Api | null;
  }

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  let { api: _api = null }: Props = $props();

  const ctx = getNamespaceContext();

  function openAudienceManager() {
    ctx.hostAction?.({ type: 'open-audience-manager' });
  }
</script>

{#if ctx.isReady}
  {#if !ctx.hasAnyAudience}
    <!-- Private workspace: no audiences -->
    <div class="space-y-3 p-3 rounded-md bg-secondary border border-border">
      <div class="flex items-start gap-2.5">
        <ShieldOff class="size-5 text-muted-foreground shrink-0 mt-0.5" />
        <div class="space-y-1">
          <p class="text-sm font-medium">Your workspace is private</p>
          <p class="text-xs text-muted-foreground">
            To share your workspace, add audience tags to your entries or set a default audience for the entire workspace.
          </p>
        </div>
      </div>

      <div class="flex flex-wrap gap-2">
        <Button
          variant="outline"
          size="sm"
          class="text-xs"
          onclick={openAudienceManager}
        >
          <Tags class="size-3.5 mr-1.5" />
          Add audience tags
        </Button>
        <Button
          variant="outline"
          size="sm"
          class="text-xs"
          onclick={() => { ctx.showDefaultAudienceInput = !ctx.showDefaultAudienceInput; }}
        >
          <Globe class="size-3.5 mr-1.5" />
          Set default audience
        </Button>
      </div>

      {#if ctx.showDefaultAudienceInput}
        <div class="flex gap-2">
          <Input
            type="text"
            bind:value={ctx.defaultAudienceInput}
            placeholder="e.g. public, family, friends"
            class="h-8 text-xs flex-1"
            onkeydown={(e) => { if (e.key === 'Enter') ctx.handleSetDefaultAudience(); }}
          />
          <Button
            variant="default"
            size="sm"
            class="h-8 text-xs shrink-0"
            onclick={() => ctx.handleSetDefaultAudience()}
            disabled={ctx.defaultAudienceInput.trim().length === 0}
          >
            Save
          </Button>
        </div>
      {/if}
    </div>
  {:else}
    <!-- Manage audiences button -->
    <div class="flex items-center justify-end">
      <Button
        variant="ghost"
        size="icon"
        class="size-6"
        onclick={openAudienceManager}
        aria-label="Manage audiences"
      >
        <Settings2 class="size-3.5" />
      </Button>
    </div>

    <NamespaceAudienceManager
      namespaceId={ctx.namespaceId ?? ''}
      audiences={ctx.allAudiences}
      audienceStates={ctx.audienceStates}
      defaultAudience={ctx.defaultAudience}
      subdomain={ctx.subdomain}
      siteBaseUrl={ctx.siteBaseUrl}
      siteDomain={ctx.siteDomain}
      onStateChange={(aud, config) => ctx.handleAudienceStateChange(aud, config)}
    />
  {/if}
{/if}
