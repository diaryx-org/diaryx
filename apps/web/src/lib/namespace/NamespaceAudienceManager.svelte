<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import NativeSelect from '$lib/components/ui/native-select/native-select.svelte';
  import { getAudienceColorStore } from '$lib/stores/audienceColorStore.svelte';
  import { getAudienceColor } from '$lib/utils/audienceDotColor';
  import {
    Globe,
    KeyRound,
    Lock,
    Loader2,
    Settings2,
    Check,
    Copy,
  } from '@lucide/svelte';
  import { showError, showSuccess, showInfo } from '@/models/services/toastService';
  import * as namespaceService from './namespaceService';

  interface Props {
    namespaceId: string;
    audiences: string[];
    audienceStates: Record<string, { state: string; access_method?: string }>;
    defaultAudience: string | null;
    onStateChange: (audience: string, config: { state: string; access_method?: string }) => void;
  }

  let { namespaceId, audiences, audienceStates, defaultAudience, onStateChange }: Props = $props();

  const colorStore = getAudienceColorStore();

  // Access control dialog state
  let accessDialogOpen = $state(false);
  let accessDialogAudience = $state<string | null>(null);
  let accessDialogState = $state<string>('unpublished');
  let accessDialogMethod = $state<string>('access-key');

  // Token generation
  let isCreatingToken = $state(false);
  let lastCreatedAccessUrl = $state<string | null>(null);
  let copiedAccessUrl = $state(false);

  function getAudienceState(audience: string) {
    return audienceStates[audience] ?? { state: 'unpublished' };
  }

  function isDefaultOnly(audience: string): boolean {
    return audience === defaultAudience && !audiences.includes(audience);
  }

  function openAccessDialog(audience: string) {
    const config = getAudienceState(audience);
    accessDialogAudience = audience;
    accessDialogState = config.state;
    accessDialogMethod = config.access_method ?? 'access-key';
    accessDialogOpen = true;
    lastCreatedAccessUrl = null;
  }

  async function handleSaveAccessDialog() {
    if (!accessDialogAudience) return;
    const config = {
      state: accessDialogState,
      access_method: accessDialogState === 'access-control' ? accessDialogMethod : undefined,
    };
    try {
      const access = accessDialogState === 'public' ? 'public'
        : accessDialogState === 'access-control' ? 'token'
        : 'private';
      // Only sync to server if namespace is configured
      if (namespaceId) {
        await namespaceService.setAudience(namespaceId, accessDialogAudience, access);
      }
      onStateChange(accessDialogAudience, config);
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to save audience state', 'Publishing');
    }
    accessDialogOpen = false;
  }

  async function handleCreateToken() {
    if (!accessDialogAudience) return;
    isCreatingToken = true;
    try {
      const result = await namespaceService.getAudienceToken(namespaceId, accessDialogAudience);
      lastCreatedAccessUrl = namespaceService.buildAccessUrl(
        namespaceId,
        accessDialogAudience,
        result.token,
      );
      showSuccess('Access link generated');
      showInfo('Copy the access URL now. It is only shown once.');
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to create token', 'Publishing');
    } finally {
      isCreatingToken = false;
    }
  }

  async function copyToClipboard(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      copiedAccessUrl = true;
      setTimeout(() => { copiedAccessUrl = false; }, 1800);
    } catch {
      showError('Copy failed. Check browser clipboard permissions.', 'Publishing');
    }
  }
</script>

<div class="space-y-1.5">
  <div class="flex items-center justify-between">
    <p class="text-xs font-medium text-muted-foreground">Audience tags</p>
  </div>

  <div class="space-y-1">
    {#each audiences as audience}
      {@const config = getAudienceState(audience)}
      {@const dotColor = getAudienceColor(audience, colorStore.audienceColors)}
      {@const isDefault = isDefaultOnly(audience)}
      <button
        class="w-full flex items-center gap-2 px-2.5 py-2 rounded-md border border-border bg-background hover:bg-secondary transition-colors text-left"
        onclick={() => openAccessDialog(audience)}
      >
        <span class="size-2.5 rounded-full shrink-0 {dotColor}"></span>
        <span class="text-sm font-medium flex-1 truncate">
          {audience}
          {#if isDefault}
            <span class="text-xs font-normal text-muted-foreground">(default)</span>
          {/if}
        </span>
        <span class="text-xs text-muted-foreground flex items-center gap-1">
          {#if config.state === 'public'}
            <Globe class="size-3" />
            Public
          {:else if config.state === 'access-control'}
            <Lock class="size-3" />
            Access Key
          {:else}
            <span class="text-muted-foreground/60">Unpublished</span>
          {/if}
        </span>
        <Settings2 class="size-3.5 text-muted-foreground/50" />
      </button>
    {/each}
  </div>
</div>

<!-- Access control dialog -->
<Dialog.Root bind:open={accessDialogOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2 text-base">
        {#if accessDialogAudience}
          {@const dotColor = getAudienceColor(accessDialogAudience, colorStore.audienceColors)}
          <span class="size-2.5 rounded-full {dotColor}"></span>
        {/if}
        {accessDialogAudience}
      </Dialog.Title>
      <Dialog.Description class="text-xs text-muted-foreground">
        Configure how this audience tag is published.
      </Dialog.Description>
    </Dialog.Header>

    <div class="space-y-3 py-2">
      <div class="space-y-2">
        <button
          class="w-full flex items-center gap-3 px-3 py-2.5 rounded-md border text-left transition-colors {accessDialogState === 'unpublished' ? 'border-primary bg-secondary' : 'border-border hover:bg-secondary'}"
          onclick={() => { accessDialogState = 'unpublished'; }}
        >
          <div class="flex-1">
            <p class="text-sm font-medium">Unpublished</p>
            <p class="text-xs text-muted-foreground">This audience is not included when publishing.</p>
          </div>
        </button>

        <button
          class="w-full flex items-center gap-3 px-3 py-2.5 rounded-md border text-left transition-colors {accessDialogState === 'public' ? 'border-primary bg-secondary' : 'border-border hover:bg-secondary'}"
          onclick={() => { accessDialogState = 'public'; }}
        >
          <Globe class="size-4 text-muted-foreground shrink-0" />
          <div class="flex-1">
            <p class="text-sm font-medium">Public</p>
            <p class="text-xs text-muted-foreground">Anyone with the link can view.</p>
          </div>
        </button>

        <button
          class="w-full flex items-center gap-3 px-3 py-2.5 rounded-md border text-left transition-colors {accessDialogState === 'access-control' ? 'border-primary bg-secondary' : 'border-border hover:bg-secondary'}"
          onclick={() => { accessDialogState = 'access-control'; }}
        >
          <Lock class="size-4 text-muted-foreground shrink-0" />
          <div class="flex-1">
            <p class="text-sm font-medium">Access Control</p>
            <p class="text-xs text-muted-foreground">Restrict access with a key link.</p>
          </div>
        </button>
      </div>

      {#if accessDialogState === 'access-control'}
        <div class="space-y-3 p-3 rounded-md bg-secondary border border-border">
          <div class="space-y-1.5">
            <label for="access-method" class="text-xs font-medium text-muted-foreground">Method</label>
            <NativeSelect id="access-method" bind:value={accessDialogMethod} class="w-full h-8 text-xs">
              <option value="access-key">Access Key Link</option>
            </NativeSelect>
          </div>

          {#if accessDialogMethod === 'access-key' && namespaceId}
            <div class="space-y-2">
              <Button
                variant="secondary"
                size="sm"
                class="w-full h-8 text-xs"
                onclick={handleCreateToken}
                disabled={isCreatingToken}
              >
                {#if isCreatingToken}
                  <Loader2 class="size-3.5 mr-1 animate-spin" />
                {:else}
                  <KeyRound class="size-3.5 mr-1" />
                {/if}
                Generate Access Link
              </Button>

              {#if lastCreatedAccessUrl}
                <div class="py-2 border border-primary/30 bg-secondary rounded-md px-3">
                  <div class="text-xs space-y-2">
                    <p class="font-medium text-foreground">Access URL (shown once)</p>
                    <code class="block text-[11px] break-all bg-background rounded p-2 border border-border">{lastCreatedAccessUrl}</code>
                    <div class="flex gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        class="h-7 text-xs"
                        onclick={() => copyToClipboard(lastCreatedAccessUrl!)}
                      >
                        {#if copiedAccessUrl}
                          <Check class="size-3.5 mr-1" /> Copied
                        {:else}
                          <Copy class="size-3.5 mr-1" /> Copy URL
                        {/if}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-7 text-xs"
                        onclick={() => { lastCreatedAccessUrl = null; }}
                      >
                        Dismiss
                      </Button>
                    </div>
                  </div>
                </div>
              {/if}
            </div>
          {:else if accessDialogMethod === 'access-key' && !namespaceId}
            <p class="text-xs text-muted-foreground">Publish the site first to generate access links.</p>
          {/if}
        </div>
      {/if}
    </div>

    <Dialog.Footer>
      <Button variant="outline" size="sm" onclick={() => { accessDialogOpen = false; }}>
        Cancel
      </Button>
      <Button size="sm" onclick={handleSaveAccessDialog}>
        Save
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
