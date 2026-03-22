/**
 * MockBackend — in-memory Backend implementation for preview mode.
 *
 * Returns canned or starter-workspace data so the real App.svelte can render
 * without WASM, Web Workers, or any real filesystem.
 */

import type { Backend, BackendEventType, BackendEventListener, ImportResult } from "./interface";
import type { Command, Response, TreeNode, EntryData } from "./generated";

// ---------------------------------------------------------------------------
// In-memory file store
// ---------------------------------------------------------------------------

/** Parsed file: raw text split into frontmatter + body. */
interface MockFile {
  raw: string;
  frontmatter: Record<string, any>;
  body: string;
}

function parseFrontmatter(raw: string): { frontmatter: Record<string, any>; body: string } {
  const match = raw.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/);
  if (!match) return { frontmatter: {}, body: raw };
  try {
    // Simple YAML-ish parser for frontmatter (handles key: value lines)
    const fm: Record<string, any> = {};
    let currentKey: string | null = null;
    let listItems: string[] = [];

    for (const line of match[1].split("\n")) {
      const kvMatch = line.match(/^(\w[\w_-]*)\s*:\s*(.*)$/);
      if (kvMatch) {
        // Flush previous list
        if (currentKey && listItems.length > 0) {
          fm[currentKey] = listItems;
          listItems = [];
        }
        currentKey = kvMatch[1];
        const val = kvMatch[2].trim();
        if (val === "" || val === "[]") {
          fm[currentKey] = val === "[]" ? [] : null;
        } else if (val.startsWith("[") && val.endsWith("]")) {
          fm[currentKey] = val.slice(1, -1).split(",").map(s => s.trim().replace(/^["']|["']$/g, ""));
        } else {
          fm[currentKey] = val.replace(/^["']|["']$/g, "");
        }
      } else if (currentKey && line.match(/^\s*-\s+(.+)$/)) {
        const item = line.match(/^\s*-\s+(.+)$/)![1].trim().replace(/^["']|["']$/g, "");
        listItems.push(item);
        fm[currentKey] = undefined; // will be replaced by list
      }
    }
    if (currentKey && listItems.length > 0) {
      fm[currentKey] = listItems;
    }
    // Clean undefined values
    for (const k of Object.keys(fm)) {
      if (fm[k] === undefined) delete fm[k];
    }
    return { frontmatter: fm, body: match[2] };
  } catch {
    return { frontmatter: {}, body: raw };
  }
}

// ---------------------------------------------------------------------------
// Tree builder
// ---------------------------------------------------------------------------

function buildTree(files: Map<string, MockFile>, rootPath: string): TreeNode {
  function buildNode(filePath: string): TreeNode {
    const file = files.get(filePath);
    const fm = file?.frontmatter ?? {};
    const name = fm.title ?? filePath.split("/").pop()?.replace(/\.md$/, "").replace(/[-_]/g, " ") ?? filePath;
    const isIndex = Array.isArray(fm.contents);
    const children: TreeNode[] = [];

    if (isIndex && Array.isArray(fm.contents)) {
      const dir = filePath.replace(/\/[^/]+$/, "");
      for (const ref of fm.contents) {
        const refStr = String(ref);
        // Resolve relative ref to full path
        let childPath: string;
        if (refStr.endsWith("/")) {
          childPath = `${dir}/${refStr}index.md`;
        } else {
          childPath = `${dir}/${refStr}`;
        }
        if (files.has(childPath)) {
          children.push(buildNode(childPath));
        }
      }
    }

    return { name, description: fm.description ?? null, path: filePath, is_index: isIndex, children, properties: {} };
  }

  return buildNode(rootPath);
}

// ---------------------------------------------------------------------------
// Default fallback data
// ---------------------------------------------------------------------------

const FALLBACK_TREE: TreeNode = {
  name: "My Workspace",
  description: null,
  path: "workspace/index.md",
  is_index: true,
  properties: {},
  children: [
    { name: "Welcome", path: "workspace/welcome.md", is_index: false, children: [], description: null, properties: {} },
  ],
};

// ---------------------------------------------------------------------------
// MockBackend
// ---------------------------------------------------------------------------

export class MockBackend implements Backend {
  private ready = false;
  private files = new Map<string, MockFile>();
  private tree: TreeNode = FALLBACK_TREE;

  /**
   * Load starter workspace files into the mock filesystem.
   * Call this before App.svelte mounts.
   */
  loadFiles(fileMap: Map<string, string>) {
    this.files.clear();
    for (const [path, raw] of fileMap) {
      const { frontmatter, body } = parseFrontmatter(raw);
      this.files.set(path, { raw, frontmatter, body });
    }
    // Build tree from root index
    if (this.files.has("workspace/index.md")) {
      this.tree = buildTree(this.files, "workspace/index.md");
    }
  }

  private storeFile(path: string, raw: string) {
    const { frontmatter, body } = parseFrontmatter(raw);
    this.files.set(path, { raw, frontmatter, body });
  }

  private serializeFile(file: MockFile): string {
    const fmLines = Object.entries(file.frontmatter)
      .filter(([, v]) => v !== undefined && v !== null)
      .map(([k, v]) => `${k}: ${typeof v === "string" ? v : JSON.stringify(v)}`)
      .join("\n");
    return `---\n${fmLines}\n---\n${file.body}`;
  }

  private rebuildTree() {
    if (this.files.has("workspace/index.md")) {
      this.tree = buildTree(this.files, "workspace/index.md");
    }
  }

  private getEntry(path: string): EntryData {
    const file = this.files.get(path);
    if (file) {
      return {
        path,
        title: file.frontmatter.title ?? null,
        frontmatter: file.frontmatter,
        content: file.body,
      };
    }
    return {
      path,
      title: path.split("/").pop()?.replace(/\.md$/, "").replace(/-/g, " ") ?? path,
      frontmatter: {},
      content: "",
    };
  }

  /** Find the first non-index leaf entry in the tree for initial display. */
  private firstEntry(): string {
    function findLeaf(node: TreeNode): string | null {
      for (const child of node.children) {
        if (!child.is_index) return child.path;
        const deeper = findLeaf(child);
        if (deeper) return deeper;
      }
      return null;
    }
    return findLeaf(this.tree) ?? this.tree.path;
  }

  async init(): Promise<void> {
    this.ready = true;
  }

  isReady(): boolean {
    return this.ready;
  }

  getWorkspacePath(): string {
    return "workspace";
  }

  getConfig() {
    return null;
  }

  getAppPaths() {
    return null;
  }

  async execute(command: Command): Promise<Response> {
    const type = command.type;
    const params = (command as any).params;

    switch (type) {
      case "FindRootIndex":
        return { type: "String", data: "workspace/index.md" };

      case "GetWorkspaceTree":
        return { type: "Tree", data: this.tree };

      case "GetEntry":
        return { type: "Entry", data: this.getEntry(params?.path ?? this.firstEntry()) };

      case "GetFrontmatter":
        return { type: "Frontmatter", data: this.getEntry(params?.path ?? "workspace/index.md").frontmatter };

      case "GetWorkspaceConfig":
        return {
          type: "WorkspaceConfig",
          data: {
            filename_style: "title",
            link_format: null,
            default_audience: null,
            audiences: [],
            templates: [],
            disabled_plugins: [],
            show_unlinked_files: false,
            show_hidden_files: false,
            theme_mode: null,
            audience_colors: null,
          },
        } as any;

      case "ValidateWorkspace":
      case "ValidateFile":
        return {
          type: "ValidationResult",
          data: { errors: [], warnings: [], root_path: "workspace", scanned_files: 0 },
        } as any;

      case "CreateChildEntry": {
        const parentPath = params?.parent_path ?? "workspace/index.md";
        const dir = parentPath.replace(/\/[^/]+$/, "");
        const childPath = dir + "/untitled.md";
        // Create the child file
        this.storeFile(childPath, "---\ntitle: Untitled\npart_of: " + parentPath + "\n---\n");
        // Add to parent's contents
        const parent = this.files.get(parentPath);
        if (parent) {
          const contents = Array.isArray(parent.frontmatter.contents) ? [...parent.frontmatter.contents] : [];
          contents.push("untitled.md");
          parent.frontmatter.contents = contents;
        }
        this.rebuildTree();
        return {
          type: "CreateChildResult",
          data: {
            child_path: childPath,
            parent_path: parentPath,
            parent_converted: false,
            original_parent_path: null,
          },
        } as any;
      }

      case "CreateEntry": {
        const entryPath = params?.path ?? "workspace/untitled.md";
        this.storeFile(entryPath, "---\ntitle: Untitled\n---\n");
        return { type: "String", data: entryPath };
      }

      case "SetFrontmatterProperty": {
        const path = params?.path ?? "";
        const key = params?.key;
        const value = params?.value;
        const file = this.files.get(path);
        if (file && key) {
          file.frontmatter[key] = value;
          // Handle title rename
          if (key === "title" && typeof value === "string") {
            const dir = path.replace(/\/[^/]+$/, "");
            const oldFilename = path.split("/").pop()!;
            const newFilename = value.toLowerCase().replace(/\s+/g, "-") + ".md";
            const newPath = dir + "/" + newFilename;
            if (newPath !== path) {
              this.files.set(newPath, file);
              this.files.delete(path);
              // Update parent's contents reference
              for (const [, parentFile] of this.files) {
                if (Array.isArray(parentFile.frontmatter.contents)) {
                  const idx = parentFile.frontmatter.contents.indexOf(oldFilename);
                  if (idx !== -1) {
                    parentFile.frontmatter.contents[idx] = newFilename;
                  }
                }
              }
              this.rebuildTree();
              return { type: "String", data: newPath };
            }
          }
        }
        return { type: "String", data: path };
      }

      case "SaveEntry": {
        const path = params?.path ?? "";
        const content = params?.content ?? "";
        const existing = this.files.get(path);
        if (existing) {
          existing.body = content;
          existing.raw = this.serializeFile(existing);
        } else {
          this.storeFile(path, "---\n---\n" + content);
        }
        return { type: "Ok" };
      }

      case "CreateWorkspace": {
        const wsPath = params?.path ?? "workspace";
        const wsName = params?.name ?? "My Workspace";
        const indexPath = wsPath + "/index.md";
        if (this.files.has(indexPath)) {
          throw new Error("Workspace already exists");
        }
        this.storeFile(indexPath, `---\ntitle: ${wsName}\ncontents: []\n---\n`);
        this.rebuildTree();
        return { type: "Ok" };
      }

      case "WriteFile":
      case "RemoveFrontmatterProperty":
      case "ReorderFrontmatterKeys":
        return { type: "Ok" };

      case "GenerateFilename":
        return { type: "String", data: (params?.title ?? "untitled").toLowerCase().replace(/\s+/g, "-") + ".md" };

      case "ReadFile": {
        const file = this.files.get(params?.path);
        return { type: "String", data: file?.raw ?? "" };
      }

      case "FileExists":
        return { type: "Bool", data: this.files.has(params?.path) };

      case "GetPluginManifests":
        return { type: "PluginManifests", data: [] } as any;

      case "SearchWorkspace":
        return { type: "SearchResults", data: { results: [], total: 0 } } as any;

      case "GetEffectiveAudience":
        return { type: "EffectiveAudience", data: { audience: null, source: "none", ancestors: [] } } as any;

      case "GetAvailableAudiences":
      case "GetAvailableParentIndexes":
        return { type: "Strings", data: [] };

      default:
        return { type: "Ok" };
    }
  }

  on(_event: BackendEventType, _listener: BackendEventListener): void {}
  off(_event: BackendEventType, _listener: BackendEventListener): void {}

  async persist(): Promise<void> {}

  async readBinary(_path: string): Promise<Uint8Array> {
    return new Uint8Array(0);
  }

  async writeBinary(_path: string, _data: Uint8Array): Promise<void> {}

  async importFromZip(): Promise<ImportResult> {
    return { success: true, files_imported: 0 };
  }
}

export async function getMockBackend(): Promise<MockBackend> {
  const backend = new MockBackend();
  await backend.init();
  return backend;
}
