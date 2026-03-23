<script lang="ts">
  /**
   * Host widget wrapping DocumentAudiencePill for use by plugins.
   *
   * Widget ID: "audience.picker"
   *
   * Shows the audience tag picker for the currently open entry. If no entry is
   * open, shows a placeholder message.
   */
  import type { Api } from '$lib/backend/api';
  import DocumentAudiencePill from '$lib/components/DocumentAudiencePill.svelte';
  import { getEntryStore } from '@/models/stores/entryStore.svelte';
  import { workspaceStore } from '@/models/stores/workspaceStore.svelte';
  import { getNamespaceContext } from './namespaceContext.svelte';

  interface Props {
    api?: Api | null;
  }

  let { api = null }: Props = $props();

  const ctx = getNamespaceContext();
  const entryStore = getEntryStore();

  let entryPath = $derived(entryStore.currentEntry?.path ?? '');
  let rootPath = $derived(workspaceStore.tree?.path ?? '');

  // Read explicit audience from current entry frontmatter (null = inheriting)
  let audience = $derived.by(() => {
    const fm = entryStore.currentEntry?.frontmatter;
    if (!fm || !('audience' in fm)) return null;
    const val = fm.audience;
    if (Array.isArray(val)) return val as string[];
    if (val === null || val === undefined) return null;
    return null;
  });

  async function handleChange(value: string[] | null) {
    if (!api || !entryPath) return;
    await api.setFrontmatterProperty(entryPath, 'audience', value);
    ctx.loadAudiences();
  }

  function openManager() {
    ctx.hostAction?.({ type: 'open-audience-manager' });
  }
</script>

{#if entryPath && rootPath}
  <DocumentAudiencePill
    {audience}
    {entryPath}
    {rootPath}
    {api}
    onChange={handleChange}
    onOpenManager={openManager}
  />
{:else}
  <p class="text-xs text-muted-foreground">Open an entry to edit its audience tags.</p>
{/if}
