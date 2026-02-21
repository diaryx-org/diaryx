<script lang="ts">
  import type { Api } from '$lib/backend/api';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Switch } from '$lib/components/ui/switch';
  import NativeSelect from '$lib/components/ui/native-select/native-select.svelte';
  import * as Alert from '$lib/components/ui/alert';
  import {
    AlertCircle,
    Check,
    Copy,
    Globe,
    KeyRound,
    Loader2,
    RefreshCw,
    Trash2,
    Upload,
  } from '@lucide/svelte';
  import { collaborationStore } from '@/models/stores/collaborationStore.svelte';
  import { workspaceStore } from '@/models/stores/workspaceStore.svelte';
  import { sitePublishingStore } from '@/models/stores/sitePublishingStore.svelte';
  import { showError, showInfo, showSuccess } from '@/models/services/toastService';
  import { getServerUrl, getAuthState, createCheckoutSession } from '$lib/auth';

  interface Props {
    onAddWorkspace?: () => void;
    api: Api | null;
  }

  let { onAddWorkspace, api }: Props = $props();

  let site = $derived(sitePublishingStore.site);
  let publishedAudiences = $derived(sitePublishingStore.audiences);
  let tokens = $derived(sitePublishingStore.tokens);
  let error = $derived(sitePublishingStore.error);

  let hasDefaultWorkspace = $derived(sitePublishingStore.hasDefaultWorkspace);
  let defaultWorkspaceId = $derived(sitePublishingStore.defaultWorkspaceId);
  let isConfigured = $derived(sitePublishingStore.isConfigured);
  let canPublish = $derived(sitePublishingStore.canPublish);
  let syncEnabled = $derived(collaborationStore.collaborationEnabled);

  let isLoading = $derived(sitePublishingStore.isLoading);
  let isCreatingSite = $derived(sitePublishingStore.isCreatingSite);
  let isDeletingSite = $derived(sitePublishingStore.isDeletingSite);
  let isPublishing = $derived(sitePublishingStore.isPublishing);
  let isCreatingToken = $derived(sitePublishingStore.isCreatingToken);
  let isRevokingToken = $derived(sitePublishingStore.isRevokingToken);
  let isRefreshingTokens = $derived(sitePublishingStore.isRefreshingTokens);
  let isSettingDomain = $derived(sitePublishingStore.isSettingDomain);
  let isRemovingDomain = $derived(sitePublishingStore.isRemovingDomain);
  let lastCreatedAccessUrl = $derived(sitePublishingStore.lastCreatedAccessUrl);

  let slug = $state('');
  let siteEnabled = $state(true);
  let autoPublish = $state(true);
  let slugError = $state<string | null>(null);
  let customDomainInput = $state('');

  let tokenAudience = $state('public');
  let tokenExpiresPreset = $state('7d');
  let availableAudiences = $state<string[]>([]);

  let copiedAccessUrl = $state(false);
  let copiedTokenId = $state<string | null>(null);
  let initializedWorkspaceId = $state<string | null>(null);

  let syncRequiredError = $derived.by(() => {
    const message = error?.toLowerCase() ?? '';
    return (
      message.includes('sync must be enabled')
      || message.includes('materialized markdown files')
    );
  });
  let publishBlockedBySync = $derived(!syncEnabled);
  let canPublishNow = $derived(canPublish && !publishBlockedBySync);
  let showSyncRequiredNotice = $derived(publishBlockedBySync || syncRequiredError);

  let inferredSiteUrl = $derived.by(() => {
    if (!site) return null;
    const base = getServerUrl()?.replace(/\/$/, '');
    return base ? `${base}/${site.slug}` : `/${site.slug}`;
  });

  $effect(() => {
    if (!defaultWorkspaceId || initializedWorkspaceId === defaultWorkspaceId) return;
    initializedWorkspaceId = defaultWorkspaceId;
    sitePublishingStore.load(defaultWorkspaceId);
  });

  // Load available audiences when site is configured
  $effect(() => {
    if (isConfigured && api && workspaceStore.tree) {
      loadAudiences();
    }
  });

  async function loadAudiences() {
    if (!api || !workspaceStore.tree) return;
    try {
      availableAudiences = await api.getAvailableAudiences(workspaceStore.tree.path);
    } catch (e) {
      console.warn('[PublishingPanel] Failed to load audiences:', e);
      availableAudiences = [];
    }
  }

  function formatUnixTimestamp(value: number | null | undefined): string {
    if (!value) return 'Never';
    return new Date(value * 1000).toLocaleString();
  }

  async function validateSlug(value: string): Promise<boolean> {
    try {
      await api?.validatePublishingSlug(value);
      slugError = null;
      return true;
    } catch (e) {
      slugError = e instanceof Error ? e.message : 'Use 3-64 lowercase letters, numbers, or hyphens.';
      return false;
    }
  }

  async function handleCreateSite() {
    const trimmedSlug = slug.trim();
    if (!(await validateSlug(trimmedSlug))) return;

    const created = await sitePublishingStore.create({
      slug: trimmedSlug,
      enabled: siteEnabled,
      auto_publish: autoPublish,
    });

    if (created) {
      slug = '';
      showSuccess('Publishing site configured', `Slug: ${created.slug}`);
    } else {
      showError(sitePublishingStore.error ?? 'Failed to create site', 'Publishing');
    }
  }

  async function handleDeleteSite() {
    if (!confirm('Delete this published site and all published artifacts?')) return;

    const deleted = await sitePublishingStore.remove();
    if (deleted) {
      showSuccess('Published site deleted');
    } else {
      showError(sitePublishingStore.error ?? 'Failed to delete site', 'Publishing');
    }
  }

  async function handlePublishNow() {
    if (publishBlockedBySync) {
      showError('Enable sync setup before publishing this workspace.', 'Publishing');
      return;
    }
    const ok = await sitePublishingStore.publishNow();
    if (ok) {
      showSuccess('Site published', `Published at ${formatUnixTimestamp(sitePublishingStore.lastPublishedAt)}`);
    } else {
      showError(sitePublishingStore.error ?? 'Failed to publish site', 'Publishing');
    }
  }

  async function handleCreateToken() {
    if (!site) return;

    const audience = tokenAudience.trim().toLowerCase();
    if (!audience) {
      showError('Audience is required.', 'Publishing');
      return;
    }

    const expiresIn = tokenExpiresPreset === 'none' ? null : tokenExpiresPreset;
    const created = await sitePublishingStore.createToken({
      audience,
      expires_in: expiresIn,
    });

    if (created) {
      showSuccess('Access token created', `Audience: ${created.audience}`);
      if (sitePublishingStore.lastCreatedAccessUrl) {
        showInfo('Copy the access URL now. It is only shown once in full.');
      }
      return;
    }

    showError(sitePublishingStore.error ?? 'Failed to create token', 'Publishing');
  }

  async function handleRevokeToken(tokenId: string) {
    if (!confirm('Revoke this token? Existing access sessions may stop after cache refresh.')) return;

    const revoked = await sitePublishingStore.revokeToken(tokenId);
    if (revoked) {
      showSuccess('Token revoked');
    } else {
      showError(sitePublishingStore.error ?? 'Failed to revoke token', 'Publishing');
    }
  }

  async function handleRefreshTokens() {
    await sitePublishingStore.refreshTokens();
  }

  function handleOpenSyncSetup() {
    if (onAddWorkspace) {
      onAddWorkspace();
      return;
    }
    showInfo('Open Sync settings to complete setup before publishing.');
  }

  async function handleSetDomain() {
    const domain = customDomainInput.trim().toLowerCase();
    if (!domain) return;

    const ok = await sitePublishingStore.setDomain(domain);
    if (ok) {
      customDomainInput = '';
      showSuccess('Custom domain set', domain);
    } else {
      showError(sitePublishingStore.error ?? 'Failed to set custom domain', 'Publishing');
    }
  }

  async function handleRemoveDomain() {
    if (!confirm('Remove custom domain from this site?')) return;

    const ok = await sitePublishingStore.removeDomain();
    if (ok) {
      showSuccess('Custom domain removed');
    } else {
      showError(sitePublishingStore.error ?? 'Failed to remove custom domain', 'Publishing');
    }
  }

  async function copyText(value: string, mode: 'access-url' | 'token-id', tokenId?: string) {
    try {
      await navigator.clipboard.writeText(value);
      if (mode === 'access-url') {
        copiedAccessUrl = true;
        setTimeout(() => {
          copiedAccessUrl = false;
        }, 1800);
      }
      if (mode === 'token-id' && tokenId) {
        copiedTokenId = tokenId;
        setTimeout(() => {
          copiedTokenId = null;
        }, 1800);
      }
    } catch (copyError) {
      console.error('[PublishingPanel] Failed to copy value:', copyError);
      showError('Copy failed. Check browser clipboard permissions.', 'Publishing');
    }
  }
</script>

<div class="p-3 space-y-4">
  {#if error}
    <Alert.Root variant="destructive" class="py-2">
      <AlertCircle class="size-4" />
      <Alert.Description class="text-xs">{error}</Alert.Description>
    </Alert.Root>
  {/if}

  {#if !hasDefaultWorkspace}
    <div class="text-center space-y-1 py-8">
      <Globe class="size-8 mx-auto text-muted-foreground" />
      <h3 class="font-medium text-sm">Publishing Unavailable</h3>
      <p class="text-xs text-muted-foreground">
        Sign in and ensure your default workspace is available.
      </p>
    </div>
  {:else if isLoading && !isConfigured}
    <div class="flex items-center justify-center py-8">
      <Loader2 class="size-5 animate-spin text-muted-foreground" />
    </div>
  {:else if !isConfigured && getAuthState().tier !== "plus"}
    <div class="text-center space-y-3 py-8">
      <Globe class="size-8 mx-auto text-muted-foreground" />
      <h3 class="font-medium text-sm">Publishing Requires Plus</h3>
      <p class="text-xs text-muted-foreground">
        Upgrade to publish your workspace as a website.
      </p>
      <Button
        variant="default"
        size="sm"
        onclick={async () => {
          try {
            const url = await createCheckoutSession();
            window.location.href = url;
          } catch {
            // handled by auth layer
          }
        }}
      >
        Upgrade to Plus — $5/month
      </Button>
    </div>
  {:else if !isConfigured}
    <div class="space-y-3">
      <div class="text-center space-y-1">
        <Globe class="size-8 mx-auto text-muted-foreground" />
        <h3 class="font-medium text-sm">Configure Publishing</h3>
        <p class="text-xs text-muted-foreground">
          Create a site slug to publish your default workspace.
        </p>
      </div>

      <div class="space-y-3 p-3 rounded-md bg-muted/50 border border-border">
        <div class="space-y-1.5">
          <label for="site-slug" class="text-xs font-medium text-muted-foreground">Site slug</label>
          <Input
            id="site-slug"
            type="text"
            bind:value={slug}
            placeholder="my-site"
            class="h-9 text-sm"
            oninput={() => validateSlug(slug.trim())}
          />
          {#if slugError}
            <p class="text-[11px] text-destructive">{slugError}</p>
          {:else}
            <p class="text-[11px] text-muted-foreground">Only lowercase letters, numbers, and hyphens.</p>
          {/if}
        </div>

        <div class="flex items-center justify-between">
          <span class="text-xs font-medium">Site enabled</span>
          <Switch bind:checked={siteEnabled} />
        </div>

        <div class="flex items-center justify-between">
          <span class="text-xs font-medium">Auto-publish after sync commits</span>
          <Switch bind:checked={autoPublish} />
        </div>
      </div>

      <Button
        class="w-full"
        onclick={handleCreateSite}
        disabled={isCreatingSite || slug.trim().length === 0}
      >
        {#if isCreatingSite}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Creating site...
        {:else}
          <Globe class="size-4 mr-2" />
          Create Site
        {/if}
      </Button>
    </div>
  {:else}
    {#if site}
      <div class="space-y-4">
      <div class="space-y-2 p-3 rounded-md bg-muted/50 border border-border">
        <div class="flex items-center justify-between gap-2">
          <h3 class="text-sm font-medium">Published Site</h3>
          <span class="text-xs font-mono px-2 py-1 rounded bg-background border border-border">/{site.slug}</span>
        </div>

        <div class="text-xs text-muted-foreground space-y-1">
          <div class="flex justify-between gap-2">
            <span>Last published</span>
            <span class="font-medium text-foreground">{formatUnixTimestamp(site.last_published_at)}</span>
          </div>
          <div class="flex justify-between gap-2">
            <span>URL</span>
            <span class="font-mono text-[11px] text-foreground truncate max-w-[180px]" title={inferredSiteUrl ?? ''}>
              {inferredSiteUrl}
            </span>
          </div>
        </div>

        {#if publishedAudiences.length > 0}
          <div class="space-y-1">
            <p class="text-xs font-medium text-muted-foreground">Audience builds</p>
            <div class="space-y-1">
              {#each publishedAudiences as audience}
                <div class="flex justify-between text-xs border border-border rounded px-2 py-1 bg-background">
                  <span class="font-medium">{audience.name}</span>
                  <span class="text-muted-foreground">{audience.file_count} files</span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <!-- Custom Domain -->
      <div class="space-y-2 p-3 rounded-md bg-muted/50 border border-border">
        <h4 class="text-sm font-medium">Custom Domain</h4>
        {#if site.custom_domain}
          <div class="flex items-center justify-between gap-2">
            <span class="text-xs font-mono text-foreground truncate" title={site.custom_domain}>
              {site.custom_domain}
            </span>
            <Button
              variant="ghost"
              size="sm"
              class="h-7 text-xs text-destructive hover:text-destructive"
              onclick={handleRemoveDomain}
              disabled={isRemovingDomain}
            >
              {#if isRemovingDomain}
                <Loader2 class="size-3.5 animate-spin" />
              {:else}
                <Trash2 class="size-3.5 mr-1" />
                Remove
              {/if}
            </Button>
          </div>
          <p class="text-[11px] text-muted-foreground">
            Point a CNAME to <code class="text-[10px]">site.diaryx.org</code>, or for apex domains, set an A record to your server IP.
          </p>
        {:else}
          <div class="flex gap-2">
            <Input
              type="text"
              bind:value={customDomainInput}
              placeholder="blog.example.com"
              class="h-8 text-xs flex-1"
            />
            <Button
              variant="secondary"
              size="sm"
              class="h-8 text-xs shrink-0"
              onclick={handleSetDomain}
              disabled={isSettingDomain || customDomainInput.trim().length === 0}
            >
              {#if isSettingDomain}
                <Loader2 class="size-3.5 mr-1 animate-spin" />
              {:else}
                <Globe class="size-3.5 mr-1" />
              {/if}
              Set
            </Button>
          </div>
          <p class="text-[11px] text-muted-foreground">
            After setting, point a CNAME to <code class="text-[10px]">site.diaryx.org</code>. For apex domains, use an A record.
          </p>
        {/if}
      </div>

      {#if showSyncRequiredNotice}
        <Alert.Root class="py-2 border border-primary/30 bg-primary/5">
          <AlertCircle class="size-4 text-primary" />
          <Alert.Description class="text-xs space-y-2">
            <p class="font-medium text-foreground">Sync setup required for publishing</p>
            <p class="text-muted-foreground">
              Publishing runs from server CRDT state. Enable sync and complete setup at least once for this workspace.
            </p>
            <Button
              variant="outline"
              size="sm"
              class="h-7 text-xs"
              onclick={handleOpenSyncSetup}
            >
              Open Sync Setup
            </Button>
          </Alert.Description>
        </Alert.Root>
      {/if}

      <div class="flex gap-2">
        <Button
          class="flex-1"
          variant="default"
          onclick={handlePublishNow}
          disabled={!canPublishNow}
        >
          {#if isPublishing}
            <Loader2 class="size-4 mr-2 animate-spin" />
            Publishing...
          {:else}
            <Upload class="size-4 mr-2" />
            Publish Now
          {/if}
        </Button>
        <Button
          class="shrink-0"
          variant="destructive"
          onclick={handleDeleteSite}
          disabled={isDeletingSite}
        >
          {#if isDeletingSite}
            <Loader2 class="size-4 animate-spin" />
          {:else}
            <Trash2 class="size-4" />
          {/if}
        </Button>
      </div>

      <div class="space-y-3 p-3 rounded-md bg-muted/50 border border-border">
        <div class="flex items-center justify-between">
          <h4 class="text-sm font-medium">Access Tokens</h4>
          <Button
            variant="ghost"
            size="icon"
            class="size-7"
            onclick={handleRefreshTokens}
            disabled={isRefreshingTokens || isCreatingToken || isRevokingToken}
          >
            <RefreshCw class="size-3.5 {isRefreshingTokens ? 'animate-spin' : ''}" />
          </Button>
        </div>

        <div class="space-y-2">
          <div class="space-y-1.5">
            <label for="token-audience" class="text-xs font-medium text-muted-foreground">Audience</label>
            <NativeSelect id="token-audience" bind:value={tokenAudience} class="w-full h-8 text-xs">
              <option value="public">public</option>
              {#each availableAudiences.filter(a => a !== 'public') as aud}
                <option value={aud}>{aud}</option>
              {/each}
            </NativeSelect>
          </div>

          <div class="space-y-1.5">
            <label for="token-expires" class="text-xs font-medium text-muted-foreground">Expires</label>
            <NativeSelect id="token-expires" bind:value={tokenExpiresPreset} class="w-full h-8 text-xs">
              <option value="none">Never</option>
              <option value="10m">10 minutes</option>
              <option value="1d">1 day</option>
              <option value="7d">7 days</option>
              <option value="30d">30 days</option>
            </NativeSelect>
          </div>

          <Button
            class="w-full"
            variant="secondary"
            onclick={handleCreateToken}
            disabled={isCreatingToken || !site}
          >
            {#if isCreatingToken}
              <Loader2 class="size-4 mr-2 animate-spin" />
              Creating token...
            {:else}
              <KeyRound class="size-4 mr-2" />
              Create Token
            {/if}
          </Button>
        </div>

        {#if lastCreatedAccessUrl}
          <Alert.Root class="py-2 border border-primary/30 bg-primary/5">
            <Alert.Description class="text-xs space-y-2">
              <p class="font-medium text-foreground">One-time access URL</p>
              <code class="block text-[11px] break-all bg-background rounded p-2 border border-border">{lastCreatedAccessUrl}</code>
              <div class="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  class="h-7 text-xs"
                  onclick={() => copyText(lastCreatedAccessUrl, 'access-url')}
                >
                  {#if copiedAccessUrl}
                    <Check class="size-3.5 mr-1" />
                    Copied
                  {:else}
                    <Copy class="size-3.5 mr-1" />
                    Copy URL
                  {/if}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-7 text-xs"
                  onclick={() => sitePublishingStore.clearLastCreatedAccessUrl()}
                >
                  Dismiss
                </Button>
              </div>
            </Alert.Description>
          </Alert.Root>
        {/if}

        {#if tokens.length === 0}
          <p class="text-xs text-muted-foreground">No active tokens.</p>
        {:else}
          <div class="space-y-2">
            {#each tokens as token (token.id)}
              <div class="rounded-md border border-border bg-background px-2 py-2">
                <div class="flex items-start justify-between gap-2">
                  <div class="min-w-0">
                    <p class="text-xs font-medium truncate">{token.audience}</p>
                    <p class="text-[11px] text-muted-foreground">
                      Expires: {formatUnixTimestamp(token.expires_at)}
                    </p>
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    <Button
                      variant="ghost"
                      size="icon"
                      class="size-6"
                      onclick={() => copyText(token.id, 'token-id', token.id)}
                      aria-label="Copy token ID"
                    >
                      {#if copiedTokenId === token.id}
                        <Check class="size-3.5" />
                      {:else}
                        <Copy class="size-3.5" />
                      {/if}
                    </Button>
                    <Button
                      variant="destructive"
                      size="icon"
                      class="size-6"
                      onclick={() => handleRevokeToken(token.id)}
                      disabled={isRevokingToken}
                      aria-label="Revoke token"
                    >
                      <Trash2 class="size-3.5" />
                    </Button>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
      </div>
    {:else}
      <div class="flex items-center justify-center py-8">
        <Loader2 class="size-5 animate-spin text-muted-foreground" />
      </div>
    {/if}
  {/if}
</div>
