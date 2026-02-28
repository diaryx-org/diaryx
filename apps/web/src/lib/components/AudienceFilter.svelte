<script lang="ts">
  /**
   * AudienceFilter — Sidebar dropdown for previewing content as a specific audience.
   *
   * When an audience is selected, the editor hides inactive conditional branches
   * and markers, showing only what that audience would see.
   */
  import * as Select from "$lib/components/ui/select";
  import { Eye } from "@lucide/svelte";
  import { getTemplateContextStore } from "../stores/templateContextStore.svelte";
  import type { Api } from "../backend";

  interface Props {
    api: Api | null;
    rootPath: string | null;
  }

  let { api, rootPath }: Props = $props();

  const templateContextStore = getTemplateContextStore();

  let audiences = $state<string[]>([]);

  async function loadAudiences() {
    if (!api || !rootPath) {
      audiences = [];
      return;
    }
    try {
      audiences = await api.getAvailableAudiences(rootPath);
    } catch (e) {
      console.warn("[AudienceFilter] Failed to load audiences:", e);
      audiences = [];
    }
  }

  // Load audiences when rootPath changes or a new audience tag is created anywhere
  $effect(() => {
    // Reading audiencesVersion here makes this effect re-run when it is bumped
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    templateContextStore.audiencesVersion;
    if (rootPath) {
      loadAudiences();
    }
  });

  function handleChange(value: string | undefined) {
    if (value === "__all__" || !value) {
      templateContextStore.setPreviewAudience(null);
    } else {
      templateContextStore.setPreviewAudience(value);
    }
  }

  let currentValue = $derived(templateContextStore.previewAudience ?? "__all__");
  let isFiltering = $derived(templateContextStore.previewAudience !== null);
</script>

{#if audiences.length > 0}
  <div class="audience-filter" class:filtering={isFiltering}>
    <Select.Root
      type="single"
      value={currentValue}
      onValueChange={handleChange}
    >
      <Select.Trigger class="audience-filter-trigger">
        <span class="audience-filter-label">
          <Eye class="size-3.5" />
          {#if isFiltering}
            {templateContextStore.previewAudience}
          {:else}
            All audiences
          {/if}
        </span>
      </Select.Trigger>
      <Select.Content>
        <Select.Item value="__all__">All audiences</Select.Item>
        <Select.Separator />
        {#each audiences as audience}
          <Select.Item value={audience}>{audience}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </div>
{/if}

<style>
  .audience-filter {
    display: flex;
    align-items: center;
  }

  .audience-filter :global(.audience-filter-trigger) {
    width: 100%;
    height: 28px;
    padding: 0 8px;
    font-size: 12px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--sidebar-foreground);
    opacity: 0.7;
    transition: all 0.15s ease;
  }

  .audience-filter :global(.audience-filter-trigger:hover) {
    opacity: 1;
    border-color: var(--sidebar-border);
    background: color-mix(in oklch, var(--sidebar-accent) 50%, transparent);
  }

  .audience-filter.filtering :global(.audience-filter-trigger) {
    opacity: 1;
    border-color: color-mix(in oklch, var(--primary) 30%, transparent);
    background: color-mix(in oklch, var(--primary) 8%, transparent);
    color: var(--primary);
  }

  .audience-filter-label {
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
