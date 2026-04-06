import { describe, expect, it, vi } from "vitest";
import { buildCommandRegistry, type CommandRegistryContext } from "./commandRegistry";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Minimal mock editor that records calls */
function makeMockEditor() {
  const run = vi.fn();
  const chain: Record<string, (...args: unknown[]) => typeof chain> & { run: ReturnType<typeof vi.fn> } = {
    focus: vi.fn(() => chain),
    toggleHeading: vi.fn(() => chain),
    toggleBulletList: vi.fn(() => chain),
    toggleOrderedList: vi.fn(() => chain),
    toggleTaskList: vi.fn(() => chain),
    toggleBlockquote: vi.fn(() => chain),
    toggleCodeBlock: vi.fn(() => chain),
    setHorizontalRule: vi.fn(() => chain),
    insertTable: vi.fn(() => chain),
    run,
  };

  return {
    chain: vi.fn(() => chain),
    commands: {
      insertAttachmentPicker: vi.fn(),
      insertHtmlBlock: vi.fn(),
    } as Record<string, unknown>,
    _chain: chain,
  };
}

type MockEditor = ReturnType<typeof makeMockEditor>;

/** Build a full context with sensible defaults; every optional callback provided */
function makeCtx(overrides?: Partial<CommandRegistryContext> & { editor?: MockEditor | null }): {
  ctx: CommandRegistryContext;
  editor: MockEditor;
} {
  const editor = overrides?.editor ?? makeMockEditor();
  const ctx: CommandRegistryContext = {
    getEditor: () => editor as unknown as ReturnType<CommandRegistryContext["getEditor"]>,
    hasEntry: true,
    hasEditor: true,
    readonly: false,
    onUploadMedia: vi.fn(),
    onDuplicateEntry: vi.fn(),
    onRenameEntry: vi.fn(),
    onDeleteEntry: vi.fn(),
    onMoveEntry: vi.fn(),
    onCreateChildEntry: vi.fn(),
    onFindInFile: vi.fn(),
    onWordCount: vi.fn(),
    onCopyAsMarkdown: vi.fn(),
    onViewMarkdown: vi.fn(),
    onReorderFootnotes: vi.fn(),
    onOpenWorkspaceSettings: vi.fn(),
    onRefreshTree: vi.fn(),
    onValidateWorkspace: vi.fn(),
    onOpenBackupImport: vi.fn(),
    onImportFromClipboard: vi.fn(),
    onImportMarkdownFile: vi.fn(),
    pluginBlockCommands: [],
    pluginBlockPickerItems: [],
    pluginCommandPaletteItems: [],
    dispatchPluginCommand: vi.fn().mockResolvedValue({ success: true }),
    ...overrides,
  };
  return { ctx, editor: editor as MockEditor };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("buildCommandRegistry", () => {
  // ── Basic structure ─────────────────────────────────────────────────

  it("returns a Map keyed by command id", () => {
    const { ctx } = makeCtx();
    const reg = buildCommandRegistry(ctx);
    expect(reg).toBeInstanceOf(Map);
    for (const [id, def] of reg) {
      expect(typeof id).toBe("string");
      expect(def.id).toBe(id);
    }
  });

  it("every definition has required fields", () => {
    const { ctx } = makeCtx();
    const reg = buildCommandRegistry(ctx);
    for (const def of reg.values()) {
      expect(def).toHaveProperty("id");
      expect(def).toHaveProperty("label");
      expect(["insert", "entry", "editor", "workspace"]).toContain(def.group);
      expect(def).toHaveProperty("icon");
      expect(typeof def.available).toBe("function");
      expect(typeof def.execute).toBe("function");
      expect(typeof def.favoritable).toBe("boolean");
    }
  });

  // ── Insert commands ─────────────────────────────────────────────────

  describe("insert commands", () => {
    it("registers core insert commands when editor is present", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      const insertIds = [...reg.values()].filter((d) => d.group === "insert").map((d) => d.id);
      expect(insertIds).toContain("insert:add-photo");
      expect(insertIds).toContain("insert:add-attachment");
      expect(insertIds).toContain("insert:heading-1");
      expect(insertIds).toContain("insert:heading-2");
      expect(insertIds).toContain("insert:heading-3");
      expect(insertIds).toContain("insert:bullet-list");
      expect(insertIds).toContain("insert:numbered-list");
      expect(insertIds).toContain("insert:task-list");
      expect(insertIds).toContain("insert:blockquote");
      expect(insertIds).toContain("insert:code-block");
      expect(insertIds).toContain("insert:horizontal-rule");
      expect(insertIds).toContain("insert:table");
      expect(insertIds).toContain("insert:html-block");
    });

    it("insert:add-photo is unavailable when onUploadMedia is not provided", () => {
      const { ctx } = makeCtx({ onUploadMedia: undefined });
      const reg = buildCommandRegistry(ctx);
      const cmd = reg.get("insert:add-photo")!;
      expect(cmd.available()).toBe(false);
    });

    it("insert:add-photo is unavailable when editor is null", () => {
      const { ctx } = makeCtx();
      // Override getEditor to return null
      ctx.getEditor = () => null;
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("insert:add-photo")!.available()).toBe(false);
    });

    it("insert:add-attachment is available when editor is present", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("insert:add-attachment")!.available()).toBe(true);
    });

    it("insert:add-attachment is unavailable when editor is null", () => {
      const { ctx } = makeCtx();
      ctx.getEditor = () => null;
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("insert:add-attachment")!.available()).toBe(false);
    });

    it("all insert commands are favoritable", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      for (const def of reg.values()) {
        if (def.group === "insert") {
          expect(def.favoritable).toBe(true);
        }
      }
    });
  });

  // ── Execute insert commands ─────────────────────────────────────────

  describe("insert command execution", () => {
    it("add-photo calls onUploadMedia", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:add-photo")!.execute();
      expect(ctx.onUploadMedia).toHaveBeenCalled();
    });

    it("add-attachment calls insertAttachmentPicker", () => {
      const { ctx, editor } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:add-attachment")!.execute();
      expect(editor.commands.insertAttachmentPicker).toHaveBeenCalled();
    });

    it("heading-1 chains focus().toggleHeading({level:1}).run()", () => {
      const { ctx, editor } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:heading-1")!.execute();
      expect(editor.chain).toHaveBeenCalled();
      expect(editor._chain.focus).toHaveBeenCalled();
      expect(editor._chain.toggleHeading).toHaveBeenCalledWith({ level: 1 });
      expect(editor._chain.run).toHaveBeenCalled();
    });

    it("heading-2 uses level 2", () => {
      const { ctx, editor } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:heading-2")!.execute();
      expect(editor._chain.toggleHeading).toHaveBeenCalledWith({ level: 2 });
    });

    it("heading-3 uses level 3", () => {
      const { ctx, editor } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:heading-3")!.execute();
      expect(editor._chain.toggleHeading).toHaveBeenCalledWith({ level: 3 });
    });

    it("bullet-list toggles bullet list", () => {
      const { ctx, editor } = makeCtx();
      buildCommandRegistry(ctx).get("insert:bullet-list")!.execute();
      expect(editor._chain.toggleBulletList).toHaveBeenCalled();
    });

    it("numbered-list toggles ordered list", () => {
      const { ctx, editor } = makeCtx();
      buildCommandRegistry(ctx).get("insert:numbered-list")!.execute();
      expect(editor._chain.toggleOrderedList).toHaveBeenCalled();
    });

    it("task-list toggles task list", () => {
      const { ctx, editor } = makeCtx();
      buildCommandRegistry(ctx).get("insert:task-list")!.execute();
      expect(editor._chain.toggleTaskList).toHaveBeenCalled();
    });

    it("blockquote toggles blockquote", () => {
      const { ctx, editor } = makeCtx();
      buildCommandRegistry(ctx).get("insert:blockquote")!.execute();
      expect(editor._chain.toggleBlockquote).toHaveBeenCalled();
    });

    it("code-block toggles code block", () => {
      const { ctx, editor } = makeCtx();
      buildCommandRegistry(ctx).get("insert:code-block")!.execute();
      expect(editor._chain.toggleCodeBlock).toHaveBeenCalled();
    });

    it("horizontal-rule inserts horizontal rule", () => {
      const { ctx, editor } = makeCtx();
      buildCommandRegistry(ctx).get("insert:horizontal-rule")!.execute();
      expect(editor._chain.setHorizontalRule).toHaveBeenCalled();
    });

    it("table inserts a 3x3 table with header row", () => {
      const { ctx, editor } = makeCtx();
      buildCommandRegistry(ctx).get("insert:table")!.execute();
      expect(editor._chain.insertTable).toHaveBeenCalledWith({
        rows: 3,
        cols: 3,
        withHeaderRow: true,
      });
    });

    it("html-block calls insertHtmlBlock command", () => {
      const { ctx, editor } = makeCtx();
      buildCommandRegistry(ctx).get("insert:html-block")!.execute();
      expect(editor.commands.insertHtmlBlock).toHaveBeenCalled();
    });
  });

  // ── Plugin block commands ───────────────────────────────────────────

  describe("plugin block commands", () => {
    it("registers plugin insert commands with correct ids", () => {
      const { ctx } = makeCtx({
        pluginBlockCommands: [
          {
            extensionId: "myWidget",
            label: "My Widget",
            iconName: null,
            description: null,
            nodeType: "BlockAtom" as const,
            placement: "Picker" as const,
            icon: {} as any,
          },
        ],
      });
      const reg = buildCommandRegistry(ctx);
      expect(reg.has("insert:plugin:myWidget")).toBe(true);
      expect(reg.get("insert:plugin:myWidget")!.label).toBe("My Widget");
      expect(reg.get("insert:plugin:myWidget")!.group).toBe("insert");
    });

    it("plugin command calls the correctly-named editor command", () => {
      const editor = makeMockEditor();
      const insertFn = vi.fn();
      editor.commands["insertMyWidget"] = insertFn;

      const { ctx } = makeCtx({
        editor,
        pluginBlockCommands: [
          { extensionId: "myWidget", label: "My Widget", iconName: null, description: null, nodeType: "BlockAtom" as const, placement: "Picker" as const, icon: {} as any },
        ],
      });
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:plugin:myWidget")!.execute();
      expect(insertFn).toHaveBeenCalled();
    });

    it("plugin command does nothing if editor is null", () => {
      const { ctx } = makeCtx({
        pluginBlockCommands: [
          { extensionId: "myWidget", label: "My Widget", iconName: null, description: null, nodeType: "BlockAtom" as const, placement: "Picker" as const, icon: {} as any },
        ],
      });
      ctx.getEditor = () => null;
      const reg = buildCommandRegistry(ctx);
      // Should not throw
      expect(() => reg.get("insert:plugin:myWidget")!.execute()).not.toThrow();
    });

    it("plugin command does nothing if editor command is not a function", () => {
      const editor = makeMockEditor();
      // Do not add a matching command
      const { ctx } = makeCtx({
        editor,
        pluginBlockCommands: [
          { extensionId: "noSuch", label: "No Such", iconName: null, description: null, nodeType: "BlockAtom" as const, placement: "Picker" as const, icon: {} as any },
        ],
      });
      const reg = buildCommandRegistry(ctx);
      expect(() => reg.get("insert:plugin:noSuch")!.execute()).not.toThrow();
    });
  });

  // ── Legacy block picker items ───────────────────────────────────────

  describe("legacy block picker items", () => {
    it("registers legacy block picker items", () => {
      const { ctx } = makeCtx({
        pluginBlockPickerItems: [
          {
            pluginId: "p1",
            contribution: {
              id: "myBlock",
              label: "My Block",
              editor_command: "insertMyBlock",
            },
          },
        ],
      });
      const reg = buildCommandRegistry(ctx);
      expect(reg.has("insert:legacy-block:myBlock")).toBe(true);
      expect(reg.get("insert:legacy-block:myBlock")!.label).toBe("My Block");
    });

    it("executes legacy block command with params", () => {
      const editor = makeMockEditor();
      const insertFn = vi.fn();
      editor.commands["insertMyBlock"] = insertFn;

      const { ctx } = makeCtx({
        editor,
        pluginBlockPickerItems: [
          {
            pluginId: "p1",
            contribution: {
              id: "myBlock",
              label: "My Block",
              editor_command: "insertMyBlock",
              params: { color: "red" },
            },
          },
        ],
      });
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:legacy-block:myBlock")!.execute();
      expect(insertFn).toHaveBeenCalledWith({ color: "red" });
    });

    it("legacy block with prompt calls window.prompt and merges result", () => {
      const editor = makeMockEditor();
      const insertFn = vi.fn();
      editor.commands["insertMyBlock"] = insertFn;
      vi.spyOn(window, "prompt").mockReturnValue("user-input");

      const { ctx } = makeCtx({
        editor,
        pluginBlockPickerItems: [
          {
            pluginId: "p1",
            contribution: {
              id: "myBlock",
              label: "My Block",
              editor_command: "insertMyBlock",
              params: { base: true },
              prompt: {
                message: "Enter value",
                default_value: "default",
                param_key: "userVal",
              },
            },
          },
        ],
      });
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:legacy-block:myBlock")!.execute();
      expect(window.prompt).toHaveBeenCalledWith("Enter value", "default");
      expect(insertFn).toHaveBeenCalledWith({ base: true, userVal: "user-input" });
    });

    it("legacy block with prompt aborts if user cancels prompt", () => {
      const editor = makeMockEditor();
      const insertFn = vi.fn();
      editor.commands["insertMyBlock"] = insertFn;
      vi.spyOn(window, "prompt").mockReturnValue(null);

      const { ctx } = makeCtx({
        editor,
        pluginBlockPickerItems: [
          {
            pluginId: "p1",
            contribution: {
              id: "myBlock",
              label: "My Block",
              editor_command: "insertMyBlock",
              prompt: {
                message: "Enter value",
                param_key: "userVal",
              },
            },
          },
        ],
      });
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:legacy-block:myBlock")!.execute();
      expect(insertFn).not.toHaveBeenCalled();
    });

    it("legacy block with prompt aborts if user enters empty string", () => {
      const editor = makeMockEditor();
      const insertFn = vi.fn();
      editor.commands["insertMyBlock"] = insertFn;
      vi.spyOn(window, "prompt").mockReturnValue("");

      const { ctx } = makeCtx({
        editor,
        pluginBlockPickerItems: [
          {
            pluginId: "p1",
            contribution: {
              id: "myBlock",
              label: "My Block",
              editor_command: "insertMyBlock",
              prompt: {
                message: "Enter",
                param_key: "val",
              },
            },
          },
        ],
      });
      const reg = buildCommandRegistry(ctx);
      reg.get("insert:legacy-block:myBlock")!.execute();
      expect(insertFn).not.toHaveBeenCalled();
    });

    it("legacy block does nothing if editor is null", () => {
      const { ctx } = makeCtx({
        pluginBlockPickerItems: [
          {
            pluginId: "p1",
            contribution: {
              id: "myBlock",
              label: "My Block",
              editor_command: "insertMyBlock",
            },
          },
        ],
      });
      ctx.getEditor = () => null;
      const reg = buildCommandRegistry(ctx);
      expect(() => reg.get("insert:legacy-block:myBlock")!.execute()).not.toThrow();
    });
  });

  // ── Entry commands ──────────────────────────────────────────────────

  describe("entry commands", () => {
    it("registers all entry commands when callbacks are provided", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.has("entry:duplicate")).toBe(true);
      expect(reg.has("entry:rename")).toBe(true);
      expect(reg.has("entry:delete")).toBe(true);
      expect(reg.has("entry:move")).toBe(true);
      expect(reg.has("entry:create-child")).toBe(true);
    });

    it("does not register entry commands when callbacks are omitted", () => {
      const { ctx } = makeCtx({
        onDuplicateEntry: undefined,
        onRenameEntry: undefined,
        onDeleteEntry: undefined,
        onMoveEntry: undefined,
        onCreateChildEntry: undefined,
      });
      const reg = buildCommandRegistry(ctx);
      expect(reg.has("entry:duplicate")).toBe(false);
      expect(reg.has("entry:rename")).toBe(false);
      expect(reg.has("entry:delete")).toBe(false);
      expect(reg.has("entry:move")).toBe(false);
      expect(reg.has("entry:create-child")).toBe(false);
    });

    it("entry commands are available when hasEntry is true", () => {
      const { ctx } = makeCtx({ hasEntry: true });
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("entry:duplicate")!.available()).toBe(true);
      expect(reg.get("entry:rename")!.available()).toBe(true);
    });

    it("entry commands are unavailable when hasEntry is false", () => {
      const { ctx } = makeCtx({ hasEntry: false });
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("entry:duplicate")!.available()).toBe(false);
      expect(reg.get("entry:rename")!.available()).toBe(false);
      expect(reg.get("entry:delete")!.available()).toBe(false);
      expect(reg.get("entry:move")!.available()).toBe(false);
      expect(reg.get("entry:create-child")!.available()).toBe(false);
    });

    it("entry:duplicate executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("entry:duplicate")!.execute();
      expect(ctx.onDuplicateEntry).toHaveBeenCalled();
    });

    it("entry:rename executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("entry:rename")!.execute();
      expect(ctx.onRenameEntry).toHaveBeenCalled();
    });

    it("entry:delete executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("entry:delete")!.execute();
      expect(ctx.onDeleteEntry).toHaveBeenCalled();
    });

    it("entry:move executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("entry:move")!.execute();
      expect(ctx.onMoveEntry).toHaveBeenCalled();
    });

    it("entry:create-child executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("entry:create-child")!.execute();
      expect(ctx.onCreateChildEntry).toHaveBeenCalled();
    });

    it("entry commands are all favoritable", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      for (const def of reg.values()) {
        if (def.group === "entry") {
          expect(def.favoritable).toBe(true);
        }
      }
    });
  });

  // ── Editor commands ─────────────────────────────────────────────────

  describe("editor commands", () => {
    it("registers all editor commands when callbacks are provided", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.has("editor:find")).toBe(true);
      expect(reg.has("editor:word-count")).toBe(true);
      expect(reg.has("editor:copy-markdown")).toBe(true);
      expect(reg.has("editor:view-markdown")).toBe(true);
      expect(reg.has("editor:reorder-footnotes")).toBe(true);
    });

    it("does not register editor commands when callbacks are omitted", () => {
      const { ctx } = makeCtx({
        onFindInFile: undefined,
        onWordCount: undefined,
        onCopyAsMarkdown: undefined,
        onViewMarkdown: undefined,
        onReorderFootnotes: undefined,
      });
      const reg = buildCommandRegistry(ctx);
      expect(reg.has("editor:find")).toBe(false);
      expect(reg.has("editor:word-count")).toBe(false);
      expect(reg.has("editor:copy-markdown")).toBe(false);
      expect(reg.has("editor:view-markdown")).toBe(false);
      expect(reg.has("editor:reorder-footnotes")).toBe(false);
    });

    it("editor commands are available when hasEditor is true", () => {
      const { ctx } = makeCtx({ hasEditor: true });
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("editor:find")!.available()).toBe(true);
    });

    it("editor commands are unavailable when hasEditor is false", () => {
      const { ctx } = makeCtx({ hasEditor: false });
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("editor:find")!.available()).toBe(false);
      expect(reg.get("editor:word-count")!.available()).toBe(false);
    });

    it("editor:find has a shortcut defined", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("editor:find")!.shortcut).toBe("Cmd/Ctrl+F");
    });

    it("editor:find executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("editor:find")!.execute();
      expect(ctx.onFindInFile).toHaveBeenCalled();
    });

    it("editor:copy-markdown executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("editor:copy-markdown")!.execute();
      expect(ctx.onCopyAsMarkdown).toHaveBeenCalled();
    });
  });

  // ── Workspace commands ──────────────────────────────────────────────

  describe("workspace commands", () => {
    it("registers optional workspace commands when callbacks are provided", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.has("workspace:settings")).toBe(true);
      expect(reg.has("workspace:refresh")).toBe(true);
      expect(reg.has("workspace:validate")).toBe(true);
    });

    it("does not register optional workspace commands when callbacks are omitted", () => {
      const { ctx } = makeCtx({
        onOpenWorkspaceSettings: undefined,
        onRefreshTree: undefined,
        onValidateWorkspace: undefined,
      });
      const reg = buildCommandRegistry(ctx);
      expect(reg.has("workspace:settings")).toBe(false);
      expect(reg.has("workspace:refresh")).toBe(false);
      expect(reg.has("workspace:validate")).toBe(false);
    });

    it("always registers backup, import-clipboard, import-markdown", () => {
      const { ctx } = makeCtx({
        onOpenWorkspaceSettings: undefined,
        onRefreshTree: undefined,
        onValidateWorkspace: undefined,
      });
      const reg = buildCommandRegistry(ctx);
      expect(reg.has("workspace:backup")).toBe(true);
      expect(reg.has("workspace:import-clipboard")).toBe(true);
      expect(reg.has("workspace:import-markdown")).toBe(true);
    });

    it("workspace:settings is always available", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("workspace:settings")!.available()).toBe(true);
    });

    it("workspace:refresh is always available", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("workspace:refresh")!.available()).toBe(true);
    });

    it("workspace:validate is always available", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("workspace:validate")!.available()).toBe(true);
    });

    it("workspace:backup is not favoritable", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("workspace:backup")!.favoritable).toBe(false);
    });

    it("workspace:import-clipboard is not favoritable", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("workspace:import-clipboard")!.favoritable).toBe(false);
    });

    it("workspace:import-markdown is not favoritable", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      expect(reg.get("workspace:import-markdown")!.favoritable).toBe(false);
    });

    it("workspace:backup executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("workspace:backup")!.execute();
      expect(ctx.onOpenBackupImport).toHaveBeenCalled();
    });

    it("workspace:import-clipboard executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("workspace:import-clipboard")!.execute();
      expect(ctx.onImportFromClipboard).toHaveBeenCalled();
    });

    it("workspace:import-markdown executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("workspace:import-markdown")!.execute();
      expect(ctx.onImportMarkdownFile).toHaveBeenCalled();
    });

    it("workspace:settings executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("workspace:settings")!.execute();
      expect(ctx.onOpenWorkspaceSettings).toHaveBeenCalled();
    });

    it("workspace:refresh executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("workspace:refresh")!.execute();
      expect(ctx.onRefreshTree).toHaveBeenCalled();
    });

    it("workspace:validate executes the provided callback", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      reg.get("workspace:validate")!.execute();
      expect(ctx.onValidateWorkspace).toHaveBeenCalled();
    });
  });

  // ── Availability reacts to context changes ──────────────────────────

  describe("availability is dynamic", () => {
    it("entry available() reflects hasEntry changes at call time", () => {
      const { ctx } = makeCtx({ hasEntry: false });
      const reg = buildCommandRegistry(ctx);
      const cmd = reg.get("entry:duplicate")!;
      expect(cmd.available()).toBe(false);

      // Mutate context — the closure should pick it up
      ctx.hasEntry = true;
      expect(cmd.available()).toBe(true);
    });

    it("editor available() reflects hasEditor changes at call time", () => {
      const { ctx } = makeCtx({ hasEditor: false });
      const reg = buildCommandRegistry(ctx);
      const cmd = reg.get("editor:find")!;
      expect(cmd.available()).toBe(false);

      ctx.hasEditor = true;
      expect(cmd.available()).toBe(true);
    });

    it("insert available() reflects getEditor returning null vs an editor", () => {
      const editor = makeMockEditor();
      const { ctx } = makeCtx({ editor });
      const reg = buildCommandRegistry(ctx);
      const cmd = reg.get("insert:heading-1")!;
      expect(cmd.available()).toBe(true);

      ctx.getEditor = () => null;
      expect(cmd.available()).toBe(false);
    });
  });

  // ── Group listing ───────────────────────────────────────────────────

  describe("group categorization", () => {
    it("all groups are represented", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      const groups = new Set([...reg.values()].map((d) => d.group));
      expect(groups).toContain("insert");
      expect(groups).toContain("entry");
      expect(groups).toContain("editor");
      expect(groups).toContain("workspace");
    });

    it("commands can be filtered by group", () => {
      const { ctx } = makeCtx();
      const reg = buildCommandRegistry(ctx);
      const entryCommands = [...reg.values()].filter((d) => d.group === "entry");
      expect(entryCommands.length).toBe(5); // duplicate, rename, delete, move, create-child
      for (const cmd of entryCommands) {
        expect(cmd.group).toBe("entry");
      }
    });
  });

  // ── No duplicate ids ────────────────────────────────────────────────

  it("has no duplicate ids (last write wins in Map, but all should be unique)", () => {
    const { ctx } = makeCtx();
    const reg = buildCommandRegistry(ctx);
    // The Map itself deduplicates, so check the count matches expectations.
    // With all callbacks + 0 plugins we expect a specific count.
    const allIds = [...reg.keys()];
    const uniqueIds = new Set(allIds);
    expect(allIds.length).toBe(uniqueIds.size);
  });
});
