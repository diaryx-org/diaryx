<script lang="ts">
  import type { EntryData } from "./backend";
  import { Button } from "$lib/components/ui/button";
  import {
    Calendar,
    Clock,
    Tag,
    FileText,
    Link,
    Hash,
    List,
    ToggleLeft,
    Type,
    PanelRightClose,
  } from "@lucide/svelte";

  interface Props {
    entry: EntryData | null;
    collapsed: boolean;
    onToggleCollapse: () => void;
  }

  let { entry, collapsed, onToggleCollapse }: Props = $props();

  // Get an icon for a frontmatter key
  function getIcon(key: string) {
    const lowerKey = key.toLowerCase();
    if (lowerKey === "title") return Type;
    if (lowerKey === "created" || lowerKey === "date") return Calendar;
    if (lowerKey === "updated" || lowerKey === "modified") return Clock;
    if (lowerKey === "tags" || lowerKey === "categories") return Tag;
    if (lowerKey === "part_of" || lowerKey === "parent") return Link;
    if (lowerKey === "contents" || lowerKey === "children") return List;
    return Hash;
  }

  // Format a value for display
  function formatValue(value: unknown): string {
    if (value === null || value === undefined) return "—";
    if (typeof value === "boolean") return value ? "Yes" : "No";
    if (typeof value === "string") {
      // Try to format as date if it looks like an ISO date
      if (/^\d{4}-\d{2}-\d{2}/.test(value)) {
        try {
          const date = new Date(value);
          return date.toLocaleDateString(undefined, {
            year: "numeric",
            month: "short",
            day: "numeric",
            hour: "2-digit",
            minute: "2-digit",
          });
        } catch {
          return value;
        }
      }
      return value;
    }
    if (Array.isArray(value)) {
      return value.join(", ");
    }
    if (typeof value === "object") {
      return JSON.stringify(value, null, 2);
    }
    return String(value);
  }

  // Check if a value is an array
  function isArray(value: unknown): value is unknown[] {
    return Array.isArray(value);
  }

  // Get frontmatter entries sorted with common fields first
  function getSortedFrontmatter(
    frontmatter: Record<string, unknown>,
  ): [string, unknown][] {
    const priorityKeys = [
      "title",
      "created",
      "updated",
      "date",
      "tags",
      "part_of",
      "contents",
    ];
    const entries = Object.entries(frontmatter);

    return entries.sort(([a], [b]) => {
      const aIndex = priorityKeys.indexOf(a.toLowerCase());
      const bIndex = priorityKeys.indexOf(b.toLowerCase());

      if (aIndex !== -1 && bIndex !== -1) return aIndex - bIndex;
      if (aIndex !== -1) return -1;
      if (bIndex !== -1) return 1;
      return a.localeCompare(b);
    });
  }

  // Format a key for display (convert snake_case to Title Case)
  function formatKey(key: string): string {
    return key.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  }
</script>

<!-- Mobile overlay backdrop -->
{#if !collapsed}
  <button
    type="button"
    class="fixed inset-0 bg-black/50 z-30 md:hidden"
    onclick={onToggleCollapse}
    aria-label="Close properties panel"
  ></button>
{/if}

<aside
  class="flex flex-col h-full border-l border-border bg-sidebar text-sidebar-foreground transition-all duration-300 ease-in-out
    {collapsed ? 'w-0 opacity-0 overflow-hidden md:w-0' : 'w-72'}
    fixed right-0 md:relative z-40 md:z-auto"
>
  <!-- Header -->
  <div
    class="flex items-center justify-between px-4 py-4 border-b border-sidebar-border shrink-0"
  >
    <Button
      variant="ghost"
      size="icon"
      onclick={onToggleCollapse}
      class="size-8"
      aria-label="Collapse properties panel"
    >
      <PanelRightClose class="size-4" />
    </Button>
    <h2 class="text-sm font-semibold text-sidebar-foreground">Properties</h2>
  </div>

  <!-- Content -->
  <div class="flex-1 overflow-y-auto">
    {#if entry}
      {#if Object.keys(entry.frontmatter).length > 0}
        <div class="p-3 space-y-3">
          {#each getSortedFrontmatter(entry.frontmatter) as [key, value]}
            {@const Icon = getIcon(key)}
            <div class="space-y-1">
              <div
                class="flex items-center gap-2 text-xs text-muted-foreground"
              >
                <Icon class="size-3.5" />
                <span class="font-medium">{formatKey(key)}</span>
              </div>
              <div class="pl-5.5">
                {#if isArray(value)}
                  <div class="flex flex-wrap gap-1">
                    {#each value as item}
                      <span
                        class="inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium bg-secondary text-secondary-foreground"
                      >
                        {formatValue(item)}
                      </span>
                    {/each}
                  </div>
                {:else if typeof value === "boolean"}
                  <div class="flex items-center gap-1.5">
                    <ToggleLeft
                      class="size-4 {value
                        ? 'text-primary'
                        : 'text-muted-foreground'}"
                    />
                    <span class="text-sm text-foreground"
                      >{value ? "Yes" : "No"}</span
                    >
                  </div>
                {:else}
                  <p class="text-sm text-foreground wrap-break-word">
                    {formatValue(value)}
                  </p>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div
          class="flex flex-col items-center justify-center py-8 px-4 text-center"
        >
          <FileText class="size-8 text-muted-foreground mb-2" />
          <p class="text-sm text-muted-foreground">No properties</p>
          <p class="text-xs text-muted-foreground mt-1">
            This entry has no frontmatter
          </p>
        </div>
      {/if}
    {:else}
      <div
        class="flex flex-col items-center justify-center py-8 px-4 text-center"
      >
        <FileText class="size-8 text-muted-foreground mb-2" />
        <p class="text-sm text-muted-foreground">No entry selected</p>
        <p class="text-xs text-muted-foreground mt-1">
          Select an entry to view its properties
        </p>
      </div>
    {/if}
  </div>

  <!-- Footer with path -->
  {#if entry}
    <div class="px-4 py-3 border-t border-sidebar-border shrink-0">
      <p class="text-xs text-muted-foreground truncate" title={entry.path}>
        {entry.path}
      </p>
    </div>
  {/if}
</aside>
