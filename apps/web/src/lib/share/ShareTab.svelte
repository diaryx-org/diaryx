<script lang="ts">
  import LiveCollaborationPanel from './LiveCollaborationPanel.svelte';
  import PublishingPanel from './PublishingPanel.svelte';
  import type { Api } from '$lib/backend/api';

  interface Props {
    onSessionStart?: () => void;
    onSessionEnd?: () => void;
    onBeforeHost?: (audience: string | null) => Promise<void>;
    onOpenEntry?: (path: string) => Promise<void>;
    api: Api | null;
    triggerStart?: boolean;
    onTriggerStartConsumed?: () => void;
  }

  let {
    onSessionStart,
    onSessionEnd,
    onBeforeHost,
    onOpenEntry,
    api,
    triggerStart = false,
    onTriggerStartConsumed,
  }: Props = $props();

  type ShareSubTab = 'live-collaboration' | 'publishing';
  let shareSubTab: ShareSubTab = $state('live-collaboration');
</script>

<div class="px-3 pt-3 pb-1">
  <div class="flex items-center gap-1 bg-muted rounded-md p-0.5">
    <button
      type="button"
      class="flex-1 px-2 py-1 text-[11px] font-medium rounded transition-colors {shareSubTab === 'live-collaboration' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
      onclick={() => (shareSubTab = 'live-collaboration')}
    >
      Live Collaboration <span class="text-[8px] font-semibold uppercase px-1 rounded-full bg-amber-500/15 text-amber-600 dark:text-amber-400">alpha</span>
    </button>
    <button
      type="button"
      class="flex-1 px-2 py-1 text-[11px] font-medium rounded transition-colors {shareSubTab === 'publishing' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
      onclick={() => (shareSubTab = 'publishing')}
    >
      Publishing
    </button>
  </div>
</div>

{#if shareSubTab === 'live-collaboration'}
  <LiveCollaborationPanel
    {onSessionStart}
    {onSessionEnd}
    {onBeforeHost}
    {onOpenEntry}
    {api}
    {triggerStart}
    {onTriggerStartConsumed}
  />
{:else}
  <PublishingPanel />
{/if}
