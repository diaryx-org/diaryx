/**
 * Central command definition and builder for the command palette.
 *
 * Defines a `CommandDefinition` type and a `buildCommandRegistry()` factory
 * that produces a Map<string, CommandDefinition> from the current app context.
 */

import type { Component } from "svelte";
import type { Editor } from "@tiptap/core";
import {
  ImagePlus,
  Paperclip,
  Heading1,
  Heading2,
  Heading3,
  List,
  ListOrdered,
  ListTodo,
  Quote,
  Braces,
  Minus,
  Table2,
  Code,
  Copy,
  Pencil,
  Trash2,
  FolderInput,
  FilePlus,
  Search,
  LetterText,
  ClipboardCopy,
  ListOrdered as ListOrderedEditor,
  Settings,
  RefreshCw,
  ShieldCheck,
  ClipboardPaste,
  FileDown,
  Download,
  Plug,
} from "@lucide/svelte";
import { showInfo, showError } from "@/models/services/toastService";
import type { PluginInsertCommand } from "@/models/stores/pluginStore.svelte";
import type { ExportFormatInfo } from "@/controllers/exportService";

/** Format a plugin command result as a human-readable description for toasts. */
function formatPluginResult(data: unknown): string {
  if (data == null) return "";
  if (typeof data === "string") return data;
  if (typeof data !== "object") return String(data);
  const obj = data as Record<string, unknown>;

  // Sync status shape: { state, label, dirty_count, clean_count, last_sync_at, ... }
  if ("label" in obj && "dirty_count" in obj) {
    const parts: string[] = [];
    if (obj.label) parts.push(String(obj.label));
    const dirty = Number(obj.dirty_count ?? 0);
    const clean = Number(obj.clean_count ?? 0);
    if (dirty > 0) parts.push(`${dirty} modified`);
    if (clean > 0) parts.push(`${clean} synced`);
    if (obj.pending_deletes && Number(obj.pending_deletes) > 0)
      parts.push(`${obj.pending_deletes} pending deletes`);
    if (obj.last_sync_at) {
      const date = new Date(Number(obj.last_sync_at));
      parts.push(`last sync ${date.toLocaleString()}`);
    }
    return parts.join(" · ");
  }

  // Sync result shape: { pushed, pulled, errors, ... }
  if ("pushed" in obj || "pulled" in obj) {
    const parts: string[] = [];
    if (obj.pushed) parts.push(`${obj.pushed} pushed`);
    if (obj.pulled) parts.push(`${obj.pulled} pulled`);
    const errors = Array.isArray(obj.errors) ? obj.errors : [];
    if (errors.length > 0) parts.push(`${errors.length} error(s)`);
    return parts.join(" · ") || "No changes";
  }

  // Fallback: compact JSON
  try {
    const json = JSON.stringify(data, null, 0);
    return json.length > 200 ? json.slice(0, 197) + "..." : json;
  } catch {
    return String(data);
  }
}

export interface CommandDefinition {
  id: string;
  label: string;
  group: "insert" | "entry" | "editor" | "workspace" | "export";
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  icon: Component<any>;
  shortcut?: string;
  available: () => boolean;
  execute: () => void | Promise<void>;
  favoritable: boolean;
}

export interface CommandRegistryContext {
  getEditor: () => Editor | null;
  hasEntry: boolean;
  hasEditor: boolean;
  readonly: boolean;
  /** Opens a native file picker filtered to images/videos, uploads, and inserts into editor */
  onUploadMedia?: () => void;
  onDuplicateEntry?: () => void;
  onRenameEntry?: () => void;
  onDeleteEntry?: () => void;
  onMoveEntry?: () => void;
  onCreateChildEntry?: () => void;
  onFindInFile?: () => void;
  onWordCount?: () => void;
  onCopyAsMarkdown?: () => void | Promise<void>;
  onViewMarkdown?: () => void;
  onReorderFootnotes?: () => void;
  onOpenWorkspaceSettings?: () => void;
  onRefreshTree?: () => void;
  onValidateWorkspace?: () => void | Promise<void>;
  onOpenBackupImport: () => void;
  onImportFromClipboard: () => void | Promise<void>;
  onImportMarkdownFile: () => void | Promise<void>;
  onSyncNow?: () => void | Promise<void>;
  isSyncAvailable?: () => boolean;
  /** Plugin-declared command palette items from manifest CommandPaletteItem contributions. */
  pluginCommandPaletteItems: Array<{
    pluginId: string;
    contribution: {
      id: string;
      label: string;
      group: string | null;
      plugin_command: string;
    };
  }>;
  /** Dispatch a command to a plugin by ID. Returns the plugin's response. */
  dispatchPluginCommand: (
    pluginId: string,
    command: string,
    params?: unknown,
  ) => Promise<{ success: boolean; data?: unknown; error?: string }>;
  /** Export formats contributed by plugins + built-ins. */
  exportFormats: ExportFormatInfo[];
  /** Run an export for a given format. */
  onExport: (format: ExportFormatInfo) => void | Promise<void>;
  pluginBlockCommands: PluginInsertCommand[];
  pluginBlockPickerItems: Array<{
    pluginId: unknown;
    contribution: {
      id: string;
      label: string;
      icon?: string | null;
      editor_command: string;
      params?: Record<string, unknown>;
      prompt?: { message: string; default_value?: string; param_key: string } | null;
    };
  }>;
}

export function buildCommandRegistry(
  ctx: CommandRegistryContext,
): Map<string, CommandDefinition> {
  const registry = new Map<string, CommandDefinition>();

  function add(cmd: CommandDefinition) {
    registry.set(cmd.id, cmd);
  }

  // ── Insert commands ──────────────────────────────────────────────────

  add({
    id: "insert:add-photo",
    label: "Add Photo/Video",
    group: "insert",
    icon: ImagePlus,
    available: () => !!ctx.getEditor() && !!ctx.onUploadMedia,
    execute: () => { ctx.onUploadMedia?.(); },
    favoritable: true,
  });

  add({
    id: "insert:add-attachment",
    label: "Add Attachment",
    group: "insert",
    icon: Paperclip,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.commands.insertAttachmentPicker(); },
    favoritable: true,
  });

  add({
    id: "insert:heading-1",
    label: "Heading 1",
    group: "insert",
    icon: Heading1,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.chain().focus().toggleHeading({ level: 1 }).run(); },
    favoritable: true,
  });

  add({
    id: "insert:heading-2",
    label: "Heading 2",
    group: "insert",
    icon: Heading2,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.chain().focus().toggleHeading({ level: 2 }).run(); },
    favoritable: true,
  });

  add({
    id: "insert:heading-3",
    label: "Heading 3",
    group: "insert",
    icon: Heading3,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.chain().focus().toggleHeading({ level: 3 }).run(); },
    favoritable: true,
  });

  add({
    id: "insert:bullet-list",
    label: "Bullet List",
    group: "insert",
    icon: List,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.chain().focus().toggleBulletList().run(); },
    favoritable: true,
  });

  add({
    id: "insert:numbered-list",
    label: "Numbered List",
    group: "insert",
    icon: ListOrdered,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.chain().focus().toggleOrderedList().run(); },
    favoritable: true,
  });

  add({
    id: "insert:task-list",
    label: "Task List",
    group: "insert",
    icon: ListTodo,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.chain().focus().toggleTaskList().run(); },
    favoritable: true,
  });

  add({
    id: "insert:blockquote",
    label: "Blockquote",
    group: "insert",
    icon: Quote,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.chain().focus().toggleBlockquote().run(); },
    favoritable: true,
  });

  add({
    id: "insert:code-block",
    label: "Code Block",
    group: "insert",
    icon: Braces,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.chain().focus().toggleCodeBlock().run(); },
    favoritable: true,
  });

  add({
    id: "insert:horizontal-rule",
    label: "Horizontal Rule",
    group: "insert",
    icon: Minus,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.chain().focus().setHorizontalRule().run(); },
    favoritable: true,
  });

  add({
    id: "insert:table",
    label: "Table",
    group: "insert",
    icon: Table2,
    available: () => !!ctx.getEditor(),
    execute: () => {
      ctx
        .getEditor()
        ?.chain()
        .focus()
        .insertTable({ rows: 3, cols: 3, withHeaderRow: true })
        .run();
    },
    favoritable: true,
  });

  add({
    id: "insert:html-block",
    label: "HTML Block",
    group: "insert",
    icon: Code,
    available: () => !!ctx.getEditor(),
    execute: () => { ctx.getEditor()?.commands.insertHtmlBlock(); },
    favoritable: true,
  });

  // ── Plugin insert commands (block) ───────────────────────────────────

  for (const cmd of ctx.pluginBlockCommands) {
    add({
      id: `insert:plugin:${cmd.extensionId}`,
      label: cmd.label,
      group: "insert",
      icon: cmd.icon,
      available: () => !!ctx.getEditor(),
      execute: () => {
        const editor = ctx.getEditor();
        if (!editor) return;
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const commands = editor.commands as Record<string, any>;
        const commandFn = commands[`insert${cmd.extensionId.charAt(0).toUpperCase()}${cmd.extensionId.slice(1)}`];
        if (typeof commandFn === "function") commandFn();
      },
      favoritable: true,
    });
  }

  // ── Legacy block picker items ────────────────────────────────────────

  for (const item of ctx.pluginBlockPickerItems) {
    const { contribution } = item;
    add({
      id: `insert:legacy-block:${contribution.id}`,
      label: contribution.label,
      group: "insert",
      icon: Code, // fallback; real icon resolved elsewhere
      available: () => !!ctx.getEditor(),
      execute: () => {
        const editor = ctx.getEditor();
        if (!editor) return;
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const params: Record<string, any> = {
          ...(contribution.params ?? {}),
        };
        if (contribution.prompt) {
          const input = window.prompt(
            contribution.prompt.message,
            contribution.prompt.default_value,
          );
          if (!input) return;
          params[contribution.prompt.param_key] = input.trim();
        }
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const commands = editor.commands as Record<string, any>;
        const commandFn = commands[contribution.editor_command];
        if (typeof commandFn === "function") commandFn(params);
      },
      favoritable: true,
    });
  }

  // ── Entry commands ───────────────────────────────────────────────────

  if (ctx.onDuplicateEntry) {
    const fn = ctx.onDuplicateEntry;
    add({
      id: "entry:duplicate",
      label: "Duplicate Entry",
      group: "entry",
      icon: Copy,
      available: () => ctx.hasEntry,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onRenameEntry) {
    const fn = ctx.onRenameEntry;
    add({
      id: "entry:rename",
      label: "Rename Entry",
      group: "entry",
      icon: Pencil,
      available: () => ctx.hasEntry,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onDeleteEntry) {
    const fn = ctx.onDeleteEntry;
    add({
      id: "entry:delete",
      label: "Delete Entry",
      group: "entry",
      icon: Trash2,
      available: () => ctx.hasEntry,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onMoveEntry) {
    const fn = ctx.onMoveEntry;
    add({
      id: "entry:move",
      label: "Move Entry",
      group: "entry",
      icon: FolderInput,
      available: () => ctx.hasEntry,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onCreateChildEntry) {
    const fn = ctx.onCreateChildEntry;
    add({
      id: "entry:create-child",
      label: "Create Child Entry",
      group: "entry",
      icon: FilePlus,
      available: () => ctx.hasEntry,
      execute: fn,
      favoritable: true,
    });
  }

  // ── Editor commands ──────────────────────────────────────────────────

  if (ctx.onFindInFile) {
    const fn = ctx.onFindInFile;
    add({
      id: "editor:find",
      label: "Find in File",
      group: "editor",
      icon: Search,
      shortcut: "Cmd/Ctrl+F",
      available: () => ctx.hasEditor,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onWordCount) {
    const fn = ctx.onWordCount;
    add({
      id: "editor:word-count",
      label: "Word Count",
      group: "editor",
      icon: LetterText,
      available: () => ctx.hasEditor,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onCopyAsMarkdown) {
    const fn = ctx.onCopyAsMarkdown;
    add({
      id: "editor:copy-markdown",
      label: "Copy as Markdown",
      group: "editor",
      icon: ClipboardCopy,
      available: () => ctx.hasEditor,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onViewMarkdown) {
    const fn = ctx.onViewMarkdown;
    add({
      id: "editor:view-markdown",
      label: "View Markdown Source",
      group: "editor",
      icon: Code,
      available: () => ctx.hasEditor,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onReorderFootnotes) {
    const fn = ctx.onReorderFootnotes;
    add({
      id: "editor:reorder-footnotes",
      label: "Reorder Footnotes",
      group: "editor",
      icon: ListOrderedEditor,
      available: () => ctx.hasEditor,
      execute: fn,
      favoritable: true,
    });
  }

  // ── Workspace commands ───────────────────────────────────────────────

  if (ctx.onOpenWorkspaceSettings) {
    const fn = ctx.onOpenWorkspaceSettings;
    add({
      id: "workspace:settings",
      label: "Workspace Settings",
      group: "workspace",
      icon: Settings,
      available: () => true,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onRefreshTree) {
    const fn = ctx.onRefreshTree;
    add({
      id: "workspace:refresh",
      label: "Refresh Tree",
      group: "workspace",
      icon: RefreshCw,
      available: () => true,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onValidateWorkspace) {
    const fn = ctx.onValidateWorkspace;
    add({
      id: "workspace:validate",
      label: "Validate Workspace",
      group: "workspace",
      icon: ShieldCheck,
      available: () => true,
      execute: fn,
      favoritable: true,
    });
  }

  if (ctx.onSyncNow) {
    const fn = ctx.onSyncNow;
    add({
      id: "workspace:sync-now",
      label: "Sync Now",
      group: "workspace",
      icon: RefreshCw,
      available: () => ctx.isSyncAvailable?.() ?? false,
      execute: fn,
      favoritable: true,
    });
  }

  // Register plugin-declared command palette items generically.
  // Any plugin can contribute CommandPaletteItem UI entries in its manifest.
  for (const { pluginId, contribution } of ctx.pluginCommandPaletteItems) {
    const pid = pluginId as unknown as string;
    const cmd = contribution.plugin_command;
    const label = contribution.label;
    const group = (contribution.group as CommandDefinition["group"]) ?? "workspace";
    add({
      id: `plugin:${pid}:${contribution.id}`,
      label,
      group,
      icon: Plug,
      available: () => true,
      execute: async () => {
        const result = await ctx.dispatchPluginCommand(pid, cmd);
        if (!result.success) {
          showError(result.error ?? "Command failed", label);
        } else if (result.data != null) {
          showInfo(label, formatPluginResult(result.data));
        }
      },
      favoritable: true,
    });
  }

  add({
    id: "workspace:backup",
    label: "Download Backup ZIP",
    group: "workspace",
    icon: Settings,
    available: () => true,
    execute: ctx.onOpenBackupImport,
    favoritable: false,
  });

  add({
    id: "workspace:import-clipboard",
    label: "Import from Clipboard",
    group: "workspace",
    icon: ClipboardPaste,
    available: () => true,
    execute: ctx.onImportFromClipboard,
    favoritable: false,
  });

  add({
    id: "workspace:import-markdown",
    label: "Import Markdown File",
    group: "workspace",
    icon: FileDown,
    available: () => true,
    execute: ctx.onImportMarkdownFile,
    favoritable: false,
  });

  // ── Export commands ─────────────────────────────────────────────────

  for (const format of ctx.exportFormats) {
    add({
      id: `export:${format.id}`,
      label: `Export as ${format.label}`,
      group: "export",
      icon: Download,
      available: () => true,
      execute: () => ctx.onExport(format),
      favoritable: true,
    });
  }

  return registry;
}
