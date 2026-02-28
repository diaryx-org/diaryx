<script lang="ts">
  import LiveCollaborationPanel from './LiveCollaborationPanel.svelte';
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
    api: _api,
    triggerStart = false,
    onTriggerStartConsumed,
  }: Props = $props();
</script>

<LiveCollaborationPanel
  {onSessionStart}
  {onSessionEnd}
  {onBeforeHost}
  {onOpenEntry}
  {triggerStart}
  {onTriggerStartConsumed}
/>
