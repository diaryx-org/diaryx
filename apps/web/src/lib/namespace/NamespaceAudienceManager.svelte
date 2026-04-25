<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { getAudienceColorStore } from '$lib/stores/audienceColorStore.svelte';
  import { getWorkspaceConfigStore } from '$lib/stores/workspaceConfigStore.svelte';
  import { getAudienceColor } from '$lib/utils/audienceDotColor';
  import {
    Globe,
    KeyRound,
    Lock,
    Loader2,
    Settings2,
    Check,
    Copy,
    Mail,
    Link as LinkIcon,
    Shield,
    AlertTriangle,
  } from '@lucide/svelte';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import { showError, showSuccess, showInfo } from '@/models/services/toastService';
  import * as namespaceService from './namespaceService';
  import type { AudienceConfig } from './namespaceContext.svelte';
  import type { AudienceDecl, Gate, ShareAction } from '$lib/backend/generated';

  interface Props {
    namespaceId: string;
    /** Audience names sourced from entry-tag scanning (legacy display). */
    audiences: string[];
    /** Legacy plugin-config audience HashMap. Used as fallback when the
     *  workspace file does not declare `audiences:`. */
    audienceStates: Record<string, AudienceConfig>;
    defaultAudience: string | null;
    /** Server context for building access URLs. */
    subdomain?: string | null;
    siteBaseUrl?: string | null;
    siteDomain?: string | null;
    /** Callback used by the legacy audience-state UI. New file-declared
     *  audiences sync via the workspace file; this stays for back-compat. */
    onStateChange: (audience: string, config: AudienceConfig) => void;
  }

  let {
    namespaceId,
    audiences,
    audienceStates,
    defaultAudience,
    subdomain = null,
    siteBaseUrl = null,
    siteDomain = null,
    onStateChange,
  }: Props = $props();

  const colorStore = getAudienceColorStore();
  const configStore = getWorkspaceConfigStore();

  // ==========================================================================
  // Source-of-truth resolution: file declaration takes priority over the
  // legacy `audience_states` HashMap. The two paths are mutually exclusive
  // at the UI level — once an audience declaration exists in the workspace
  // file, the legacy panel hides itself.
  // ==========================================================================

  const declaredAudiences = $derived(configStore.config?.audiences ?? null);
  const audiencesMigrated = $derived(
    configStore.config?.audiences_migrated === true,
  );
  const legacyEntriesPresent = $derived(
    Object.keys(audienceStates).length > 0,
  );
  const showMigrationBanner = $derived(
    !audiencesMigrated && legacyEntriesPresent && declaredAudiences === null,
  );
  const usingFile = $derived(declaredAudiences !== null);

  // ==========================================================================
  // Password set/rotate dialog state
  // ==========================================================================

  let passwordDialogOpen = $state(false);
  let passwordDialogAudience = $state<string | null>(null);
  let passwordDialogValue = $state('');
  let passwordDialogConfirm = $state('');
  let isRotatingPassword = $state(false);

  // ==========================================================================
  // Generated link state — tracked per audience so the UI can show the URL
  // inline next to whichever audience the writer just minted a token for.
  // ==========================================================================

  const generatedLinks = $state<Record<string, string>>({});
  let creatingLinkFor = $state<string | null>(null);
  const copiedLinks = $state<Record<string, boolean>>({});

  // ==========================================================================
  // Migration state
  // ==========================================================================

  let isMigrating = $state(false);

  // ==========================================================================
  // Legacy access dialog (kept for the no-file fallback path).
  // ==========================================================================

  let accessDialogOpen = $state(false);
  let accessDialogAudience = $state<string | null>(null);
  let accessDialogState = $state<string>('unpublished');
  let accessDialogMethod = $state<string>('access-key');

  // --------------------------------------------------------------------------
  // Helpers
  // --------------------------------------------------------------------------

  function buildUrl(audience: string, token?: string): string {
    return namespaceService.buildAccessUrl(
      namespaceId,
      audience,
      token,
      subdomain ?? undefined,
      siteBaseUrl,
      siteDomain,
    );
  }

  function hasGate(decl: AudienceDecl, kind: Gate['kind']): boolean {
    return decl.gates.some((g) => g.kind === kind);
  }

  async function copyToClipboard(value: string, audienceKey?: string) {
    try {
      await navigator.clipboard.writeText(value);
      if (audienceKey) {
        copiedLinks[audienceKey] = true;
        setTimeout(() => {
          copiedLinks[audienceKey] = false;
        }, 1800);
      } else {
        showSuccess('Copied to clipboard');
      }
    } catch {
      showError(
        'Copy failed. Check browser clipboard permissions.',
        'Audiences',
      );
    }
  }

  // --------------------------------------------------------------------------
  // Magic-link generation
  // --------------------------------------------------------------------------

  async function generateLink(audience: string) {
    if (!namespaceId) {
      showInfo('Publish first to enable link generation.');
      return;
    }
    creatingLinkFor = audience;
    try {
      const result = await namespaceService.getAudienceToken(
        namespaceId,
        audience,
      );
      const url = buildUrl(audience, result.token);
      generatedLinks[audience] = url;
      await copyToClipboard(url, audience);
      showSuccess('Link copied — share it directly.');
    } catch (e) {
      showError(
        e instanceof Error ? e.message : 'Failed to generate link',
        'Audiences',
      );
    } finally {
      creatingLinkFor = null;
    }
  }

  // --------------------------------------------------------------------------
  // Password set/rotate
  // --------------------------------------------------------------------------

  function openPasswordDialog(audience: string) {
    passwordDialogAudience = audience;
    passwordDialogValue = '';
    passwordDialogConfirm = '';
    passwordDialogOpen = true;
  }

  async function submitPassword() {
    if (!passwordDialogAudience) return;
    if (passwordDialogValue.length < 4) {
      showError('Password must be at least 4 characters.', 'Audiences');
      return;
    }
    if (passwordDialogValue !== passwordDialogConfirm) {
      showError('Passwords do not match.', 'Audiences');
      return;
    }
    if (!namespaceId) {
      showError('Publish first to enable the password gate.', 'Audiences');
      return;
    }
    isRotatingPassword = true;
    try {
      const result = await namespaceService.rotateAudiencePassword(
        namespaceId,
        passwordDialogAudience,
        passwordDialogValue,
      );
      showSuccess(`Password set (version ${result.version}).`);
      passwordDialogOpen = false;
      passwordDialogValue = '';
      passwordDialogConfirm = '';
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to set password';
      // The server returns InvalidInput when the audience has no password
      // gate yet (e.g. declared in the file but not synced via publish).
      if (msg.toLowerCase().includes('password gate')) {
        showError(
          'Audience has no password gate on the server yet — publish once after declaring it, then try again.',
          'Audiences',
        );
      } else {
        showError(msg, 'Audiences');
      }
    } finally {
      isRotatingPassword = false;
    }
  }

  // --------------------------------------------------------------------------
  // Mailto: composition for `email` share-actions
  // --------------------------------------------------------------------------

  /** Conservative mailto cap. Most clients fall over above ~2000 chars. */
  const MAILTO_LIMIT = 1800;

  function fillTemplate(
    template: string | undefined,
    fallback: string,
    vars: Record<string, string>,
  ): string {
    let out = template ?? fallback;
    for (const [key, value] of Object.entries(vars)) {
      out = out.replaceAll(`{{${key}}}`, value);
    }
    return out;
  }

  function buildMailto(
    decl: AudienceDecl,
    action: Extract<ShareAction, { kind: 'email' }>,
  ): { url: string; truncated: boolean } {
    const url = decl.gates.length === 0 ? buildUrl(decl.name) : (generatedLinks[decl.name] ?? '');
    const subject = fillTemplate(action.subject_template, 'New from me', {
      title: decl.name,
      url,
    });
    const body = fillTemplate(
      action.body_template,
      url ? `${url}\n\n— Sent via Diaryx` : '— Sent via Diaryx',
      {
        title: decl.name,
        url,
      },
    );

    // Pack recipients into the BCC field, truncating until the URL fits the
    // conservative cap. Surface the truncation count so the writer can copy
    // remaining addresses from the share-actions UI later.
    const recipients = action.recipients.slice();
    let truncated = false;
    while (recipients.length > 0) {
      const bcc = recipients.join(',');
      const built = `mailto:?bcc=${encodeURIComponent(bcc)}&subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`;
      if (built.length <= MAILTO_LIMIT) {
        return { url: built, truncated };
      }
      recipients.pop();
      truncated = true;
    }
    return {
      url: `mailto:?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`,
      truncated: true,
    };
  }

  function composeEmail(
    decl: AudienceDecl,
    action: Extract<ShareAction, { kind: 'email' }>,
  ) {
    // For password / link audiences, require a generated link first so the
    // body actually contains something readers can click.
    if (decl.gates.length > 0 && !generatedLinks[decl.name]) {
      showInfo('Generate a link first so the email has something to share.');
      return;
    }
    const { url, truncated } = buildMailto(decl, action);
    if (truncated) {
      showInfo(
        `Recipient list trimmed to fit the email-link size limit. Copy the rest from the audience card.`,
      );
    }
    window.location.href = url;
  }

  // --------------------------------------------------------------------------
  // Migration: import legacy `audience_states` into `audiences:` block.
  // --------------------------------------------------------------------------

  function legacyToDecl(): AudienceDecl[] {
    const decls: AudienceDecl[] = [];
    for (const name of audiences) {
      const cfg = audienceStates[name];
      if (!cfg || cfg.state === 'unpublished') continue;
      const gates: Gate[] =
        cfg.state === 'public' ? [] : [{ kind: 'link' }];
      decls.push({ name, gates, share_actions: [] });
    }
    return decls;
  }

  async function migrate() {
    if (!configStore.rootIndexPath) {
      showError('No workspace open.', 'Audiences');
      return;
    }
    isMigrating = true;
    try {
      const decls = legacyToDecl();
      // The setField API expects a string value; for structured fields we
      // pass JSON which the Rust core re-parses into the expected shape.
      await configStore.setField('audiences', JSON.stringify(decls));
      await configStore.setField('audiences_migrated', 'true');
      showSuccess(
        `Imported ${decls.length} audience${decls.length === 1 ? '' : 's'} into the workspace file.`,
      );
    } catch (e) {
      showError(
        e instanceof Error ? e.message : 'Migration failed',
        'Audiences',
      );
    } finally {
      isMigrating = false;
    }
  }

  // --------------------------------------------------------------------------
  // Legacy access dialog handlers (used only when no file declaration exists)
  // --------------------------------------------------------------------------

  function openAccessDialog(audience: string) {
    const config = audienceStates[audience] ?? { state: 'unpublished' };
    accessDialogAudience = audience;
    accessDialogState = config.state;
    accessDialogMethod = config.access_method ?? 'access-key';
    accessDialogOpen = true;
  }

  async function saveLegacyDialog() {
    if (!accessDialogAudience) return;
    const config: AudienceConfig = {
      state: accessDialogState,
      access_method:
        accessDialogState === 'access-control'
          ? accessDialogMethod
          : undefined,
    };
    try {
      const access =
        accessDialogState === 'public'
          ? 'public'
          : accessDialogState === 'access-control'
            ? 'token'
            : 'private';
      if (namespaceId) {
        await namespaceService.setAudience(
          namespaceId,
          accessDialogAudience,
          access,
        );
      }
      onStateChange(accessDialogAudience, config);
    } catch (e) {
      showError(
        e instanceof Error ? e.message : 'Failed to save audience state',
        'Publishing',
      );
    }
    accessDialogOpen = false;
  }
</script>

<div class="space-y-3">
  {#if showMigrationBanner}
    <div
      class="rounded-md border border-amber-300/40 bg-amber-100/40 p-3 text-sm dark:border-amber-700/40 dark:bg-amber-950/30"
    >
      <div class="mb-1.5 flex items-center gap-2 font-medium">
        <AlertTriangle class="size-4 text-amber-700 dark:text-amber-400" />
        <span>Audiences moved to the workspace file</span>
      </div>
      <p class="mb-2 text-xs text-muted-foreground">
        Diaryx now stores audiences directly in your root index frontmatter.
        Importing brings your existing settings over so they sync on the next
        publish.
      </p>
      <div class="flex items-center gap-2">
        <Button
          size="sm"
          onclick={migrate}
          disabled={isMigrating}
        >
          {#if isMigrating}
            <Loader2 class="mr-1.5 size-3 animate-spin" />
          {/if}
          Import existing audiences
        </Button>
      </div>
    </div>
  {/if}

  {#if usingFile && declaredAudiences}
    <!-- File-as-truth path: render declared audiences with gate chips +
         share actions. -->
    {#if declaredAudiences.length === 0}
      <p class="text-xs text-muted-foreground">
        No audiences declared in the workspace file yet. Add an
        <code class="rounded bg-muted px-1 py-0.5">audiences:</code>
        block to your root index frontmatter.
      </p>
    {:else}
      <div class="space-y-2">
        {#each declaredAudiences as decl (decl.name)}
          {@const dot = getAudienceColor(decl.name, colorStore.audienceColors)}
          {@const isPublic = decl.gates.length === 0}
          {@const hasLink = hasGate(decl, 'link')}
          {@const hasPassword = hasGate(decl, 'password')}
          <div
            class="rounded-md border border-border bg-card p-3 text-sm shadow-sm"
          >
            <div class="mb-2 flex items-center gap-2">
              <span
                class="inline-block size-2 rounded-full"
                style="background-color: {dot};"
                aria-hidden="true"
              ></span>
              <span class="font-medium">{decl.name}</span>
              {#if decl.name === defaultAudience}
                <span class="text-xs text-muted-foreground">(default)</span>
              {/if}
              <div class="ml-auto flex items-center gap-1">
                {#if isPublic}
                  <span
                    class="inline-flex items-center gap-1 rounded-full border border-border bg-muted px-1.5 py-0.5 text-[10px] font-medium"
                  >
                    <Globe class="size-3" />
                    Public
                  </span>
                {/if}
                {#if hasLink}
                  <span
                    class="inline-flex items-center gap-1 rounded-full border border-border bg-muted px-1.5 py-0.5 text-[10px] font-medium"
                  >
                    <LinkIcon class="size-3" />
                    Link
                  </span>
                {/if}
                {#if hasPassword}
                  <span
                    class="inline-flex items-center gap-1 rounded-full border border-border bg-muted px-1.5 py-0.5 text-[10px] font-medium"
                  >
                    <Lock class="size-3" />
                    Password
                  </span>
                {/if}
              </div>
            </div>

            <!-- Share-action row -->
            <div class="flex flex-wrap items-center gap-1.5">
              {#if isPublic}
                <Button
                  size="sm"
                  variant="outline"
                  onclick={() =>
                    copyToClipboard(buildUrl(decl.name), decl.name)}
                >
                  {#if copiedLinks[decl.name]}
                    <Check class="mr-1 size-3" /> Copied
                  {:else}
                    <Copy class="mr-1 size-3" /> Copy public URL
                  {/if}
                </Button>
              {/if}

              {#if hasLink}
                <Button
                  size="sm"
                  variant="outline"
                  onclick={() => generateLink(decl.name)}
                  disabled={creatingLinkFor === decl.name || !namespaceId}
                >
                  {#if creatingLinkFor === decl.name}
                    <Loader2 class="mr-1 size-3 animate-spin" />
                  {:else if generatedLinks[decl.name]}
                    <Check class="mr-1 size-3" />
                  {:else}
                    <LinkIcon class="mr-1 size-3" />
                  {/if}
                  {generatedLinks[decl.name]
                    ? 'Re-generate link'
                    : 'Generate link'}
                </Button>
              {/if}

              {#if hasPassword}
                <Button
                  size="sm"
                  variant="outline"
                  onclick={() => openPasswordDialog(decl.name)}
                  disabled={!namespaceId}
                >
                  <KeyRound class="mr-1 size-3" /> Set / rotate password
                </Button>
              {/if}

              {#each decl.share_actions as action}
                {#if action.kind === 'email'}
                  <Button
                    size="sm"
                    variant="ghost"
                    onclick={() => composeEmail(decl, action)}
                    disabled={action.recipients.length === 0}
                  >
                    <Mail class="mr-1 size-3" />
                    Compose email
                    {#if action.recipients.length > 0}
                      <span class="ml-1 text-[10px] text-muted-foreground">
                        ({action.recipients.length})
                      </span>
                    {/if}
                  </Button>
                {:else if action.kind === 'copy_link'}
                  <Button
                    size="sm"
                    variant="ghost"
                    onclick={() => {
                      const url = isPublic
                        ? buildUrl(decl.name)
                        : (generatedLinks[decl.name] ?? '');
                      if (!url) {
                        showInfo(
                          'Generate a link first to copy a shareable URL.',
                        );
                        return;
                      }
                      copyToClipboard(url, decl.name);
                    }}
                  >
                    <Copy class="mr-1 size-3" />
                    {action.label ?? 'Copy link'}
                  </Button>
                {/if}
              {/each}
            </div>

            {#if generatedLinks[decl.name]}
              <p
                class="mt-2 break-all rounded bg-muted px-2 py-1 text-[11px] text-muted-foreground"
              >
                {generatedLinks[decl.name]}
              </p>
            {/if}

            {#if hasPassword && !namespaceId}
              <p class="mt-2 text-[11px] text-muted-foreground">
                Publish at least once to enable the password gate.
              </p>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <!-- Legacy fallback path -->
    {#if audiences.length === 0}
      <p class="text-xs text-muted-foreground">No audiences yet.</p>
    {:else}
      <div class="space-y-1">
        {#each audiences as audience (audience)}
          {@const config = audienceStates[audience] ?? { state: 'unpublished' }}
          {@const dot = getAudienceColor(audience, colorStore.audienceColors)}
          <button
            class="flex w-full items-center gap-2 rounded-md border border-border bg-card px-3 py-1.5 text-left text-sm hover:bg-accent"
            onclick={() => openAccessDialog(audience)}
            type="button"
          >
            <span
              class="inline-block size-2 rounded-full"
              style="background-color: {dot};"
              aria-hidden="true"
            ></span>
            <span class="flex-1 truncate">
              {audience}
              {#if audience === defaultAudience}
                <span class="text-xs text-muted-foreground">(default)</span>
              {/if}
            </span>
            <span
              class="inline-flex items-center gap-1 text-xs text-muted-foreground"
            >
              {#if config.state === 'public'}
                <Globe class="size-3" /> Public
              {:else if config.state === 'access-control'}
                <Lock class="size-3" /> Access key
              {:else}
                Unpublished
              {/if}
              <Settings2 class="size-3" />
            </span>
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<!-- =========================================================================
     Password set/rotate dialog
     ========================================================================= -->
<Dialog.Root bind:open={passwordDialogOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <Shield class="size-4" />
        Set password for "{passwordDialogAudience}"
      </Dialog.Title>
      <Dialog.Description>
        Setting a new password invalidates any unlock cookies readers
        already have. Magic links you've shared keep working.
      </Dialog.Description>
    </Dialog.Header>
    <div class="space-y-3 py-2">
      <div>
        <Label for="audience-password">Password</Label>
        <Input
          id="audience-password"
          type="password"
          bind:value={passwordDialogValue}
          autocomplete="new-password"
          autofocus
        />
      </div>
      <div>
        <Label for="audience-password-confirm">Confirm</Label>
        <Input
          id="audience-password-confirm"
          type="password"
          bind:value={passwordDialogConfirm}
          autocomplete="new-password"
        />
      </div>
    </div>
    <Dialog.Footer>
      <Button
        variant="outline"
        onclick={() => (passwordDialogOpen = false)}
      >
        Cancel
      </Button>
      <Button onclick={submitPassword} disabled={isRotatingPassword}>
        {#if isRotatingPassword}
          <Loader2 class="mr-1.5 size-3 animate-spin" />
        {/if}
        Save
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- =========================================================================
     Legacy access dialog (only shown when audiences are not declared in
     the workspace file).
     ========================================================================= -->
<Dialog.Root bind:open={accessDialogOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Access for "{accessDialogAudience}"</Dialog.Title>
    </Dialog.Header>
    <div class="space-y-2 py-2">
      <Button
        variant={accessDialogState === 'unpublished' ? 'default' : 'outline'}
        class="w-full justify-start"
        onclick={() => (accessDialogState = 'unpublished')}
      >
        Unpublished
      </Button>
      <Button
        variant={accessDialogState === 'public' ? 'default' : 'outline'}
        class="w-full justify-start"
        onclick={() => (accessDialogState = 'public')}
      >
        <Globe class="mr-2 size-4" /> Public
      </Button>
      <Button
        variant={accessDialogState === 'access-control' ? 'default' : 'outline'}
        class="w-full justify-start"
        onclick={() => (accessDialogState = 'access-control')}
      >
        <Lock class="mr-2 size-4" /> Access key
      </Button>
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (accessDialogOpen = false)}>
        Cancel
      </Button>
      <Button onclick={saveLegacyDialog}>Save</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
