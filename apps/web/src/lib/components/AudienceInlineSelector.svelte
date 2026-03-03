<script lang="ts">
  import * as Command from "$lib/components/ui/command";
  import { Plus } from "@lucide/svelte";

  interface Props {
    onSelect: (audience: string) => void;
    onCancel: () => void;
    getAvailableAudiences?: () => Promise<string[]>;
  }

  let { onSelect, onCancel, getAvailableAudiences }: Props = $props();

  let searchValue = $state("");
  let availableAudiences = $state<string[]>([]);
  let selectorEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    if (getAvailableAudiences) {
      getAvailableAudiences()
        .then((a) => { availableAudiences = a; })
        .catch(() => {});
    }
  });

  // Auto-focus the combobox input on mount
  $effect(() => {
    if (selectorEl) {
      const input = selectorEl.querySelector("input");
      setTimeout(() => input?.focus(), 0);
    }
  });

  const filteredSuggestions = $derived(
    availableAudiences.filter(
      (a) =>
        !searchValue.trim() ||
        a.toLowerCase().includes(searchValue.trim().toLowerCase()),
    ),
  );

  const showCreateOption = $derived(
    searchValue.trim().length > 0 &&
      !availableAudiences.some(
        (a) => a.toLowerCase() === searchValue.trim().toLowerCase(),
      ),
  );

  function handleSelect(value: string) {
    onSelect(value.trim());
  }

  function handleKeydown(e: KeyboardEvent) {
    // Stop ALL key events from bubbling to TipTap/ProseMirror.
    // Without this, printable keys reach the editor while the block picker
    // node is still ProseMirror-selected, causing it to delete the node and
    // insert the character directly into the document.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  }

  function handleMousedown(e: MouseEvent) {
    // Prevent ProseMirror from receiving mousedown and repositioning its
    // cursor/selection, which can deselect the block picker node.
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={selectorEl}
  class="audience-inline-selector"
  onkeydown={handleKeydown}
  onmousedown={handleMousedown}
>
  <Command.Root
    class="rounded-md border-none shadow-none"
    shouldFilter={false}
    aria-label="Select audience"
  >
    <Command.Input
      placeholder="Search audiences or create new..."
      bind:value={searchValue}
    />
    <Command.List role="listbox" aria-label="Audience suggestions">
      {#if showCreateOption}
        <Command.Item
          value={"create:" + searchValue.trim()}
          onSelect={() => handleSelect(searchValue.trim())}
        >
          <Plus class="mr-2 size-4 shrink-0" />
          Create "{searchValue.trim()}"
        </Command.Item>
      {/if}
      {#if filteredSuggestions.length === 0 && !showCreateOption}
        <Command.Empty>Type to create an audience.</Command.Empty>
      {/if}
      {#each filteredSuggestions as tag (tag)}
        <Command.Item value={tag} onSelect={() => handleSelect(tag)}>
          {tag}
        </Command.Item>
      {/each}
    </Command.List>
  </Command.Root>
</div>

<style>
  .audience-inline-selector {
    width: 220px;
    animation: audience-scale-in 0.15s ease;
    transform-origin: top center;
  }

  @keyframes audience-scale-in {
    from {
      opacity: 0;
      transform: scaleY(0.95);
    }
    to {
      opacity: 1;
      transform: scaleY(1);
    }
  }
</style>
