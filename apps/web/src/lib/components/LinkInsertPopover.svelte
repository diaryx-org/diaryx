<script lang="ts">
  import * as Tabs from "$lib/components/ui/tabs";
  import * as Command from "$lib/components/ui/command";
  import * as Drawer from "$lib/components/ui/drawer";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
  import { getLinkFormatStore } from "$lib/stores/linkFormatStore.svelte";
  import type { TreeNode } from "$lib/backend";
  import type { Api } from "$lib/backend/api";
  import { FileText, Globe, ArrowRight } from "@lucide/svelte";

  interface Props {
    open: boolean;
    onSelect: (href: string) => void;
    onClose: () => void;
    currentEntryPath?: string;
    api?: Api | null;
  }

  let {
    open = $bindable(),
    onSelect,
    onClose,
    currentEntryPath = "",
    api = null,
  }: Props = $props();

  let activeTab = $state("remote");
  let urlInput = $state("");
  let searchValue = $state("");
  let urlInputElement: HTMLInputElement | null = $state(null);
  let wrapperElement: HTMLDivElement | null = $state(null);
  let showBelow = $state(false);

  const mobileState = getMobileState();
  const linkFormatStore = getLinkFormatStore();

  function getAllEntries(node: TreeNode | null): { path: string; name: string }[] {
    if (!node) return [];
    const entries: { path: string; name: string }[] = [];
    function traverse(n: TreeNode) {
      entries.push({ path: n.path, name: n.name });
      for (const child of n.children) {
        traverse(child);
      }
    }
    traverse(node);
    return entries;
  }

  const allEntries = $derived(getAllEntries(workspaceStore.tree));
  const filteredEntries = $derived(
    allEntries
      .filter((e) => e.path !== currentEntryPath)
      .filter(
        (e) =>
          !searchValue.trim() ||
          e.name.toLowerCase().includes(searchValue.toLowerCase()) ||
          e.path.toLowerCase().includes(searchValue.toLowerCase()),
      ),
  );

  function handleRemoteSubmit() {
    const url = urlInput.trim();
    if (!url) return;
    // Auto-add https:// if no protocol specified
    const href = url.match(/^https?:\/\//) ? url : `https://${url}`;
    onSelect(href);
    reset();
  }

  async function handleLocalSelect(file: { path: string; name: string }) {
    if (api && currentEntryPath) {
      try {
        const format = linkFormatStore.format;
        // formatLink expects canonical paths — the file.path from the tree is already canonical
        const formatted = await api.formatLink(
          file.path,
          file.name,
          format,
          currentEntryPath,
        );
        // For markdown formats, extract just the href part
        // markdown_root: [Title](/path.md) → /path.md
        // markdown_relative: [Title](../path.md) → ../path.md
        const mdMatch = formatted.match(/\[.*?\]\((.*?)\)/);
        let href = mdMatch ? mdMatch[1] : formatted;
        // Strip angle brackets used for paths with spaces: <path with spaces.md> → path with spaces.md
        if (href.startsWith("<") && href.endsWith(">")) {
          href = href.slice(1, -1);
        }
        onSelect(href);
      } catch {
        // Fallback: use the raw path
        onSelect(file.path);
      }
    } else {
      onSelect(file.path);
    }
    reset();
  }

  function reset() {
    urlInput = "";
    searchValue = "";
    activeTab = "remote";
    open = false;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      reset();
    }
  }

  function handleClickOutside(e: MouseEvent) {
    if (wrapperElement && !wrapperElement.contains(e.target as Node)) {
      onClose();
      reset();
    }
  }

  // Check if there's enough room above, flip below if not
  $effect(() => {
    if (open && wrapperElement && !mobileState.isMobile) {
      const rect = wrapperElement.parentElement?.getBoundingClientRect();
      if (rect) {
        // Estimate popover height (~300px for local tab with results)
        showBelow = rect.top < 320;
      }
    }
  });

  // Focus URL input when opening on remote tab
  $effect(() => {
    if (open && activeTab === "remote" && urlInputElement) {
      // Small delay to let the DOM settle
      setTimeout(() => urlInputElement?.focus(), 50);
    }
  });

  // Set up click outside listener when open
  $effect(() => {
    if (open && !mobileState.isMobile) {
      const timeoutId = setTimeout(() => {
        document.addEventListener("mousedown", handleClickOutside);
        document.addEventListener("keydown", handleKeydown);
      }, 0);

      return () => {
        clearTimeout(timeoutId);
        document.removeEventListener("mousedown", handleClickOutside);
        document.removeEventListener("keydown", handleKeydown);
      };
    }
  });
</script>

{#snippet linkContent()}
  <Tabs.Root bind:value={activeTab} class="w-full">
    <Tabs.List class="w-full grid grid-cols-2">
      <Tabs.Trigger value="remote">
        <Globe class="size-3.5 mr-1.5" />
        Remote
      </Tabs.Trigger>
      <Tabs.Trigger value="local">
        <FileText class="size-3.5 mr-1.5" />
        Local
      </Tabs.Trigger>
    </Tabs.List>

    <Tabs.Content value="remote" class="mt-2">
      <form
        onsubmit={(e) => {
          e.preventDefault();
          handleRemoteSubmit();
        }}
        class="flex gap-1.5"
      >
        <input
          bind:this={urlInputElement}
          bind:value={urlInput}
          type="text"
          placeholder="https://example.com"
          class="flex-1 min-w-0 rounded-md border border-input bg-background px-2.5 py-1.5 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
          onmousedown={(e) => e.stopPropagation()}
        />
        <button
          type="submit"
          class="inline-flex items-center justify-center rounded-md bg-primary px-2.5 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          disabled={!urlInput.trim()}
        >
          <ArrowRight class="size-4" />
        </button>
      </form>
    </Tabs.Content>

    <Tabs.Content value="local" class="mt-2">
      <Command.Root class="rounded-lg border-none shadow-none" shouldFilter={false}>
        <Command.Input
          placeholder="Search files..."
          bind:value={searchValue}
          onmousedown={(e) => e.stopPropagation()}
        />
        <Command.List class="max-h-48">
          <Command.Empty>No files found.</Command.Empty>
          {#each filteredEntries.slice(0, 20) as file (file.path)}
            <Command.Item value={file.path} onSelect={() => handleLocalSelect(file)}>
              <FileText class="mr-2 size-4 shrink-0" />
              <div class="flex flex-col min-w-0">
                <span class="truncate text-sm">{file.name}</span>
                <span class="truncate text-xs text-muted-foreground">{file.path}</span>
              </div>
            </Command.Item>
          {/each}
        </Command.List>
      </Command.Root>
    </Tabs.Content>
  </Tabs.Root>
{/snippet}

{#if mobileState.isMobile}
  <Drawer.Root bind:open direction="bottom" onClose={() => { onClose(); reset(); }}>
    <Drawer.Content>
      <div class="mx-auto w-full max-w-md px-4 pb-4 pt-2">
        {@render linkContent()}
      </div>
    </Drawer.Content>
  </Drawer.Root>
{:else if open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={wrapperElement}
    class="link-insert-dropdown"
    class:show-below={showBelow}
    role="dialog"
    tabindex="-1"
    onmousedown={(e) => e.preventDefault()}
  >
    {@render linkContent()}
  </div>
{/if}

<style>
  .link-insert-dropdown {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    width: 320px;
    padding: 12px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 100;
    animation: fadeInAbove 0.15s ease;
  }

  .link-insert-dropdown.show-below {
    bottom: auto;
    top: calc(100% + 8px);
    animation: fadeInBelow 0.15s ease;
  }

  @keyframes fadeInAbove {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  @keyframes fadeInBelow {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }
</style>
