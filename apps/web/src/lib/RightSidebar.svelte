<script lang="ts">
  import type { EntryData } from "./backend";
  import { maybeStartWindowDrag } from "./windowDrag";
  import type { Api } from "$lib/backend/api";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import * as Alert from "$lib/components/ui/alert";
  import FilePickerPopover from "$lib/components/FilePickerPopover.svelte";
  import AudienceEditor from "$lib/components/AudienceEditor.svelte";
  import WorkspaceConfigSection from "$lib/components/WorkspaceConfigSection.svelte";
  import NestedObjectDisplay from "$lib/components/NestedObjectDisplay.svelte";
  import PluginConfigSection from "$lib/components/PluginConfigSection.svelte";
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
    Plus,
    X,
    Check,
    AlertCircle,
    Paperclip,
    Trash2,
    File,
    FileImage,
    FileArchive,
    FileSpreadsheet,
    FileCode,
    History,
    RefreshCw,
    RotateCcw,
    ArrowUpRight,
    Replace,
    Eye,
    Settings2,
    ChevronRight,
    CloudDownload,
    Download,
    CheckCircle2,
    Loader2,
    Puzzle,
  } from "@lucide/svelte";
  import type { Component } from "svelte";
  import VersionDiff from "./history/VersionDiff.svelte";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import * as Kbd from "$lib/components/ui/kbd";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import { getPlugin as getBrowserPlugin } from "$lib/plugins/browserPluginManager.svelte";
  import { getAuthState } from "$lib/auth";
  import PluginSidebarPanel from "$lib/components/PluginSidebarPanel.svelte";
  import UpgradeBanner from "$lib/components/UpgradeBanner.svelte";
  import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
  import {
    getAttachmentMetadata,
    enqueueAttachmentDownload,
    isAttachmentSyncEnabled,
  } from "$lib/sync/attachmentSyncService";
  import {
    getAttachmentAvailability,
    getAttachmentMediaKind,
    getAttachmentThumbnailUrl,
    isPreviewableAttachmentKind,
  } from "@/models/services/attachmentService";
  import { parseLinkDisplay } from "$lib/utils/linkParser";

  interface CrdtHistoryEntry {
    update_id: bigint;
    timestamp: bigint;
    origin: string;
    device_name?: string | null;
    summary?: string | null;
  }

  interface FileDiff {
    path: string;
    change_type: string;
    old_value?: string | null;
    new_value?: string | null;
  }

  // Platform detection for keyboard shortcut display
  const isMac =
    typeof navigator !== "undefined" &&
    navigator.platform.toUpperCase().indexOf("MAC") >= 0;
  const modKey = isMac ? "⌘" : "Ctrl";

  // Mobile state for hiding tooltips
  const mobileState = getMobileState();

  interface Props {
    entry: EntryData | null;
    collapsed: boolean;
    sidebarWidth?: number;
    resizing?: boolean;
    /** 0-1 during an interactive swipe gesture, null otherwise */
    swipeProgress?: number | null;
    onToggleCollapse: () => void;
    onPropertyChange?: (key: string, value: unknown) => void;
    onPropertyRemove?: (key: string) => void;
    onPropertyAdd?: (key: string, value: unknown) => void;
    titleError?: string | null;
    onTitleErrorClear?: () => void;
    onDeleteAttachment?: (attachmentPath: string) => void;
    onPreviewAttachment?: (attachmentPath: string) => void;
    attachmentError?: string | null;
    onAttachmentErrorClear?: () => void;
    // Navigation
    onOpenEntry?: (path: string) => Promise<void>;
    // History props
    rustApi?: any | null;
    onHistoryRestore?: () => void;
    // API for properties tab
    api?: Api | null;
    // External tab control (built-in tabs or plugin tab IDs)
    requestedTab?: string | null;
    onRequestedTabConsumed?: () => void;
    onPluginHostAction?: (action: { type: string; payload?: unknown }) => Promise<unknown> | unknown;
    onOpenAudienceManager?: () => void;
  }

  let {
    entry,
    collapsed,
    sidebarWidth = 288,
    resizing = false,
    swipeProgress = null,
    onToggleCollapse,
    onPropertyChange,
    onPropertyRemove,
    onPropertyAdd,
    titleError = null,
    onTitleErrorClear,
    onDeleteAttachment,
    onPreviewAttachment,
    attachmentError = null,
    onAttachmentErrorClear,
    onOpenEntry,
    rustApi = null,
    onHistoryRestore,
    api = null,
    requestedTab = null,
    onRequestedTabConsumed,
    onPluginHostAction,
    onOpenAudienceManager,
  }: Props = $props();

  // Progressive swipe derived state (mobile only – desktop keeps width-based animation)
  const swiping = $derived(swipeProgress != null);
  const isMobile = $derived(mobileState.isMobile);

  // Detect if current entry is the workspace root index
  const isRootIndex = $derived(
    entry !== null && workspaceStore.tree !== null && entry.path === workspaceStore.tree.path
  );

  // Plugin store for right sidebar tabs
  const pluginStore = getPluginStore();
  const historyTabId = $derived<string | null>(null);
  const historyTabLabel = $derived("History");
  const nonHistoryPluginTabs = $derived.by(() => {
    return pluginStore.rightSidebarTabs;
  });
  let authState = $derived(getAuthState());

  let aiPluginConfig = $state<Record<string, JsonValue>>({});
  let aiConfigLoading = $state(false);

  // Tab state — built-in "properties"/"history" + plugin tab IDs
  let activeTab: string = $state("properties");

  // Handle external tab request
  $effect(() => {
    if (requestedTab && requestedTab !== activeTab) {
      activeTab = requestedTab;
      onRequestedTabConsumed?.();
    }
  });

  $effect(() => {
    const validTabIds = new Set<string>([
      "properties",
      ...(historyTabId ? [historyTabId] : []),
      ...nonHistoryPluginTabs.map((tab) => tab.contribution.id),
    ]);
    if (!validTabIds.has(activeTab)) {
      activeTab = "properties";
    }
  });

  // History state
  let history: CrdtHistoryEntry[] = $state([]);
  let historyLoading = $state(false);
  let historyError = $state<string | null>(null);
  let selectedEntry: CrdtHistoryEntry | null = $state(null);
  let diffs: FileDiff[] = $state([]);
  let loadingDiff = $state(false);

  // Load history for current file (combines workspace metadata + body content changes)
  async function loadHistory() {
    if (!rustApi || !entry) return;

    historyLoading = true;
    historyError = null;
    selectedEntry = null;
    diffs = [];

    try {
      // Use file-specific history that combines workspace and body doc changes
      history = await rustApi.getFileHistory(entry.path, 100);
    } catch (e) {
      historyError = e instanceof Error ? e.message : "Failed to load history";
      console.error("[RightSidebar] Error loading history:", e);
    } finally {
      historyLoading = false;
    }
  }

  // Select a history entry and load its diff
  async function selectHistoryEntry(historyEntry: CrdtHistoryEntry) {
    if (!rustApi) return;

    if (selectedEntry?.update_id === historyEntry.update_id) {
      // Deselect
      selectedEntry = null;
      diffs = [];
      return;
    }

    selectedEntry = historyEntry;
    loadingDiff = true;
    diffs = [];

    try {
      const idx = history.findIndex((h) => h.update_id === historyEntry.update_id);
      if (idx < history.length - 1) {
        const previousEntry = history[idx + 1];
        // Diff operates on workspace document for metadata changes
        diffs = await rustApi.getVersionDiff(previousEntry.update_id, historyEntry.update_id, "workspace");
      }
    } catch (e) {
      console.error("[RightSidebar] Error loading diff:", e);
    } finally {
      loadingDiff = false;
    }
  }

  // Restore to a specific version
  async function restoreVersion(historyEntry: CrdtHistoryEntry) {
    if (!rustApi || !entry) return;

    const confirmRestore = confirm(`Restore to version from ${formatTimestamp(historyEntry.timestamp)}?`);
    if (!confirmRestore) return;

    try {
      // Restore operates on workspace document for metadata
      await rustApi.restoreVersion(historyEntry.update_id, "workspace");
      onHistoryRestore?.();
      await loadHistory();
    } catch (e) {
      console.error("[RightSidebar] Error restoring version:", e);
      alert("Failed to restore version");
    }
  }

  function formatTimestamp(timestamp: bigint): string {
    const date = new Date(Number(timestamp));
    return date.toLocaleString();
  }

  function formatRelativeTime(timestamp: bigint): string {
    const now = Date.now();
    const diff = now - Number(timestamp);
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (minutes < 1) return "Just now";
    if (minutes < 60) return `${minutes}m ago`;
    if (hours < 24) return `${hours}h ago`;
    return `${days}d ago`;
  }

  function getOriginLabel(entry: CrdtHistoryEntry): string {
    // Show device name if available
    if (entry.device_name) {
      if (entry.origin === "local") {
        return `You (${entry.device_name})`;
      }
      return entry.device_name;
    }
    // Fallback to origin-based label
    switch (entry.origin) {
      case "local": return "You";
      case "remote": return "Remote";
      case "sync": return "Sync";
      default: return entry.origin;
    }
  }

  function getOriginClass(origin: string): string {
    switch (origin) {
      case "Local": return "bg-primary text-primary-foreground";
      case "Remote": return "bg-secondary text-secondary-foreground";
      case "Sync": return "bg-accent text-accent-foreground";
      default: return "bg-muted text-muted-foreground";
    }
  }

  // Load history when switching to history tab or when entry changes
  $effect(() => {
    if (activeTab === historyTabId && entry && rustApi) {
      loadHistory();
    }
  });

  function isManagedMode(config: Record<string, JsonValue>): boolean {
    const mode = config.provider_mode;
    return typeof mode === "string" && mode.toLowerCase() === "managed";
  }

  async function loadAiPluginConfig() {
    if (!api) return;
    aiConfigLoading = true;
    try {
      const browserPlugin = getBrowserPlugin("diaryx.ai");
      if (browserPlugin) {
        const raw = await browserPlugin.getConfig();
        aiPluginConfig = (raw as Record<string, JsonValue>) ?? {};
        return;
      }
      const raw = await api.getPluginConfig("diaryx.ai");
      aiPluginConfig = (raw as Record<string, JsonValue>) ?? {};
    } catch {
      aiPluginConfig = {};
    } finally {
      aiConfigLoading = false;
    }
  }

  $effect(() => {
    const pluginTab = nonHistoryPluginTabs.find((t) => t.contribution.id === activeTab);
    if (!pluginTab || String(pluginTab.pluginId) !== "diaryx.ai") return;
    void loadAiPluginConfig();
  });

  // Reset history state when entry changes
  $effect(() => {
    if (entry) {
      history = [];
      selectedEntry = null;
      diffs = [];
    }
  });

  // Get attachments from frontmatter
  $effect(() => {
    if (attachmentError && entry) {
      // Auto-clear error after 5 seconds
      const timeout = setTimeout(() => onAttachmentErrorClear?.(), 5000);
      return () => clearTimeout(timeout);
    }
  });

  // Get attachments list from frontmatter
  function getAttachments(): string[] {
    if (!entry?.frontmatter?.attachments) return [];
    const attachments = entry.frontmatter.attachments;
    if (Array.isArray(attachments)) {
      return attachments.filter((a): a is string => typeof a === "string");
    }
    return [];
  }

  function getFilename(path: string): string {
    return path.split("/").pop() ?? path;
  }

  // Extract display name from an attachment value (may be a markdown link or plain path)
  function getAttachmentDisplayName(attachment: string): string {
    const parsed = parseLinkDisplay(attachment);
    if (parsed) return parsed.title || getFilename(parsed.path);
    return getFilename(attachment);
  }

  // Extract the actual file path from an attachment value (may be a markdown link or plain path)
  function getAttachmentPath(attachment: string): string {
    const parsed = parseLinkDisplay(attachment);
    if (parsed) return parsed.path;
    return attachment;
  }

  // Get file type icon based on extension
  function getFileIcon(filename: string): Component {
    const mediaKind = getAttachmentMediaKind(filename);
    const ext = filename.split('.').pop()?.toLowerCase() || '';
    const docExts = ['pdf', 'doc', 'docx', 'txt', 'md', 'rtf'];
    const spreadsheetExts = ['xls', 'xlsx', 'csv'];
    const archiveExts = ['zip', 'tar', 'gz', '7z', 'rar'];
    const codeExts = ['json', 'xml', 'html', 'css', 'js', 'ts'];

    if (mediaKind === 'image') return FileImage;
    if (docExts.includes(ext)) return FileText;
    if (spreadsheetExts.includes(ext)) return FileSpreadsheet;
    if (archiveExts.includes(ext)) return FileArchive;
    if (codeExts.includes(ext)) return FileCode;
    return File;
  }

  // Attachment availability: 'local' | 'remote' | 'downloading' | 'unknown'
  type AttachmentStatus = 'local' | 'remote' | 'downloading' | 'unknown';
  let attachmentStatuses = $state<Map<string, AttachmentStatus>>(new Map());

  // Check local availability for all attachments when entry changes
  $effect(() => {
    if (!entry || !api) return;
    const attachments = getAttachments();
    if (attachments.length === 0) return;
    const entryPath = entry.path;
    const currentApi = api;
    const statuses = new Map<string, AttachmentStatus>();
    let cancelled = false;

    // Start with 'unknown' then probe each attachment
    for (const attachment of attachments) {
      const attachPath = getAttachmentPath(attachment);
      statuses.set(attachPath, 'unknown');
    }
    attachmentStatuses = new Map(statuses);

    // Probe each attachment asynchronously
    void (async () => {
      let changed = false;
      for (const attachment of attachments) {
        if (cancelled) return;
        const attachPath = getAttachmentPath(attachment);
        const availability = await getAttachmentAvailability(
          currentApi,
          entryPath,
          attachPath,
        );
        if (cancelled) return;
        if (statuses.get(attachPath) !== availability) {
          statuses.set(attachPath, availability);
          changed = true;
        }
      }
      if (!cancelled && changed) {
        attachmentStatuses = new Map(statuses);
      }
    })();

    return () => {
      cancelled = true;
    };
  });

  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }

  function downloadAttachment(attachPath: string): void {
    if (!entry) return;
    const meta = getAttachmentMetadata(entry.path, attachPath);
    if (!meta) return;
    attachmentStatuses.set(attachPath, 'downloading');
    attachmentStatuses = new Map(attachmentStatuses);
    enqueueAttachmentDownload({
      workspaceId: meta.workspaceId,
      entryPath: entry.path,
      attachmentPath: attachPath,
      hash: meta.hash,
      mimeType: meta.mimeType,
      sizeBytes: meta.sizeBytes,
    });
  }

  function downloadAllRemoteAttachments(): void {
    if (!entry) return;
    for (const attachment of getAttachments()) {
      const attachPath = getAttachmentPath(attachment);
      if (attachmentStatuses.get(attachPath) === 'remote') {
        downloadAttachment(attachPath);
      }
    }
  }

  const hasRemoteAttachments = $derived(
    [...attachmentStatuses.values()].some(s => s === 'remote')
  );

  // Attachment thumbnail cache (attachPath -> blob URL)
  let attachmentThumbnails = $state<Map<string, string>>(new Map());

  // Load thumbnails for image attachments when the section is expanded
  $effect(() => {
    if (attachmentsCollapsed || !entry || !api) return;
    const attachments = getAttachments();
    if (attachments.length === 0) return;
    const entryPath = entry.path;
    const currentApi = api;
    let cancelled = false;

    void (async () => {
      let changed = false;
      for (const attachment of attachments) {
        if (cancelled) return;
        const attachPath = getAttachmentPath(attachment);
        if (attachmentThumbnails.has(attachPath)) continue;
        const kind = getAttachmentMediaKind(attachPath);
        if (kind !== "image") continue;
        // Only load thumbnail if the file is available locally
        const status = attachmentStatuses.get(attachPath);
        if (status !== "local" && status !== undefined) continue;
        try {
          const url = await getAttachmentThumbnailUrl(currentApi, entryPath, attachPath);
          if (cancelled) return;
          if (url) {
            attachmentThumbnails.set(attachPath, url);
            changed = true;
          }
        } catch {
          // Ignore thumbnail load failures
        }
      }
      if (!cancelled && changed) {
        attachmentThumbnails = new Map(attachmentThumbnails);
      }
    })();

    return () => { cancelled = true; };
  });

  // Clear thumbnails when entry changes
  $effect(() => {
    void entry?.path;
    attachmentThumbnails = new Map();
  });

  // Collapsible section state
  let audienceCollapsed = $state(true);
  let configCollapsed = $state(true);
  let pluginsCollapsed = $state(true);
  let attachmentsCollapsed = $state(true);
  let collapseTooltipOpen = $state(false);

  // State for adding new properties
  let showAddProperty = $state(false);
  let newPropertyKey = $state("");
  let newPropertyValue = $state("");

  // State for adding new array items
  let addingArrayItemKey = $state<string | null>(null);
  let newArrayItem = $state("");

  // Resolve a link string to a workspace path and navigate to it
  async function navigateToLink(link: string) {
    if (!api || !entry) return;
    try {
      const resolved = await api.canonicalizeLink(link, entry.path);
      await onOpenEntry?.(resolved);
    } catch {
      // Fallback: try the raw value
      await onOpenEntry?.(link);
    }
  }

  // Format a selected file as a markdown link using the WASM api
  async function formatAsLink(filePath: string, fileName: string): Promise<string> {
    if (!api || !entry) return `[${fileName}](/${filePath})`;
    try {
      return await api.formatLink(filePath, fileName, "markdown_root", entry.path);
    } catch {
      return `[${fileName}](/${filePath})`;
    }
  }

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

  // Check if a value is an array
  function isArray(value: unknown): value is unknown[] {
    return Array.isArray(value);
  }

  // Check if a value looks like a date
  function isDateValue(key: string, value: unknown): boolean {
    if (typeof value !== "string") return false;
    const lowerKey = key.toLowerCase();
    const dateKeys = ["created", "updated", "date", "modified"];
    return dateKeys.includes(lowerKey) || /^\d{4}-\d{2}-\d{2}/.test(value);
  }

  // Keys that have dedicated UI sections and should not appear in generic metadata
  const DEDICATED_SECTION_KEYS = ["attachments", "audience"];

  // Workspace config keys that are shown in the dedicated config section (root index only)
  const WORKSPACE_CONFIG_KEYS = [
    "link_format",
    "default_template",
    "sync_title_to_heading",
    "auto_update_timestamp",
    "auto_rename_to_title",
    "filename_style",
    "default_audience",
  ];

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
      "audience",
      "part_of",
      "contents",
    ];
    const entries = Object.entries(frontmatter).filter(
      ([key]) =>
        !DEDICATED_SECTION_KEYS.includes(key.toLowerCase()) &&
        !(isRootIndex && WORKSPACE_CONFIG_KEYS.includes(key.toLowerCase())) &&
        !(isRootIndex && key.toLowerCase() === "plugins"),
    );

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

  // Handle string property change
  function handleStringChange(key: string, event: Event) {
    const target = event.target as HTMLInputElement;
    onPropertyChange?.(key, target.value);
  }

  // Handle boolean toggle
  function handleBooleanToggle(key: string, currentValue: boolean) {
    onPropertyChange?.(key, !currentValue);
  }

  // Handle array item removal
  function handleArrayItemRemove(key: string, index: number) {
    if (!entry) return;
    const currentArray = entry.frontmatter[key] as unknown[];
    const newArray = [...currentArray];
    newArray.splice(index, 1);
    onPropertyChange?.(key, newArray);
  }

  // Handle contents item removal — also deletes the linked file
  async function handleContentsItemRemove(key: string, index: number) {
    if (!entry) return;
    const currentArray = entry.frontmatter[key] as unknown[];
    const item = currentArray[index];
    const parsed = parseLinkDisplay(String(item));
    if (parsed?.path && api) {
      try {
        await api.deleteEntry(parsed.path);
      } catch (e) {
        console.error(`[RightSidebar] Failed to delete entry ${parsed.path}:`, e);
      }
    }
    const newArray = [...currentArray];
    newArray.splice(index, 1);
    onPropertyChange?.(key, newArray);
  }

  // Handle adding new array item
  function handleAddArrayItem(key: string) {
    if (!entry || !newArrayItem.trim()) return;
    const currentArray = (entry.frontmatter[key] as unknown[]) || [];
    const newArray = [...currentArray, newArrayItem.trim()];
    onPropertyChange?.(key, newArray);
    newArrayItem = "";
    addingArrayItemKey = null;
  }

  // Handle adding new property
  function handleAddProperty() {
    if (!newPropertyKey.trim()) return;

    // Try to parse as JSON for complex values, otherwise use as string
    let value: unknown = newPropertyValue;
    try {
      value = JSON.parse(newPropertyValue);
    } catch {
      // Keep as string
    }

    onPropertyAdd?.(newPropertyKey.trim(), value);
    newPropertyKey = "";
    newPropertyValue = "";
    showAddProperty = false;
  }

  // Handle key press in inputs
  function handleKeyPress(event: KeyboardEvent, callback: () => void) {
    if (event.key === "Enter") {
      event.preventDefault();
      callback();
    }
    if (event.key === "Escape") {
      showAddProperty = false;
      addingArrayItemKey = null;
      newArrayItem = "";
    }
  }

  // Format date for datetime-local input
  function formatDateForInput(value: string): string {
    try {
      const date = new Date(value);
      // Format as YYYY-MM-DDTHH:mm
      return date.toISOString().slice(0, 16);
    } catch {
      return value;
    }
  }

  // Parse datetime-local input back to ISO string
  function parseDateFromInput(value: string): string {
    try {
      const date = new Date(value);
      return date.toISOString();
    } catch {
      return value;
    }
  }

  function handleCollapseClick(event: MouseEvent): void {
    collapseTooltipOpen = false;
    if (event.currentTarget instanceof HTMLElement) {
      event.currentTarget.blur();
    }
    onToggleCollapse();
  }

  $effect(() => {
    if (collapsed) {
      collapseTooltipOpen = false;
    }
  });
</script>

<!-- Mobile overlay backdrop -->
{#if !collapsed || (swipeProgress != null && swipeProgress > 0)}
  <button
    type="button"
    class="fixed inset-0 z-30 md:hidden {swipeProgress != null ? 'pointer-events-none' : ''}"
    style="background: rgba(0,0,0,{swipeProgress != null ? swipeProgress * 0.5 : 0.5}); {swipeProgress != null ? '' : 'transition: background 0.3s ease-in-out;'}"
    onclick={onToggleCollapse}
    aria-label="Close properties panel"
  ></button>
{/if}

<aside
  class="flex flex-col h-full border-l border-border bg-sidebar text-sidebar-foreground shrink-0 select-none
    fixed right-0 md:relative z-40 md:z-auto
    {!isMobile && collapsed ? 'opacity-0 overflow-hidden' : ''}
    {isMobile ? 'overflow-hidden' : ''}"
  style="{isMobile
    ? `width: ${sidebarWidth}px; transform: translateX(${swiping ? (100 - (swipeProgress as unknown as number) * 100) : (collapsed ? 100 : 0)}%); ${swiping ? '' : 'transition: transform 0.3s ease-in-out;'}`
    : `width: ${collapsed ? 0 : sidebarWidth}px; ${resizing ? '' : 'transition: width 0.3s ease-in-out, opacity 0.3s ease-in-out;'}`
  }"
  data-spotlight="properties-panel"
>
  <!-- Header with collapse button -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="relative flex items-center justify-between px-2 md:px-4 py-1.5 md:py-3 border-b border-sidebar-border shrink-0 pt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height))] md:pt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height)+0.75rem)] bg-sidebar-accent"
    onmousedown={maybeStartWindowDrag}
  >
    <Tooltip.Root bind:open={collapseTooltipOpen}>
      <Tooltip.Trigger>
        <Button
          variant="ghost"
          size="icon"
          onclick={handleCollapseClick}
          data-window-drag-exclude
          class="size-10 md:size-8"
          aria-label="Collapse panel"
        >
          <PanelRightClose class="size-5 md:size-4" />
        </Button>
      </Tooltip.Trigger>
      {#if !mobileState.isMobile && !collapsed}
        <Tooltip.Content>
          <div class="flex items-center gap-2">
            Collapse panel
            <Kbd.Group>
              <Kbd.Root>{modKey}</Kbd.Root>
              <span>+</span>
              <Kbd.Root>]</Kbd.Root>
            </Kbd.Group>
          </div>
        </Tooltip.Content>
      {/if}
    </Tooltip.Root>

    {#if entry}
      <p class="text-xs text-muted-foreground truncate flex-1 min-w-0" title={entry.path}>
        {entry.path}
      </p>
    {/if}
  </div>

  <!-- Content -->
  <div class="flex-1 overflow-y-auto overflow-x-hidden">
    {#if activeTab === "properties"}
      <!-- Properties Tab -->
      {#if entry}
      {#if Object.keys(entry.frontmatter).length > 0}
        <div class="p-3 space-y-3">
          {#each getSortedFrontmatter(entry.frontmatter) as [key, value]}
            {@const Icon = getIcon(key)}
            <div class="space-y-1 group">
              <div
                class="flex items-center justify-between text-xs text-muted-foreground"
              >
                <div class="flex items-center gap-2">
                  <Icon class="size-3.5" />
                  <span class="font-medium">{formatKey(key)}</span>
                </div>
                <!-- Delete button -->
                <Button
                  variant="ghost"
                  size="icon"
                  class="size-11 md:size-5 opacity-0 group-hover:opacity-100 transition-opacity"
                  onclick={() => onPropertyRemove?.(key)}
                  aria-label="Remove property"
                >
                  <X class="size-4 md:size-3" />
                </Button>
              </div>
              <div class="pl-5.5">
                {#if isArray(value) && key === 'contents'}
                  <!-- Contents array with file picker -->
                  <div class="space-y-1">
                    <div class="flex flex-wrap gap-1">
                      {#each value as item, i}
                        {@const parsed = parseLinkDisplay(String(item))}
                        <span
                          class="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-secondary text-secondary-foreground"
                        >
                          {#if parsed}
                            <button
                              type="button"
                              class="hover:underline cursor-pointer"
                              onclick={() => navigateToLink(String(item))}
                              title={parsed.path}
                            >
                              {parsed.title || parsed.path}
                            </button>
                          {:else}
                            {item}
                          {/if}
                          <button
                            type="button"
                            class="text-muted-foreground hover:text-destructive transition-colors p-2.5 md:p-0"
                            onclick={() => handleContentsItemRemove(key, i)}
                            aria-label="Remove item"
                          >
                            <X class="size-4 md:size-3" />
                          </button>
                        </span>
                      {/each}
                    </div>
                    <FilePickerPopover
                      excludePaths={value.map(String).map((v) => parseLinkDisplay(v)?.path).filter((p): p is string => !!p)}
                      placeholder="Add child entry..."
                      onSelect={async (file) => {
                        const link = await formatAsLink(file.path, file.name);
                        const currentArray = (entry?.frontmatter[key] as unknown[]) || [];
                        onPropertyChange?.(key, [...currentArray, link]);
                      }}
                    >
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-11 md:h-6 text-sm md:text-xs px-2 mt-1"
                      >
                        <Plus class="size-4 md:size-3 mr-1" />
                        Add
                      </Button>
                    </FilePickerPopover>
                  </div>
                {:else if isArray(value)}
                  <!-- Generic array editor -->
                  <div class="space-y-1">
                    <div class="flex flex-wrap gap-1">
                      {#each value as item, i}
                        <span
                          class="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-secondary text-secondary-foreground group/tag"
                        >
                          {item}
                          <button
                            type="button"
                            class="opacity-0 group-hover/tag:opacity-100 hover:text-destructive transition-opacity p-2.5 md:p-0"
                            onclick={() => handleArrayItemRemove(key, i)}
                            aria-label="Remove item"
                          >
                            <X class="size-4 md:size-3" />
                          </button>
                        </span>
                      {/each}
                    </div>
                    {#if addingArrayItemKey === key}
                      <div class="flex items-center gap-1 mt-1">
                        <Input
                          type="text"
                          bind:value={newArrayItem}
                          class="h-7 text-base md:text-xs"
                          placeholder="New item..."
                          onkeydown={(e) =>
                            handleKeyPress(e, () => handleAddArrayItem(key))}
                        />
                        <Button
                          variant="ghost"
                          size="icon"
                          class="size-11 md:size-6"
                          onclick={() => handleAddArrayItem(key)}
                        >
                          <Check class="size-4 md:size-3" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="size-11 md:size-6"
                          onclick={() => {
                            addingArrayItemKey = null;
                            newArrayItem = "";
                          }}
                        >
                          <X class="size-4 md:size-3" />
                        </Button>
                      </div>
                    {:else}
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-11 md:h-6 text-sm md:text-xs px-2 mt-1"
                        onclick={() => (addingArrayItemKey = key)}
                      >
                        <Plus class="size-4 md:size-3 mr-1" />
                        Add
                      </Button>
                    {/if}
                  </div>
                {:else if typeof value === "boolean"}
                  <!-- Boolean toggle -->
                  <button
                    type="button"
                    class="flex items-center gap-1.5 cursor-pointer hover:opacity-80 transition-opacity"
                    onclick={() => handleBooleanToggle(key, value)}
                  >
                    <ToggleLeft
                      class="size-4 {value
                        ? 'text-primary'
                        : 'text-muted-foreground'}"
                    />
                    <span class="text-sm text-foreground"
                      >{value ? "Yes" : "No"}</span
                    >
                  </button>
                {:else if isDateValue(key, value)}
                  <!-- Date input -->
                  <Input
                    type="datetime-local"
                    value={formatDateForInput(String(value))}
                    class="h-8 text-base md:text-sm min-w-0 w-full"
                    onchange={(e) => {
                      const target = e.target as HTMLInputElement;
                      onPropertyChange?.(key, parseDateFromInput(target.value));
                    }}
                  />
                {:else if key === 'part_of'}
                  <!-- part_of with file picker -->
                  {@const partOfParsed = parseLinkDisplay(String(value ?? ""))}
                  <div class="space-y-1">
                    {#if value}
                      <div class="flex items-center gap-1">
                        <button
                          type="button"
                          class="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-secondary text-secondary-foreground hover:underline cursor-pointer"
                          onclick={() => navigateToLink(String(value))}
                          title={partOfParsed?.path ?? String(value)}
                        >
                          <ArrowUpRight class="size-3" />
                          {partOfParsed?.title || partOfParsed?.path || String(value)}
                        </button>
                        <Button
                          variant="ghost"
                          size="icon"
                          class="size-11 md:size-5"
                          onclick={() => onPropertyChange?.(key, "")}
                          aria-label="Clear parent"
                        >
                          <X class="size-4 md:size-3" />
                        </Button>
                      </div>
                    {/if}
                    <FilePickerPopover
                      excludePaths={entry ? [entry.path] : []}
                      placeholder="Search for parent..."
                      onSelect={async (file) => {
                        const link = await formatAsLink(file.path, file.name);
                        onPropertyChange?.(key, link);
                      }}
                    >
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-11 md:h-6 text-xs px-2"
                      >
                        {#if value}
                          <Replace class="size-3 mr-1" />
                          Change
                        {:else}
                          <Plus class="size-3 mr-1" />
                          Set parent
                        {/if}
                      </Button>
                    </FilePickerPopover>
                  </div>
                {:else if typeof value === 'object' && value !== null && !Array.isArray(value)}
                  <!-- Nested object display -->
                  <NestedObjectDisplay data={value as Record<string, unknown>} onNavigateLink={navigateToLink} />
                {:else}
                  <!-- String input (with link detection) -->
                  {@const linkParsed = parseLinkDisplay(String(value ?? ""))}
                  {#if linkParsed}
                    <button
                      type="button"
                      class="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-secondary text-secondary-foreground hover:underline cursor-pointer"
                      onclick={() => navigateToLink(String(value))}
                      title={linkParsed.path}
                    >
                      <ArrowUpRight class="size-3" />
                      {linkParsed.title || linkParsed.path}
                    </button>
                  {:else}
                    <Input
                      type="text"
                      value={String(value ?? "")}
                      class="h-8 text-base md:text-sm {key.toLowerCase() ===
                        'title' && titleError
                        ? 'border-destructive'
                        : ''}"
                      onblur={(e) => handleStringChange(key, e)}
                      onfocus={() => {
                        if (key.toLowerCase() === "title") onTitleErrorClear?.();
                      }}
                      onkeydown={(e) => {
                        if (e.key === "Enter") {
                          handleStringChange(key, e);
                          (e.target as HTMLInputElement).blur();
                        }
                      }}
                    />
                    {#if key.toLowerCase() === "title" && titleError}
                      <Alert.Root variant="destructive" class="mt-2 py-2">
                        <AlertCircle class="size-4" />
                        <Alert.Description class="text-xs">
                          {titleError}
                        </Alert.Description>
                      </Alert.Root>
                    {/if}
                  {/if}
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
            Add frontmatter properties below
          </p>
        </div>
      {/if}

      <!-- Add Property Section -->
      <div class="p-3 border-t border-sidebar-border">
        {#if showAddProperty}
          <div class="space-y-2">
            <Input
              type="text"
              bind:value={newPropertyKey}
              class="h-8 text-base md:text-sm"
              placeholder="Property name..."
              onkeydown={(e) => handleKeyPress(e, handleAddProperty)}
            />
            <Input
              type="text"
              bind:value={newPropertyValue}
              class="h-8 text-base md:text-sm"
              placeholder="Value..."
              onkeydown={(e) => handleKeyPress(e, handleAddProperty)}
            />
            <div class="flex gap-2">
              <Button
                variant="default"
                size="sm"
                class="flex-1 h-11 md:h-7 text-xs"
                onclick={handleAddProperty}
              >
                <Check class="size-3 mr-1" />
                Add
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="h-11 md:h-7 text-xs"
                onclick={() => {
                  showAddProperty = false;
                  newPropertyKey = "";
                  newPropertyValue = "";
                }}
              >
                Cancel
              </Button>
            </div>
          </div>
        {:else}
          <Button
            variant="outline"
            size="sm"
            class="w-full h-11 md:h-8 text-xs"
            onclick={() => (showAddProperty = true)}
          >
            <Plus class="size-3 mr-1" />
            Add Property
          </Button>
        {/if}
      </div>

      <!-- Audience Section -->
      <div class="p-3 border-t border-sidebar-border">
        <button
          type="button"
          class="flex items-center justify-between w-full cursor-pointer {audienceCollapsed ? '' : 'mb-2'}"
          onclick={() => audienceCollapsed = !audienceCollapsed}
        >
          <div class="flex items-center gap-2 text-xs text-muted-foreground">
            <Eye class="size-4.5 md:size-3.5" />
            <span class="font-medium">Audience</span>
          </div>
          <ChevronRight class="size-4.5 md:size-3.5 text-muted-foreground transition-transform {audienceCollapsed ? '' : 'rotate-90'}" />
        </button>
        {#if !audienceCollapsed}
        <AudienceEditor
          audience={entry.frontmatter.audience as string[] | null ?? null}
          entryPath={entry.path}
          rootPath={workspaceStore.tree?.path ?? ""}
          {api}
          onChange={(value) => {
            if (value === null) {
              onPropertyRemove?.("audience");
            } else {
              onPropertyChange?.("audience", value);
            }
          }}
          onOpenManager={onOpenAudienceManager}
        />
        {/if}
      </div>

      <!-- Workspace Config Section (root index only) -->
      {#if isRootIndex}
        <div class="p-3 border-t border-sidebar-border">
          <button
            type="button"
            class="flex items-center justify-between w-full cursor-pointer {configCollapsed ? '' : 'mb-2'}"
            onclick={() => configCollapsed = !configCollapsed}
          >
            <div class="flex items-center gap-2 text-xs text-muted-foreground">
              <Settings2 class="size-4.5 md:size-3.5" />
              <span class="font-medium">Workspace Config</span>
            </div>
            <ChevronRight class="size-4.5 md:size-3.5 text-muted-foreground transition-transform {configCollapsed ? '' : 'rotate-90'}" />
          </button>
          {#if !configCollapsed}
            <WorkspaceConfigSection rootIndexPath={entry.path} />
          {/if}
        </div>

        <!-- Plugins Section (root index only) -->
        {#if entry.frontmatter.plugins && typeof entry.frontmatter.plugins === 'object' && !Array.isArray(entry.frontmatter.plugins)}
          <div class="p-3 border-t border-sidebar-border">
            <button
              type="button"
              class="flex items-center justify-between w-full cursor-pointer {pluginsCollapsed ? '' : 'mb-2'}"
              onclick={() => pluginsCollapsed = !pluginsCollapsed}
            >
              <div class="flex items-center gap-2 text-xs text-muted-foreground">
                <Puzzle class="size-4.5 md:size-3.5" />
                <span class="font-medium">Plugins</span>
              </div>
              <ChevronRight class="size-4.5 md:size-3.5 text-muted-foreground transition-transform {pluginsCollapsed ? '' : 'rotate-90'}" />
            </button>
            {#if !pluginsCollapsed}
              <PluginConfigSection
                plugins={entry.frontmatter.plugins as Record<string, unknown>}
                onNavigateLink={navigateToLink}
              />
            {/if}
          </div>
        {/if}
      {/if}

      <!-- Attachments Section -->
      <div class="p-3 border-t border-sidebar-border">
        <button
          type="button"
          class="flex items-center justify-between w-full cursor-pointer {attachmentsCollapsed ? '' : 'mb-2'}"
          onclick={() => attachmentsCollapsed = !attachmentsCollapsed}
        >
          <div class="flex items-center gap-2 text-xs text-muted-foreground">
            <Paperclip class="size-4.5 md:size-3.5" />
            <span class="font-medium">Attachments</span>
          </div>
          <ChevronRight class="size-4.5 md:size-3.5 text-muted-foreground transition-transform {attachmentsCollapsed ? '' : 'rotate-90'}" />
        </button>

        {#if !attachmentsCollapsed}
        {#if attachmentError}
          <Alert.Root variant="destructive" class="mb-2 py-2">
            <AlertCircle class="size-4" />
            <Alert.Description class="text-xs">
              {attachmentError}
            </Alert.Description>
          </Alert.Root>
        {/if}

        {#if getAttachments().length > 0}
          <div class="space-y-1 mb-2">
            {#each getAttachments() as attachment}
              {@const displayName = getAttachmentDisplayName(attachment)}
              {@const attachPath = getAttachmentPath(attachment)}
              {@const Icon = getFileIcon(attachPath)}
              {@const previewKind = getAttachmentMediaKind(attachPath)}
              {@const canPreview = isPreviewableAttachmentKind(previewKind)}
              {@const status = attachmentStatuses.get(attachPath) ?? 'unknown'}
              {@const meta = entry ? getAttachmentMetadata(entry.path, attachPath) : null}
              {@const thumbUrl = attachmentThumbnails.get(attachPath)}
              <div
                class="flex items-center justify-between gap-2 px-2 py-1.5 rounded-md bg-secondary/50 group cursor-grab active:cursor-grabbing"
                role="listitem"
                aria-label="Attachment: {displayName}, drag to move"
                draggable="true"
                ondragstart={(e) => {
                  if (e.dataTransfer && entry) {
                    e.dataTransfer.setData('text/x-diaryx-attachment', attachment);
                    e.dataTransfer.setData('text/x-diaryx-source-entry', entry.path);
                    e.dataTransfer.effectAllowed = 'move';
                  }
                }}
              >
                <button
                  type="button"
                  class="flex items-center gap-2 min-w-0 {canPreview ? 'hover:text-primary cursor-pointer' : 'cursor-default'}"
                  onclick={() => canPreview && onPreviewAttachment?.(attachment)}
                  disabled={!canPreview}
                >
                  {#if thumbUrl}
                    <img src={thumbUrl} alt="" class="size-8 rounded object-cover shrink-0" draggable="false" />
                  {:else}
                    <Icon class="size-3.5 shrink-0 text-muted-foreground" />
                  {/if}
                  <div class="flex flex-col min-w-0">
                    <span
                      class="text-xs text-foreground truncate {canPreview ? 'hover:underline' : ''}"
                      title={canPreview ? `Preview ${displayName}` : attachPath}
                    >
                      {displayName}
                    </span>
                    {#if meta && (status === 'remote' || status === 'downloading')}
                      <span class="text-[10px] text-muted-foreground">{formatFileSize(meta.sizeBytes)}</span>
                    {/if}
                  </div>
                </button>
                <div class="flex items-center gap-0.5 shrink-0">
                  {#if status === 'local'}
                    <CheckCircle2 class="size-3 text-green-500" />
                  {:else if status === 'downloading'}
                    <Loader2 class="size-3 text-muted-foreground animate-spin" />
                  {:else if status === 'remote'}
                    <Button
                      variant="ghost"
                      size="icon"
                      class="size-11 md:size-5"
                      onclick={(e: MouseEvent) => { e.stopPropagation(); downloadAttachment(attachPath); }}
                      aria-label="Download attachment"
                    >
                      <Download class="size-4 md:size-3 text-muted-foreground" />
                    </Button>
                  {/if}
                  <Button
                    variant="ghost"
                    size="icon"
                    class="size-11 md:size-5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
                    onclick={() => onDeleteAttachment?.(attachment)}
                    aria-label="Remove attachment"
                  >
                    <Trash2 class="size-4 md:size-3" />
                  </Button>
                </div>
              </div>
            {/each}
          </div>
          {#if isAttachmentSyncEnabled() && hasRemoteAttachments}
            <Button
              variant="outline"
              size="sm"
              class="w-full h-11 md:h-7 text-xs mb-2"
              onclick={() => downloadAllRemoteAttachments()}
            >
              <CloudDownload class="size-3 mr-1" />
              Download All
            </Button>
          {/if}
        {:else}
          <p class="text-xs text-muted-foreground mb-2">No attachments</p>
        {/if}

        <FilePickerPopover
          excludePaths={getAttachments().map(getAttachmentPath)}
          placeholder="Add attachment..."
          onSelect={async (file) => {
            const link = await formatAsLink(file.path, file.name);
            const currentAttachments = getAttachments();
            onPropertyChange?.("attachments", [...currentAttachments, link]);
          }}
        >
          <Button
            variant="outline"
            size="sm"
            class="w-full h-11 md:h-7 text-xs"
          >
            <Plus class="size-4 md:size-3 mr-1" />
            Add Attachment
          </Button>
        </FilePickerPopover>
        {/if}
      </div>
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
    {:else if activeTab !== "properties" && activeTab !== historyTabId}
      <!-- Plugin Tab -->
      {@const pluginTab = nonHistoryPluginTabs.find(t => t.contribution.id === activeTab)}
      {#if pluginTab && api}
        <div class="h-full">
          {#if String(pluginTab.pluginId) === "diaryx.ai" && aiConfigLoading}
            <div class="h-full flex items-center justify-center text-sm text-muted-foreground">
              Loading...
            </div>
          {:else if String(pluginTab.pluginId) === "diaryx.ai" && authState.tier !== "plus" && isManagedMode(aiPluginConfig)}
            <UpgradeBanner
              feature="Managed AI"
              description="Upgrade to Diaryx Plus to use managed AI without your own API key."
            />
          {:else}
            <PluginSidebarPanel
              pluginId={pluginTab.pluginId}
              component={pluginTab.contribution.component}
              {api}
              {entry}
              onHostAction={onPluginHostAction}
            />
          {/if}
        </div>
      {/if}
    {:else if activeTab === historyTabId}
      <!-- History Tab (CRDT Changes) -->
      {#if entry}
        <div class="p-3">
          <!-- History Header -->
          <div class="flex items-center justify-between mb-3">
            <div class="flex items-center gap-2 text-xs text-muted-foreground">
              <History class="size-3.5" />
              <span class="font-medium">Version History</span>
            </div>
            <Button
              variant="ghost"
              size="icon"
              class="size-6"
              onclick={loadHistory}
              disabled={historyLoading}
              aria-label="Refresh history"
            >
              <RefreshCw class="size-3 {historyLoading ? 'animate-spin' : ''}" />
            </Button>
          </div>

          {#if historyError}
            <Alert.Root variant="destructive" class="mb-3 py-2">
              <AlertCircle class="size-4" />
              <Alert.Description class="text-xs">
                {historyError}
              </Alert.Description>
            </Alert.Root>
          {/if}

          {#if historyLoading && history.length === 0}
            <div class="flex items-center justify-center py-8">
              <RefreshCw class="size-5 animate-spin text-muted-foreground" />
            </div>
          {:else if history.length === 0}
            <div class="flex flex-col items-center justify-center py-8 text-center">
              <History class="size-8 text-muted-foreground mb-2" />
              <p class="text-sm text-muted-foreground">No history available</p>
              <p class="text-xs text-muted-foreground mt-1">
                Changes will appear here
              </p>
            </div>
          {:else}
            <!-- History Entries -->
            <div class="space-y-1">
              {#each history as historyEntry (historyEntry.update_id)}
                {@const isSelected = selectedEntry?.update_id === historyEntry.update_id}
                <div
                  class="rounded-md cursor-pointer transition-colors {isSelected ? 'bg-accent' : 'hover:bg-muted'}"
                  role="button"
                  tabindex="0"
                  onclick={() => selectHistoryEntry(historyEntry)}
                  onkeydown={(e) => e.key === 'Enter' && selectHistoryEntry(historyEntry)}
                >
                  <div class="flex items-center justify-between p-2">
                    <div class="flex-1 min-w-0">
                      <div class="flex items-center gap-2">
                        <span class="text-sm font-medium text-foreground">
                          {formatRelativeTime(historyEntry.timestamp)}
                        </span>
                        <span class="text-[10px] px-1.5 py-0.5 rounded {getOriginClass(historyEntry.origin)}">
                          {getOriginLabel(historyEntry)}
                        </span>
                      </div>
                      <div class="text-[10px] text-muted-foreground mt-0.5">
                        #{historyEntry.update_id.toString()}
                      </div>
                    </div>
                    {#if isSelected}
                      <Button
                        variant="default"
                        size="sm"
                        class="h-6 text-xs px-2 shrink-0"
                        onclick={(e) => { e.stopPropagation(); restoreVersion(historyEntry); }}
                      >
                        <RotateCcw class="size-3 mr-1" />
                        Restore
                      </Button>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>

            <!-- Version Diff -->
            {#if selectedEntry && (diffs.length > 0 || loadingDiff)}
              <div class="mt-4 pt-3 border-t border-sidebar-border">
                <h4 class="text-xs font-medium text-muted-foreground mb-2">Changes in this version</h4>
                {#if loadingDiff}
                  <div class="flex items-center justify-center py-4">
                    <RefreshCw class="size-4 animate-spin text-muted-foreground" />
                  </div>
                {:else}
                  <VersionDiff {diffs} />
                {/if}
              </div>
            {/if}
          {/if}
        </div>
      {:else}
        <div
          class="flex flex-col items-center justify-center py-8 px-4 text-center"
        >
          <History class="size-8 text-muted-foreground mb-2" />
          <p class="text-sm text-muted-foreground">No entry selected</p>
          <p class="text-xs text-muted-foreground mt-1">
            Select an entry to view its history
          </p>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Safe area footer spacer (when no tab toggle) -->
  {#if !(historyTabId || nonHistoryPluginTabs.length > 0)}
    <div class="shrink-0 pb-[env(safe-area-inset-bottom)] md:hidden border-t border-sidebar-border bg-sidebar-accent"></div>
  {/if}

  <!-- Tab Toggle (hidden when only one tab) -->
  {#if historyTabId || nonHistoryPluginTabs.length > 0}
  <div class="shrink-0 border-t border-sidebar-border pb-[env(safe-area-inset-bottom)] bg-sidebar-accent">
    <div class="flex items-center px-3 py-2">
    <div class="flex-1 flex items-center gap-1 bg-muted rounded-md min-h-8 py-1 px-0.5 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      <button
        type="button"
        class="flex-1 shrink-0 whitespace-nowrap px-2 py-1 text-xs font-medium rounded transition-colors {activeTab === 'properties' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
        onclick={() => activeTab = "properties"}
      >
        Props
      </button>
      {#if historyTabId}
        <button
          type="button"
          class="flex-1 shrink-0 whitespace-nowrap px-2 py-1 text-xs font-medium rounded transition-colors {activeTab === historyTabId ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
          onclick={() => activeTab = historyTabId}
        >
          {historyTabLabel}
        </button>
      {/if}
      {#each nonHistoryPluginTabs as tab}
        <button
          type="button"
          class="flex-1 shrink-0 whitespace-nowrap px-2 py-1 text-xs font-medium rounded transition-colors {activeTab === tab.contribution.id ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}"
          onclick={() => activeTab = tab.contribution.id}
        >
          {tab.contribution.label}
        </button>
      {/each}
    </div>
    </div>
  </div>
  {/if}

</aside>
