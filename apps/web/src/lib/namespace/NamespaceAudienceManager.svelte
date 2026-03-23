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
    Mail,
    Send,
    Users,
    Plus,
    Trash2,
    Link,
  } from '@lucide/svelte';
  import { Input } from '$lib/components/ui/input';
  import { Switch } from '$lib/components/ui/switch';
  import { Label } from '$lib/components/ui/label';
  import { showError, showSuccess, showInfo } from '@/models/services/toastService';
  import * as namespaceService from './namespaceService';
  import type { AudienceConfig } from './namespaceContext.svelte';

  interface Props {
    namespaceId: string;
    audiences: string[];
    audienceStates: Record<string, AudienceConfig>;
    defaultAudience: string | null;
    onStateChange: (audience: string, config: AudienceConfig) => void;
    onSendEmail?: (audience: string) => void;
  }

  let { namespaceId, audiences, audienceStates, defaultAudience, onStateChange, onSendEmail }: Props = $props();

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

  // Email settings dialog state
  let emailOnPublish = $state(false);
  let emailSubject = $state('');
  let emailCover = $state('');
  let isSendingEmail = $state(false);

  // Subscriber management state
  let subscribers = $state<namespaceService.SubscriberInfo[]>([]);
  let subscribersLoading = $state(false);
  let subscribersExpanded = $state(false);
  let newSubscriberEmail = $state('');
  let isAddingSubscriber = $state(false);
  let copiedSignupUrl = $state(false);
  let subscriberError = $state<string | null>(null);

  function getAudienceState(audience: string): AudienceConfig {
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
    emailOnPublish = config.email_on_publish ?? false;
    emailSubject = config.email_subject ?? '';
    emailCover = config.email_cover ?? '';
    accessDialogOpen = true;
    lastCreatedAccessUrl = null;
    // Reset subscriber state
    subscribers = [];
    subscribersExpanded = false;
    newSubscriberEmail = '';
    copiedSignupUrl = false;
    subscriberError = null;
    // Load subscribers if email is enabled
    if (emailOnPublish && namespaceId) {
      loadSubscribers(audience);
    }
  }

  async function loadSubscribers(audience: string) {
    if (!namespaceId) return;
    subscribersLoading = true;
    subscriberError = null;
    try {
      subscribers = await namespaceService.listSubscribers(namespaceId, audience);
    } catch (e) {
      subscribers = [];
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes('not configured') || msg.includes('unavailable') || msg.includes('503')) {
        subscriberError = 'Email service not configured. Set RESEND_API_KEY on the server to manage subscribers.';
      }
    } finally {
      subscribersLoading = false;
    }
  }

  async function handleAddSubscriber() {
    if (!accessDialogAudience || !namespaceId || !newSubscriberEmail.trim()) return;
    isAddingSubscriber = true;
    subscriberError = null;
    try {
      await namespaceService.addSubscriber(namespaceId, accessDialogAudience, newSubscriberEmail.trim());
      newSubscriberEmail = '';
      showSuccess('Subscriber added');
      await loadSubscribers(accessDialogAudience);
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to add subscriber';
      if (msg.includes('not configured') || msg.includes('unavailable') || msg.includes('503')) {
        subscriberError = 'Email service not configured. Set RESEND_API_KEY on the server.';
      } else {
        showError(msg, 'Subscribers');
      }
    } finally {
      isAddingSubscriber = false;
    }
  }

  async function handleRemoveSubscriber(contactId: string) {
    if (!accessDialogAudience || !namespaceId) return;
    try {
      await namespaceService.removeSubscriber(namespaceId, accessDialogAudience, contactId);
      subscribers = subscribers.filter(s => s.id !== contactId);
      showSuccess('Subscriber removed');
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to remove subscriber', 'Subscribers');
    }
  }

  function getSignupUrl(): string {
    if (!namespaceId || !accessDialogAudience) return '';
    return namespaceService.buildSubscribeUrl(namespaceId, accessDialogAudience);
  }

  async function copySignupUrl() {
    try {
      await navigator.clipboard.writeText(getSignupUrl());
      copiedSignupUrl = true;
      setTimeout(() => { copiedSignupUrl = false; }, 1800);
    } catch {
      showError('Copy failed. Check browser clipboard permissions.', 'Subscribers');
    }
  }

  async function handleSendEmail() {
    if (!accessDialogAudience || !onSendEmail) return;
    isSendingEmail = true;
    try {
      onSendEmail(accessDialogAudience);
      showSuccess(`Email send triggered for "${accessDialogAudience}"`);
    } catch (e) {
      showError(e instanceof Error ? e.message : 'Failed to send email', 'Email');
    } finally {
      isSendingEmail = false;
    }
  }

  async function handleSaveAccessDialog() {
    if (!accessDialogAudience) return;
    const config: AudienceConfig = {
      state: accessDialogState,
      access_method: accessDialogState === 'access-control' ? accessDialogMethod : undefined,
      email_on_publish: emailOnPublish,
      email_subject: emailSubject || undefined,
      email_cover: emailCover || undefined,
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
          {#if config.email_on_publish}
            <Mail class="size-3 text-muted-foreground" />
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

      <!-- Email settings -->
      {#if accessDialogState !== 'unpublished'}
        <div class="space-y-3 p-3 rounded-md bg-secondary border border-border">
          <div class="flex items-center justify-between">
            <Label class="text-xs font-medium text-muted-foreground flex items-center gap-1.5">
              <Mail class="size-3" />
              Email on publish
            </Label>
            <Switch
              checked={emailOnPublish}
              onCheckedChange={(checked) => {
                emailOnPublish = checked;
                if (checked && accessDialogAudience && namespaceId) {
                  loadSubscribers(accessDialogAudience);
                }
              }}
            />
          </div>

          {#if emailOnPublish}
            <div class="space-y-2">
              <div class="space-y-1">
                <label for="email-subject" class="text-xs font-medium text-muted-foreground">Subject template</label>
                <Input
                  id="email-subject"
                  type="text"
                  placeholder="{'{'}title{'}'} — New posts"
                  bind:value={emailSubject}
                  class="h-8 text-xs"
                />
                <p class="text-[10px] text-muted-foreground">Use {'{'}title{'}'} for the site title.</p>
              </div>

              <div class="space-y-1">
                <label for="email-cover" class="text-xs font-medium text-muted-foreground">Cover file (optional)</label>
                <Input
                  id="email-cover"
                  type="text"
                  placeholder="newsletters/intro.md"
                  bind:value={emailCover}
                  class="h-8 text-xs"
                />
                <p class="text-[10px] text-muted-foreground">Markdown file shown as intro above the entry digest.</p>
              </div>

              {#if namespaceId && onSendEmail}
                <Button
                  variant="secondary"
                  size="sm"
                  class="w-full h-8 text-xs"
                  onclick={handleSendEmail}
                  disabled={isSendingEmail || subscribers.length === 0}
                >
                  {#if isSendingEmail}
                    <Loader2 class="size-3.5 mr-1 animate-spin" />
                    Sending...
                  {:else}
                    <Send class="size-3.5 mr-1" />
                    {subscribers.length > 0 ? `Send to ${subscribers.length} subscriber${subscribers.length === 1 ? '' : 's'}` : 'Add subscribers first'}
                  {/if}
                </Button>
              {/if}

              <!-- Subscriber management -->
              {#if namespaceId}
                <div class="border-t border-border pt-2 mt-1">
                  <button
                    class="w-full flex items-center justify-between text-xs text-muted-foreground hover:text-foreground transition-colors py-1"
                    onclick={() => {
                      subscribersExpanded = !subscribersExpanded;
                      if (subscribersExpanded && accessDialogAudience) loadSubscribers(accessDialogAudience);
                    }}
                  >
                    <span class="flex items-center gap-1.5 font-medium">
                      <Users class="size-3" />
                      Subscribers
                      {#if subscribers.length > 0}
                        <span class="text-[10px] bg-muted px-1.5 py-0.5 rounded-full">{subscribers.length}</span>
                      {/if}
                    </span>
                    <span class="text-[10px]">{subscribersExpanded ? 'Hide' : 'Show'}</span>
                  </button>

                  {#if subscribersExpanded}
                    <div class="space-y-2 pt-1.5">
                      {#if subscriberError}
                        <p class="text-[11px] text-destructive bg-destructive/10 rounded px-2 py-1.5">{subscriberError}</p>
                      {/if}
                      <!-- Add subscriber -->
                      <div class="flex gap-1.5">
                        <Input
                          type="email"
                          placeholder="email@example.com"
                          bind:value={newSubscriberEmail}
                          class="h-7 text-xs flex-1"
                          onkeydown={(e) => { if (e.key === 'Enter') handleAddSubscriber(); }}
                        />
                        <Button
                          variant="secondary"
                          size="sm"
                          class="h-7 text-xs px-2 shrink-0"
                          onclick={handleAddSubscriber}
                          disabled={isAddingSubscriber || !newSubscriberEmail.trim()}
                        >
                          {#if isAddingSubscriber}
                            <Loader2 class="size-3 animate-spin" />
                          {:else}
                            <Plus class="size-3" />
                          {/if}
                        </Button>
                      </div>

                      <!-- Signup link -->
                      <Button
                        variant="ghost"
                        size="sm"
                        class="w-full h-7 text-[11px] text-muted-foreground justify-start"
                        onclick={copySignupUrl}
                      >
                        {#if copiedSignupUrl}
                          <Check class="size-3 mr-1" /> Copied signup URL
                        {:else}
                          <Link class="size-3 mr-1" /> Copy public signup URL
                        {/if}
                      </Button>

                      <!-- Subscriber list -->
                      {#if subscribersLoading}
                        <div class="flex items-center justify-center py-2">
                          <Loader2 class="size-3.5 animate-spin text-muted-foreground" />
                        </div>
                      {:else if subscribers.length === 0}
                        <p class="text-[11px] text-muted-foreground text-center py-1">No subscribers yet.</p>
                      {:else}
                        <div class="max-h-32 overflow-y-auto space-y-0.5">
                          {#each subscribers as sub (sub.id)}
                            <div class="flex items-center justify-between px-1.5 py-1 rounded text-xs hover:bg-muted/50 group">
                              <span class="truncate flex-1 text-[11px]">{sub.email}</span>
                              <button
                                class="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive transition-opacity p-0.5"
                                onclick={() => handleRemoveSubscriber(sub.id)}
                                aria-label="Remove subscriber"
                              >
                                <Trash2 class="size-3" />
                              </button>
                            </div>
                          {/each}
                        </div>
                      {/if}
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}

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
