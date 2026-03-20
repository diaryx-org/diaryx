<script lang="ts">
  import * as Alert from '$lib/components/ui/alert';
  import { Button } from '$lib/components/ui/button';
  import UpgradeBanner from '$lib/components/UpgradeBanner.svelte';
  import { AlertCircle, Globe, Loader2 } from '@lucide/svelte';
  import { getNamespaceContext } from './namespaceContext.svelte';

  const ctx = getNamespaceContext();

  // Trigger loading when api and rootPath are both available.
  // tryLoad() reads ctx.api and ctx.rootPath (both $state), so
  // Svelte re-runs this effect when init() sets api.
  $effect(() => {
    ctx.tryLoad();
  });

  // Reload audiences when rootPath or audiences version changes
  $effect(() => {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    ctx.templateContextStore.audiencesVersion;
    if (ctx.rootPath) ctx.loadAudiences();
  });

  // Load workspace config when rootPath becomes available
  $effect(() => {
    if (ctx.rootPath) ctx.configStore.load(ctx.rootPath);
  });
</script>

{#if ctx.error}
  <Alert.Root variant="destructive" class="py-2">
    <AlertCircle class="size-4" />
    <Alert.Description class="text-xs">{ctx.error}</Alert.Description>
  </Alert.Root>
{/if}

{#if !ctx.hasDefaultWorkspace}
  <div class="text-center space-y-1 py-8">
    <Globe class="size-8 mx-auto text-muted-foreground" />
    <h3 class="font-medium text-sm">Publishing Unavailable</h3>
    <p class="text-xs text-muted-foreground">
      Sign in and ensure your default workspace is available.
    </p>
  </div>
{:else if ctx.isLoading}
  <div class="flex items-center justify-center py-8">
    <Loader2 class="size-5 animate-spin text-muted-foreground" />
  </div>
{:else if !ctx.isAuthenticated}
  <div class="text-center space-y-3 py-8">
    <Globe class="size-8 mx-auto text-muted-foreground" />
    <div class="space-y-1">
      <h3 class="font-medium text-sm">Sign in to publish</h3>
      <p class="text-xs text-muted-foreground">
        Publish your workspace as a site with audience-based access control.
      </p>
    </div>
    <Button variant="outline" size="sm" onclick={() => ctx.handleOpenSyncSetup()}>
      Open Account Setup
    </Button>
  </div>
{:else if !ctx.hasPublishingAccess}
  <UpgradeBanner
    feature="Publishing"
    description="This account does not include website publishing."
    icon={Globe}
  />
{/if}
