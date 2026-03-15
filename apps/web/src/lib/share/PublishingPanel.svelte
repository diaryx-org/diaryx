<script lang="ts">
  import type { Api } from '$lib/backend/api';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Alert from '$lib/components/ui/alert';
  import * as Dialog from '$lib/components/ui/dialog';
  import NativeSelect from '$lib/components/ui/native-select/native-select.svelte';
  import { getTemplateContextStore } from '$lib/stores/templateContextStore.svelte';
  import { getAudienceColorStore } from '$lib/stores/audienceColorStore.svelte';
  import { getAudienceColor } from '$lib/utils/audienceDotColor';
  import { getWorkspaceConfigStore } from '$lib/stores/workspaceConfigStore.svelte';
  import { workspaceStore } from '@/models/stores';
  import ManageAudiencesModal from '$lib/components/ManageAudiencesModal.svelte';
  import {
    AlertCircle,
    Check,
    Copy,
    Globe,
    KeyRound,
    Loader2,
    Lock,
    Settings2,
    ShieldOff,
    Tags,
    Trash2,
    Upload,
  } from '@lucide/svelte';
  import { sitePublishingStore } from '@/models/stores/sitePublishingStore.svelte';
  import { showError, showSuccess, showInfo } from '@/models/services/toastService';
  import { getAuthState } from '$lib/auth';
  import UpgradeBanner from '$lib/components/UpgradeBanner.svelte';

  interface Props {
    onAddWorkspace?: () => void;
    api: Api | null;
  }

  let { onAddWorkspace, api }: Props = $props();

  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();
  const configStore = getWorkspaceConfigStore();

  let site = $derived(sitePublishingStore.site);
  let error = $derived(sitePublishingStore.error);
  let hasDefaultWorkspace = $derived(sitePublishingStore.hasDefaultWorkspace);
  let defaultWorkspaceId = $derived(sitePublishingStore.defaultWorkspaceId);
  let workspaceName = $derived(sitePublishingStore.defaultWorkspaceName);
  let isConfigured = $derived(sitePublishingStore.isConfigured);
  let authState = $derived(getAuthState());
  let isAuthenticated = $derived(authState.isAuthenticated);

  let isLoading = $derived(sitePublishingStore.isLoading);
  let isPublishing = $derived(sitePublishingStore.isPublishing);
  let isCreatingSite = $derived(sitePublishingStore.isCreatingSite);
  let isCreatingToken = $derived(sitePublishingStore.isCreatingToken);
  let isRevokingToken = $derived(sitePublishingStore.isRevokingToken);
  let lastCreatedAccessUrl = $derived(sitePublishingStore.lastCreatedAccessUrl);
  let tokens = $derived(sitePublishingStore.tokens);

  // Workspace root path from tree (for getAvailableAudiences)
  let rootPath = $derived(workspaceStore.tree?.path ?? null);
  let defaultAudience = $derived(configStore.config?.default_audience ?? null);

  // Audience states: audience name -> { state, access_method }
  type AudienceState = 'unpublished' | 'public' | 'access-control';
  interface AudienceConfig { state: AudienceState; access_method?: string }
  let audienceStates = $state<Record<string, AudienceConfig>>({});
  let availableAudiences = $state<string[]>([]);
  let initializedWorkspaceId = $state<string | null>(null);

  // Manage audiences modal
  let showManageAudiences = $state(false);

  // Set default audience inline
  let showDefaultAudienceInput = $state(false);
  let defaultAudienceInput = $state('');

  // Access control dialog state
  let accessDialogOpen = $state(false);
  let accessDialogAudience = $state<string | null>(null);
  let accessDialogState = $state<AudienceState>('unpublished');
  let accessDialogMethod = $state<string>('access-key');
  let tokenExpiresPreset = $state('7d');
  let copiedAccessUrl = $state(false);
  let copiedTokenId = $state<string | null>(null);

  let siteSlug = $derived.by(() => {
    if (site?.slug) return site.slug;
    if (!workspaceName) return null;
    return workspaceName.toLowerCase().replace(/[^a-z0-9-]/g, '-').replace(/-+/g, '-').replace(/^-|-$/g, '');
  });

  let siteUrl = $derived.by(() => {
    if (!siteSlug) return null;
    return `site.diaryx.org/${siteSlug}`;
  });

  // Combine explicit audience tags + default audience into a unified list
  let allAudiences = $derived.by(() => {
    const set = new Set(availableAudiences);
    if (defaultAudience && !set.has(defaultAudience)) {
      set.add(defaultAudience);
    }
    return [...set];
  });

  let hasAnyAudience = $derived(allAudiences.length > 0);

  let publishedAudienceCount = $derived(
    Object.values(audienceStates).filter(c => c.state !== 'unpublished').length
  );

  let hasPublishingAccess = $derived(
    isConfigured || authState.publishedSiteLimit > 0
  );

  let canPublish = $derived(
    hasDefaultWorkspace
    && !isPublishing
    && !isLoading
    && !isCreatingSite
    && isAuthenticated
    && hasPublishingAccess
    && publishedAudienceCount > 0
  );

  // Tokens for the currently-open dialog audience
  let dialogAudienceTokens = $derived(
    accessDialogAudience
      ? tokens.filter(t => t.audience === accessDialogAudience)
      : []
  );

  $effect(() => {
    if (!defaultWorkspaceId || initializedWorkspaceId === defaultWorkspaceId) return;
    initializedWorkspaceId = defaultWorkspaceId;
    sitePublishingStore.load(defaultWorkspaceId);
    loadAudienceStates();
  });

  // Load workspace config when rootPath becomes available
  $effect(() => {
    if (rootPath) configStore.load(rootPath);
  });

  // Reload audiences when rootPath or version bumps
  $effect(() => {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    templateContextStore.audiencesVersion;
    if (rootPath) loadAudiences();
  });

  async function loadAudiences() {
    if (!api || !rootPath) return;
    try {
      availableAudiences = await api.getAvailableAudiences(rootPath);
      for (const name of availableAudiences) colorStore.assignColor(name);
    } catch {
      availableAudiences = [];
    }
  }

  function audienceStatesStorageKey(): string {
    return `diaryx-audience-publish-states:${defaultWorkspaceId ?? 'default'}`;
  }

  function loadAudienceStates() {
    try {
      const raw = localStorage.getItem(audienceStatesStorageKey());
      if (raw) {
        audienceStates = JSON.parse(raw) as Record<string, AudienceConfig>;
      }
    } catch {
      audienceStates = {};
    }
  }

  function saveAudienceState(audience: string, config: AudienceConfig) {
    if (config.state === 'unpublished') {
      const { [audience]: _, ...rest } = audienceStates;
      audienceStates = rest;
    } else {
      audienceStates = { ...audienceStates, [audience]: config };
    }
    try {
      localStorage.setItem(audienceStatesStorageKey(), JSON.stringify(audienceStates));
    } catch {
      showError('Failed to save audience state', 'Publishing');
    }
  }

  function getAudienceState(audience: string): AudienceConfig {
    return audienceStates[audience] ?? { state: 'unpublished' };
  }

  function isDefaultOnly(audience: string): boolean {
    return audience === defaultAudience && !availableAudiences.includes(audience);
  }

  function openAccessDialog(audience: string) {
    const config = getAudienceState(audience);
    accessDialogAudience = audience;
    accessDialogState = config.state;
    accessDialogMethod = config.access_method ?? 'access-key';
    accessDialogOpen = true;
    sitePublishingStore.clearLastCreatedAccessUrl();
  }

  function handleSaveAccessDialog() {
    if (!accessDialogAudience) return;
    const config: AudienceConfig = {
      state: accessDialogState,
      access_method: accessDialogState === 'access-control' ? accessDialogMethod : undefined,
    };
    saveAudienceState(accessDialogAudience, config);
    accessDialogOpen = false;
  }

  async function handlePublish() {
    // Auto-create site if not configured
    if (!isConfigured && siteSlug) {
      const created = await sitePublishingStore.create({
        slug: siteSlug,
        enabled: true,
        auto_publish: true,
      });
      if (!created) {
        showError(sitePublishingStore.error ?? 'Failed to create site', 'Publishing');
        return;
      }
    }

    // Publish all non-unpublished audiences
    const audiencesToPublish = Object.entries(audienceStates)
      .filter(([, c]) => c.state !== 'unpublished')
      .map(([name]) => name);

    if (audiencesToPublish.length === 0) {
      showError('No audiences are set to publish.', 'Publishing');
      return;
    }

    // Publish each audience individually (server accepts one audience per request)
    let allOk = true;
    for (const audience of audiencesToPublish) {
      const ok = await sitePublishingStore.publishNow(audience);
      if (!ok) {
        showError(sitePublishingStore.error ?? `Failed to publish audience "${audience}"`, 'Publishing');
        allOk = false;
        break;
      }
    }
    if (allOk) {
      showSuccess('Site published');
    }
  }

  async function handleCreateToken() {
    if (!accessDialogAudience || !site) return;
    const expiresIn = tokenExpiresPreset === 'none' ? null : tokenExpiresPreset;
    const created = await sitePublishingStore.createToken({
      audience: accessDialogAudience,
      expires_in: expiresIn,
    });
    if (created) {
      showSuccess('Access token created');
      if (sitePublishingStore.lastCreatedAccessUrl) {
        showInfo('Copy the access URL now. It is only shown once.');
      }
    } else {
      showError(sitePublishingStore.error ?? 'Failed to create token', 'Publishing');
    }
  }

  async function handleRevokeToken(tokenId: string) {
    if (!confirm('Revoke this token?')) return;
    const revoked = await sitePublishingStore.revokeToken(tokenId);
    if (revoked) showSuccess('Token revoked');
    else showError(sitePublishingStore.error ?? 'Failed to revoke token', 'Publishing');
  }

  async function handleSetDefaultAudience() {
    const value = defaultAudienceInput.trim();
    if (!value) return;
    await configStore.setField('default_audience', value);
    colorStore.assignColor(value);
    showDefaultAudienceInput = false;
    defaultAudienceInput = '';
    templateContextStore.bumpAudiencesVersion();
  }

  async function copyText(value: string, mode: 'access-url' | 'token-id', tokenId?: string) {
    try {
      await navigator.clipboard.writeText(value);
      if (mode === 'access-url') {
        copiedAccessUrl = true;
        setTimeout(() => { copiedAccessUrl = false; }, 1800);
      }
      if (mode === 'token-id' && tokenId) {
        copiedTokenId = tokenId;
        setTimeout(() => { copiedTokenId = null; }, 1800);
      }
    } catch {
      showError('Copy failed. Check browser clipboard permissions.', 'Publishing');
    }
  }

  function handleOpenSyncSetup() {
    if (onAddWorkspace) {
      onAddWorkspace();
      return;
    }
    showInfo('Open account or sync settings to enable faster server-side publishing.');
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
  {:else if !isAuthenticated}
    <div class="text-center space-y-3 py-8">
      <Globe class="size-8 mx-auto text-muted-foreground" />
      <div class="space-y-1">
        <h3 class="font-medium text-sm">Sign in to publish</h3>
        <p class="text-xs text-muted-foreground">
          Publish your workspace as a site with audience-based access control.
        </p>
      </div>
      <Button variant="outline" size="sm" onclick={handleOpenSyncSetup}>
        Open Account Setup
      </Button>
    </div>
  {:else if authState.publishedSiteLimit === 0 && !isConfigured}
    <UpgradeBanner
      feature="Publishing"
      description="This account does not include website publishing."
      icon={Globe}
    />
  {:else}
    <!-- Header: Publish as a site -->
    <div class="space-y-1">
      <h3 class="font-medium text-sm">Publish as a site</h3>
      {#if siteUrl}
        <p class="text-xs text-muted-foreground font-mono">{siteUrl}</p>
      {/if}
    </div>

    <!-- Audience list or empty state -->
    {#if !hasAnyAudience}
      <!-- Private workspace: no audiences, no default audience -->
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
            onclick={() => { showManageAudiences = true; }}
          >
            <Tags class="size-3.5 mr-1.5" />
            Add audience tags
          </Button>
          <Button
            variant="outline"
            size="sm"
            class="text-xs"
            onclick={() => { showDefaultAudienceInput = !showDefaultAudienceInput; }}
          >
            <Globe class="size-3.5 mr-1.5" />
            Set default audience
          </Button>
        </div>

        {#if showDefaultAudienceInput}
          <div class="flex gap-2">
            <Input
              type="text"
              bind:value={defaultAudienceInput}
              placeholder="e.g. public, family, friends"
              class="h-8 text-xs flex-1"
              onkeydown={(e) => { if (e.key === 'Enter') handleSetDefaultAudience(); }}
            />
            <Button
              variant="default"
              size="sm"
              class="h-8 text-xs shrink-0"
              onclick={handleSetDefaultAudience}
              disabled={defaultAudienceInput.trim().length === 0}
            >
              Save
            </Button>
          </div>
        {/if}
      </div>
    {:else}
      <!-- Audience tags list -->
      <div class="space-y-1.5">
        <div class="flex items-center justify-between">
          <p class="text-xs font-medium text-muted-foreground">Audience tags</p>
          <Button
            variant="ghost"
            size="icon"
            class="size-6"
            onclick={() => { showManageAudiences = true; }}
            aria-label="Manage audiences"
          >
            <Settings2 class="size-3.5" />
          </Button>
        </div>

        <div class="space-y-1">
          {#each allAudiences as audience}
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

      <!-- Publish button -->
      <Button
        class="w-full"
        variant="default"
        onclick={handlePublish}
        disabled={!canPublish}
      >
        {#if isPublishing}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Publishing...
        {:else if isCreatingSite}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Setting up site...
        {:else}
          <Upload class="size-4 mr-2" />
          Publish
          {#if publishedAudienceCount > 0}
            ({publishedAudienceCount} {publishedAudienceCount === 1 ? 'audience' : 'audiences'})
          {/if}
        {/if}
      </Button>

      {#if site?.last_published_at}
        <p class="text-[11px] text-muted-foreground text-center">
          Last published {new Date(site.last_published_at * 1000).toLocaleString()}
        </p>
      {/if}
    {/if}
  {/if}
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
      <!-- State selector -->
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

      <!-- Access control options (shown when access-control selected) -->
      {#if accessDialogState === 'access-control'}
        <div class="space-y-3 p-3 rounded-md bg-secondary border border-border">
          <div class="space-y-1.5">
            <label for="access-method" class="text-xs font-medium text-muted-foreground">Method</label>
            <NativeSelect id="access-method" bind:value={accessDialogMethod} class="w-full h-8 text-xs">
              <option value="access-key">Access Key Link</option>
            </NativeSelect>
          </div>

          {#if accessDialogMethod === 'access-key' && site}
            <div class="space-y-2">
              <div class="flex items-center justify-between">
                <p class="text-xs font-medium text-muted-foreground">Access Tokens</p>
              </div>

              <div class="space-y-1.5">
                <label for="token-expires" class="text-xs text-muted-foreground">New token expires</label>
                <div class="flex gap-2">
                  <NativeSelect id="token-expires" bind:value={tokenExpiresPreset} class="flex-1 h-8 text-xs">
                    <option value="none">Never</option>
                    <option value="10m">10 minutes</option>
                    <option value="1d">1 day</option>
                    <option value="7d">7 days</option>
                    <option value="30d">30 days</option>
                  </NativeSelect>
                  <Button
                    variant="secondary"
                    size="sm"
                    class="h-8 text-xs shrink-0"
                    onclick={handleCreateToken}
                    disabled={isCreatingToken}
                  >
                    {#if isCreatingToken}
                      <Loader2 class="size-3.5 mr-1 animate-spin" />
                    {:else}
                      <KeyRound class="size-3.5 mr-1" />
                    {/if}
                    Create
                  </Button>
                </div>
              </div>

              {#if lastCreatedAccessUrl}
                <Alert.Root class="py-2 border border-primary/30 bg-secondary">
                  <Alert.Description class="text-xs space-y-2">
                    <p class="font-medium text-foreground">Access URL (shown once)</p>
                    <code class="block text-[11px] break-all bg-background rounded p-2 border border-border">{lastCreatedAccessUrl}</code>
                    <div class="flex gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        class="h-7 text-xs"
                        onclick={() => copyText(lastCreatedAccessUrl!, 'access-url')}
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
                        onclick={() => sitePublishingStore.clearLastCreatedAccessUrl()}
                      >
                        Dismiss
                      </Button>
                    </div>
                  </Alert.Description>
                </Alert.Root>
              {/if}

              {#if dialogAudienceTokens.length === 0}
                <p class="text-xs text-muted-foreground">No active tokens for this audience.</p>
              {:else}
                <div class="space-y-1">
                  {#each dialogAudienceTokens as token (token.id)}
                    <div class="flex items-center justify-between gap-2 rounded border border-border bg-background px-2 py-1.5">
                      <div class="min-w-0">
                        <p class="text-[11px] text-muted-foreground">
                          Expires: {token.expires_at ? new Date(token.expires_at * 1000).toLocaleDateString() : 'Never'}
                        </p>
                      </div>
                      <div class="flex items-center gap-1 shrink-0">
                        <Button
                          variant="ghost"
                          size="icon"
                          class="size-6"
                          onclick={() => copyText(token.id, 'token-id', token.id)}
                        >
                          {#if copiedTokenId === token.id}
                            <Check class="size-3" />
                          {:else}
                            <Copy class="size-3" />
                          {/if}
                        </Button>
                        <Button
                          variant="destructive"
                          size="icon"
                          class="size-6"
                          onclick={() => handleRevokeToken(token.id)}
                          disabled={isRevokingToken}
                        >
                          <Trash2 class="size-3" />
                        </Button>
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {:else if accessDialogMethod === 'access-key' && !site}
            <p class="text-xs text-muted-foreground">Publish the site first to manage access tokens.</p>
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

<!-- Manage Audiences Modal -->
{#if api && rootPath}
  <ManageAudiencesModal
    open={showManageAudiences}
    {api}
    {rootPath}
    onClose={() => { showManageAudiences = false; loadAudiences(); }}
  />
{/if}
