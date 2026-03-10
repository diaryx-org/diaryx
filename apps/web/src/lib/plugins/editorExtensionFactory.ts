/**
 * Editor Extension Factory — generates TipTap extensions from plugin manifest declarations.
 *
 * When a plugin declares `EditorExtension` entries in its manifest, this factory
 * creates corresponding TipTap extensions:
 * - **Atom nodes** (`InlineAtom`, `BlockAtom`): TipTap `Node` with vanilla DOM node views
 *   and async rendering via the plugin's exported render function.
 * - **Inline marks** (`InlineMark`): TipTap `Mark` that wraps rich text with
 *   input/paste rules, keyboard shortcuts, and optional click behavior.
 */

import { Mark, Node, mergeAttributes } from "@tiptap/core";
import { markInputRule, markPasteRule } from "@tiptap/core";
import { Plugin as ProseMirrorPlugin, PluginKey } from "@tiptap/pm/state";
import { TemplateVariable } from "$lib/extensions/TemplateVariable";
import { ConditionalBlock } from "$lib/extensions/ConditionalBlock";

// ============================================================================
// Builtin extension registry — complex extensions registered by host ID
// ============================================================================

/**
 * Registry of host-provided TipTap extensions that are too complex for the
 * declarative manifest. Plugins declare `node_type: Builtin { host_extension_id }`
 * and the factory looks up the pre-registered TypeScript extension(s) here.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const BUILTIN_EXTENSION_REGISTRY: Record<string, () => any[]> = {
  templateVariable: () => [TemplateVariable],
  conditionalBlock: () => [ConditionalBlock.configure({ enabled: true })],
};

/**
 * Look up a builtin extension by host_extension_id.
 * Returns the TipTap extension instances, or null if not found.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function getBuiltinExtension(hostExtensionId: string): any[] | null {
  const factory = BUILTIN_EXTENSION_REGISTRY[hostExtensionId];
  return factory ? factory() : null;
}

// ============================================================================
// Types matching the Rust EditorExtension manifest
// ============================================================================

export interface EditorExtensionManifest {
  slot: "EditorExtension";
  extension_id: string;
  node_type:
    | "InlineAtom"
    | "BlockAtom"
    | "InlineMark"
    | { Builtin: { host_extension_id: string } };
  markdown: {
    level: "Inline" | "Block";
    open: string;
    close: string;
  };
  render_export: string | null;
  edit_mode: "Popover" | "SourceToggle" | null;
  css: string | null;
  insert_command?: {
    label: string;
    icon?: string | null;
    description?: string | null;
  } | null;
  keyboard_shortcut?: string | null;
  click_behavior?: {
    ToggleClass: {
      hidden_class: string;
      revealed_class: string;
    };
  } | null;
}

export interface EditorExtensionFactoryOptions {
  preserveOnly?: boolean;
  pluginName?: string;
}

/** Check if a UiContribution is an EditorExtension. */
export function isEditorExtension(ui: unknown): ui is EditorExtensionManifest {
  return (
    typeof ui === "object" &&
    ui !== null &&
    (ui as Record<string, unknown>).slot === "EditorExtension"
  );
}

// ============================================================================
// Extension factory
// ============================================================================

/** Generic render function signature accepted by the extension factory. */
export type RenderFn = (
  source: string,
  displayMode: boolean,
) => Promise<{ html?: string; error?: string }>;

/**
 * Create a TipTap Node extension from a manifest declaration and a render function.
 *
 * The generated extension:
 * 1. Defines a Node with `{ source: string }` attribute
 * 2. Generates markdown tokenizer from open/close delimiters
 * 3. Mounts a Svelte node view that calls the provided render function
 * 4. Supports source editing (popover for inline, source toggle for block)
 */
export function createExtensionFromManifest(
  ext: EditorExtensionManifest,
  renderFn: RenderFn,
  options: EditorExtensionFactoryOptions = {},
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
): any {
  const isInline = ext.node_type === "InlineAtom";
  const isBlock = ext.node_type === "BlockAtom";
  const preserveOnly = options.preserveOnly === true;
  const missingPluginLabel = options.pluginName ?? ext.insert_command?.label ?? ext.extension_id;

  // Escape delimiters for use in regex
  const openEsc = escapeRegex(ext.markdown.open);
  const closeEsc = escapeRegex(ext.markdown.close);

  // Inject CSS if declared
  if (ext.css && !preserveOnly) {
    injectCss(ext.extension_id, ext.css);
  }

  injectAtomNodeViewCss();

  return Node.create({
    name: ext.extension_id,

    group: isInline ? "inline" : "block",

    inline: isInline,

    atom: true,

    draggable: isBlock,

    selectable: true,

    addAttributes() {
      return {
        source: { default: "" },
      };
    },

    parseHTML() {
      return [{ tag: `span[data-${ext.extension_id}]` }];
    },

    renderHTML({ HTMLAttributes }) {
      const tag = isInline ? "span" : "div";
      return [
        tag,
        {
          [`data-${ext.extension_id}`]: "",
          class: isInline ? "atom-inline" : "atom-block",
        },
        HTMLAttributes.source || "",
      ];
    },

    addNodeView() {
      if (preserveOnly) {
        return createMissingAtomView(ext, missingPluginLabel);
      }
      if (isInline) {
        return createInlineAtomView(ext, renderFn);
      }
      return createBlockAtomView(ext, renderFn);
    },

    // Markdown tokenizer — generated from open/close delimiters
    // @ts-ignore - custom field for @tiptap/markdown
    markdownTokenizer: {
      name: ext.extension_id,
      level: ext.markdown.level === "Inline" ? "inline" : "block",
      start: ext.markdown.open,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      tokenize(src: string, _tokens: any[]) {
        if (isBlock) {
          // Block: match $$\n...\n$$ or $$...$$ (with optional newlines)
          const re = new RegExp(
            `^${openEsc}\\n?([\\s\\S]+?)\\n?${closeEsc}(?:\\n|$)`,
          );
          const match = re.exec(src);
          if (!match) return undefined;
          return {
            type: ext.extension_id,
            raw: match[0],
            source: match[1].trim(),
          };
        } else {
          // Inline: match $...$ but NOT $$
          // Must not be preceded by another $ (to avoid matching $$ blocks)
          const re = new RegExp(`^${openEsc}(?!${openEsc})(.+?)${closeEsc}`);
          const match = re.exec(src);
          if (!match) return undefined;
          return {
            type: ext.extension_id,
            raw: match[0],
            source: match[1],
          };
        }
      },
    },

    // Parse token → node
    // @ts-ignore - custom field for @tiptap/markdown
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    parseMarkdown(token: any, helpers: any) {
      return helpers.createNode(ext.extension_id, {
        source: token.source || "",
      });
    },
  }).extend({
    // Render node → markdown
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    renderMarkdown(node: any) {
      const source = node.attrs?.source ?? "";
      if (isBlock) {
        return `${ext.markdown.open}\n${source}\n${ext.markdown.close}\n`;
      }
      return `${ext.markdown.open}${source}${ext.markdown.close}`;
    },
  });
}

// ============================================================================
// Mark factory (InlineMark)
// ============================================================================

/**
 * Create a TipTap Mark extension from a manifest declaration.
 *
 * The generated mark:
 * 1. Wraps inline content (supports rich text children)
 * 2. Generates input/paste rules from open/close delimiters
 * 3. Adds keyboard shortcut and click behavior from manifest
 * 4. Generates markdown tokenizer/parser/renderer
 */
export function createMarkFromManifest(
  ext: EditorExtensionManifest,
  options: EditorExtensionFactoryOptions = {},
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
): any {
  const preserveOnly = options.preserveOnly === true;
  const openEsc = escapeRegex(ext.markdown.open);
  const closeEsc = escapeRegex(ext.markdown.close);

  // Input rule: matches ||text|| at end of input
  const inputRegex = new RegExp(`${openEsc}([^${escapeRegex(ext.markdown.open[0])}]+)${closeEsc}$`);
  // Paste rule: matches ||text|| globally
  const pasteRegex = new RegExp(`${openEsc}([^${escapeRegex(ext.markdown.open[0])}]+)${closeEsc}`, "g");

  // Extract click behavior classes
  const clickBehavior = ext.click_behavior?.ToggleClass ?? null;
  const hiddenClass = clickBehavior?.hidden_class ?? `${ext.extension_id}-hidden`;
  const revealedClass = clickBehavior?.revealed_class ?? `${ext.extension_id}-revealed`;

  // Inject CSS if declared
  if (ext.css && !preserveOnly) {
    injectCss(ext.extension_id, ext.css);
  }
  if (preserveOnly) {
    injectMissingPluginCss();
  }

  // PascalCase name for commands (e.g., "spoiler" → "Spoiler")
  const pascalName = ext.extension_id.charAt(0).toUpperCase() + ext.extension_id.slice(1);

  return Mark.create({
    name: ext.extension_id,

    parseHTML() {
      return [{ tag: `span[data-${ext.extension_id}]` }];
    },

    renderHTML({ HTMLAttributes }) {
      return [
        "span",
        mergeAttributes(HTMLAttributes, {
          [`data-${ext.extension_id}`]: "",
          class: preserveOnly
            ? "plugin-missing-mark"
            : `${ext.extension_id}-mark ${hiddenClass}`,
        }),
        0,
      ];
    },

    addCommands() {
      if (preserveOnly) {
        return {};
      }
      return {
        [`set${pascalName}`]:
          () =>
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          ({ commands }: any) => {
            return commands.setMark(ext.extension_id);
          },
        [`toggle${pascalName}`]:
          () =>
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          ({ commands }: any) => {
            return commands.toggleMark(ext.extension_id);
          },
        [`unset${pascalName}`]:
          () =>
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          ({ commands }: any) => {
            return commands.unsetMark(ext.extension_id);
          },
      };
    },

    addKeyboardShortcuts() {
      if (preserveOnly || !ext.keyboard_shortcut) return {};
      return {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        [ext.keyboard_shortcut]: () => (this as any).editor.commands[`toggle${pascalName}`](),
      };
    },

    addInputRules() {
      if (preserveOnly) return [];
      return [
        markInputRule({
          find: inputRegex,
          type: this.type,
        }),
      ];
    },

    addPasteRules() {
      if (preserveOnly) return [];
      return [
        markPasteRule({
          find: pasteRegex,
          type: this.type,
        }),
      ];
    },

    addProseMirrorPlugins() {
      if (preserveOnly || !clickBehavior) return [];

      const dataAttr = `data-${ext.extension_id}`;

      return [
        new ProseMirrorPlugin({
          key: new PluginKey(`${ext.extension_id}Click`),
          props: {
            handleClick: (view, _pos, event) => {
              const target = event.target as HTMLElement;
              const markEl = target.closest(`[${dataAttr}]`) as HTMLElement | null;

              if (markEl) {
                // Toggle reveal state
                if (markEl.classList.contains(hiddenClass)) {
                  markEl.classList.remove(hiddenClass);
                  markEl.classList.add(revealedClass);
                } else {
                  markEl.classList.remove(revealedClass);
                  markEl.classList.add(hiddenClass);
                }
                return true;
              }

              // Click elsewhere: hide all revealed marks of this type
              const editorDom = view.dom;
              const revealed = editorDom.querySelectorAll(`.${revealedClass}`);
              revealed.forEach((el) => {
                el.classList.remove(revealedClass);
                el.classList.add(hiddenClass);
              });

              return false;
            },
          },
        }),
      ];
    },

    // Custom tokenizer for parsing delimited text from markdown
    // @ts-ignore - markdownTokenizer is a custom field for @tiptap/markdown
    markdownTokenizer: {
      name: ext.extension_id,
      level: "inline",
      start: ext.markdown.open,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      tokenize(src: string, _tokens: any[], helper: any) {
        const re = new RegExp(`^${openEsc}([^${escapeRegex(ext.markdown.open[0])}]+)${closeEsc}`);
        const match = re.exec(src);
        if (!match) return undefined;
        return {
          type: ext.extension_id,
          raw: match[0],
          tokens: helper.inlineTokens(match[1]),
        };
      },
    },

    // Parse the token into a mark result
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    parseMarkdown(token: any, helpers: any) {
      const content = token.tokens ? helpers.parseInline(token.tokens) : [];
      return helpers.applyMark(ext.extension_id, content);
    },
  }).extend({
    // Render mark → markdown
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    renderMarkdown(node: any, helpers: any) {
      const content = helpers.renderChildren(node);
      return `${ext.markdown.open}${content}${ext.markdown.close}`;
    },
  });
}

// ============================================================================
// Helpers
// ============================================================================

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

const injectedCss = new Set<string>();
let missingPluginCssInjected = false;

function injectCss(id: string, css: string) {
  if (injectedCss.has(id)) return;
  injectedCss.add(id);
  const style = document.createElement("style");
  style.setAttribute("data-plugin-css", id);
  style.textContent = css;
  document.head.appendChild(style);
}

function injectMissingPluginCss() {
  if (missingPluginCssInjected) return;
  missingPluginCssInjected = true;
  const style = document.createElement("style");
  style.setAttribute("data-plugin-missing-css", "");
  style.textContent = `
    .plugin-missing-mark {
      background: color-mix(in srgb, var(--muted) 75%, transparent);
      border: 1px dashed var(--border);
      border-radius: 3px;
      padding: 0 2px;
    }
  `;
  document.head.appendChild(style);
}

// ============================================================================
// Atom node view helpers (vanilla DOM — no Svelte dependency)
// ============================================================================

let atomCssInjected = false;

function injectAtomNodeViewCss() {
  if (atomCssInjected) return;
  atomCssInjected = true;
  const style = document.createElement("style");
  style.setAttribute("data-atom-node-views", "");
  style.textContent = `
    .atom-inline-rendered { cursor: pointer; border-radius: 3px; padding: 0 2px; transition: background 0.15s; }
    .atom-inline-rendered:hover { background: var(--accent); }
    .atom-inline-error { font-size: 0.9em; color: var(--destructive); border-bottom: 1px wavy var(--destructive); }
    .atom-inline-loading, .atom-inline-empty { color: var(--muted-foreground); font-style: italic; }
    .atom-inline-editing { display: inline-flex; align-items: center; gap: 1px; background: var(--muted); border-radius: 3px; padding: 0 2px; }
    .atom-inline-delim { color: var(--muted-foreground); font-family: "SF Mono", Monaco, "Cascadia Code", monospace; font-size: 0.85em; }
    .atom-inline-input { border: none; background: transparent; color: var(--foreground); font-family: "SF Mono", Monaco, "Cascadia Code", monospace; font-size: 0.85em; outline: none; min-width: 3em; width: auto; padding: 0; }
    .atom-block-container { border: 1px dashed var(--border); border-radius: 6px; margin: 0.5em 0; overflow: hidden; user-select: none; -webkit-user-select: none; }
    .atom-block-header { display: flex; justify-content: space-between; align-items: center; padding: 4px 8px; background: var(--muted); border-bottom: 1px solid var(--border); }
    .atom-block-label { font-size: 11px; font-weight: 600; color: var(--muted-foreground); text-transform: uppercase; letter-spacing: 0.05em; }
    .atom-block-toggle { display: flex; align-items: center; justify-content: center; padding: 2px 6px; border: none; background: transparent; border-radius: 3px; cursor: pointer; color: var(--muted-foreground); font-size: 11px; font-family: "SF Mono", Monaco, "Cascadia Code", monospace; }
    .atom-block-toggle:hover { background: var(--accent); color: var(--accent-foreground); }
    .atom-block-preview { padding: 12px; text-align: center; overflow-x: auto; }
    .atom-block-empty, .atom-block-loading { color: var(--muted-foreground); font-style: italic; font-size: 13px; }
    .atom-block-error { display: flex; flex-direction: column; gap: 4px; align-items: center; }
    .atom-block-error code { font-size: 13px; color: var(--foreground); }
    .atom-block-error-msg { font-size: 11px; color: var(--destructive); }
    .atom-inline-missing {
      display: inline-block;
      color: var(--muted-foreground);
      border: 1px dashed var(--border);
      border-radius: 3px;
      padding: 0 4px;
      font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
      font-size: 0.85em;
    }
    .atom-block-missing { border-style: solid; }
    .atom-block-missing-label { color: var(--destructive); }
    .atom-block-missing-body {
      padding: 12px;
      white-space: pre-wrap;
      font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
      font-size: 13px;
      color: var(--muted-foreground);
      background: var(--card);
    }
    .atom-block-source { width: 100%; min-height: 60px; padding: 12px; border: none; background: var(--card); color: var(--foreground); font-family: "SF Mono", Monaco, "Cascadia Code", monospace; font-size: 13px; line-height: 1.5; resize: vertical; outline: none; box-sizing: border-box; field-sizing: content; }
  `;
  document.head.appendChild(style);
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function createMissingAtomView(ext: EditorExtensionManifest, pluginName: string): any {
  const isInline = ext.node_type === "InlineAtom";

  return ({ node }: { node: any }) => {
    if (isInline) {
      const dom = document.createElement("span");
      dom.className = "atom-inline-missing";
      dom.setAttribute("contenteditable", "false");

      const renderSource = (source: string) => {
        dom.textContent = `${ext.markdown.open}${source}${ext.markdown.close}`;
        dom.title = `${pluginName} plugin removed — raw markdown preserved`;
      };

      renderSource((node.attrs.source as string) || "");

      return {
        dom,
        update(updatedNode: any) {
          if (updatedNode.type.name !== ext.extension_id) return false;
          renderSource((updatedNode.attrs.source as string) || "");
          return true;
        },
        destroy() {},
      };
    }

    const dom = document.createElement("div");
    dom.className = "atom-block-container atom-block-missing";
    dom.setAttribute("contenteditable", "false");

    const header = document.createElement("div");
    header.className = "atom-block-header";

    const label = document.createElement("span");
    label.className = "atom-block-label atom-block-missing-label";
    label.textContent = `${pluginName} removed`;

    header.append(label);

    const content = document.createElement("div");
    content.className = "atom-block-missing-body";

    const renderSource = (source: string) => {
      content.textContent = `${ext.markdown.open}\n${source}\n${ext.markdown.close}`;
      content.title = `${pluginName} plugin removed — raw markdown preserved`;
    };

    renderSource((node.attrs.source as string) || "");
    dom.append(header, content);

    return {
      dom,
      update(updatedNode: any) {
        if (updatedNode.type.name !== ext.extension_id) return false;
        renderSource((updatedNode.attrs.source as string) || "");
        return true;
      },
      destroy() {},
    };
  };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function createInlineAtomView(ext: EditorExtensionManifest, renderFn: RenderFn): any {
  return ({ node, getPos, editor }: { node: any; getPos: () => number | undefined; editor: any }) => {
    const dom = document.createElement("span");
    dom.classList.add("atom-inline-wrapper");
    dom.style.display = "inline";
    dom.setAttribute("contenteditable", "false");

    let currentSource = (node.attrs.source as string) || "";
    let editing = false;

    function updateSource(newSource: string) {
      const pos = getPos();
      if (typeof pos !== "number") return;
      editor.view.dispatch(
        editor.view.state.tr.setNodeMarkup(pos, null, { source: newSource }),
      );
    }

    function renderPreview(source: string) {
      dom.textContent = "";
      if (!source.trim()) {
        const empty = document.createElement("span");
        empty.className = "atom-inline-empty";
        empty.textContent = `${ext.markdown.open}\u2026${ext.markdown.close}`;
        dom.appendChild(empty);
        return;
      }
      const loading = document.createElement("span");
      loading.className = "atom-inline-loading";
      loading.textContent = "\u2026";
      dom.appendChild(loading);

      renderFn(source, false)
        .then((result) => {
          if (editing) return;
          dom.textContent = "";
          if (result.html) {
            const rendered = document.createElement("span");
            rendered.className = "atom-inline-rendered";
            rendered.title = source;
            rendered.innerHTML = result.html;
            dom.appendChild(rendered);
          } else {
            const err = document.createElement("code");
            err.className = "atom-inline-error";
            err.textContent = `${ext.markdown.open}${source}${ext.markdown.close}`;
            dom.appendChild(err);
          }
        })
        .catch(() => {
          if (editing) return;
          dom.textContent = "";
          const err = document.createElement("code");
          err.className = "atom-inline-error";
          err.textContent = `${ext.markdown.open}${source}${ext.markdown.close}`;
          dom.appendChild(err);
        });
    }

    function startEditing() {
      if (!editor.isEditable) return;
      editing = true;
      dom.textContent = "";

      const wrapper = document.createElement("span");
      wrapper.className = "atom-inline-editing";

      const openDelim = document.createElement("span");
      openDelim.className = "atom-inline-delim";
      openDelim.textContent = ext.markdown.open;

      const input = document.createElement("input");
      input.type = "text";
      input.className = "atom-inline-input";
      input.value = currentSource;
      input.spellcheck = false;

      const closeDelim = document.createElement("span");
      closeDelim.className = "atom-inline-delim";
      closeDelim.textContent = ext.markdown.close;

      wrapper.append(openDelim, input, closeDelim);
      dom.appendChild(wrapper);
      input.focus();
      input.select();

      function commit() {
        if (!editing) return;
        editing = false;
        const val = input.value;
        if (val !== currentSource) {
          updateSource(val);
        } else {
          renderPreview(currentSource);
        }
      }

      input.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          commit();
        } else if (e.key === "Escape") {
          editing = false;
          renderPreview(currentSource);
        }
      });
      input.addEventListener("blur", commit);
    }

    dom.addEventListener("click", () => {
      if (!editing) startEditing();
    });
    renderPreview(currentSource);

    return {
      dom,
      stopEvent: (event: Event) => dom.contains(event.target as globalThis.Node),
      update(updatedNode: any) {
        if (updatedNode.type.name !== ext.extension_id) return false;
        const newSource = (updatedNode.attrs.source as string) || "";
        if (newSource !== currentSource) {
          currentSource = newSource;
          if (!editing) renderPreview(newSource);
        }
        return true;
      },
      destroy() {},
    };
  };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function createBlockAtomView(ext: EditorExtensionManifest, renderFn: RenderFn): any {
  const blockLabel = ext.insert_command?.label || ext.extension_id;

  return ({ node, getPos, editor }: { node: any; getPos: () => number | undefined; editor: any }) => {
    const dom = document.createElement("div");
    dom.className = "atom-block-container";
    dom.setAttribute("contenteditable", "false");

    let currentSource = (node.attrs.source as string) || "";
    let sourceMode = false;

    function updateSource(newSource: string) {
      const pos = getPos();
      if (typeof pos !== "number") return;
      editor.view.dispatch(
        editor.view.state.tr.setNodeMarkup(pos, null, { source: newSource }),
      );
    }

    // Header
    const header = document.createElement("div");
    header.className = "atom-block-header";

    const label = document.createElement("span");
    label.className = "atom-block-label";
    label.textContent = blockLabel;

    const toggleBtn = document.createElement("button");
    toggleBtn.type = "button";
    toggleBtn.className = "atom-block-toggle";
    toggleBtn.title = "Edit source";
    toggleBtn.textContent = "</>";

    header.append(label, toggleBtn);

    // Content area
    const content = document.createElement("div");
    dom.append(header, content);

    function renderPreview(source: string) {
      sourceMode = false;
      toggleBtn.textContent = "</>";
      toggleBtn.title = "Edit source";
      content.textContent = "";
      content.className = "atom-block-preview";

      if (!source.trim()) {
        const empty = document.createElement("span");
        empty.className = "atom-block-empty";
        empty.textContent = `Empty ${blockLabel.toLowerCase()} block`;
        content.appendChild(empty);
        return;
      }

      const loading = document.createElement("span");
      loading.className = "atom-block-loading";
      loading.textContent = "Rendering\u2026";
      content.appendChild(loading);

      renderFn(source, true)
        .then((result) => {
          if (sourceMode) return;
          content.textContent = "";
          if (result.html) {
            content.innerHTML = result.html;
          } else {
            const errDiv = document.createElement("div");
            errDiv.className = "atom-block-error";
            const code = document.createElement("code");
            code.textContent = source;
            const msg = document.createElement("span");
            msg.className = "atom-block-error-msg";
            msg.textContent = result.error || "Render failed";
            errDiv.append(code, msg);
            content.appendChild(errDiv);
          }
        })
        .catch((e) => {
          if (sourceMode) return;
          content.textContent = "";
          const errDiv = document.createElement("div");
          errDiv.className = "atom-block-error";
          const code = document.createElement("code");
          code.textContent = source;
          const msg = document.createElement("span");
          msg.className = "atom-block-error-msg";
          msg.textContent = e instanceof Error ? e.message : String(e);
          errDiv.append(code, msg);
          content.appendChild(errDiv);
        });
    }

    function showSourceEditor() {
      sourceMode = true;
      toggleBtn.textContent = "\u2713";
      toggleBtn.title = "Done editing";
      content.textContent = "";
      content.className = "";

      const textarea = document.createElement("textarea");
      textarea.className = "atom-block-source";
      textarea.value = currentSource;
      textarea.spellcheck = false;
      textarea.placeholder = `Enter ${blockLabel.toLowerCase()} source\u2026`;
      content.appendChild(textarea);

      function commit() {
        if (!sourceMode) return;
        const val = textarea.value;
        if (val !== currentSource) {
          updateSource(val);
        } else {
          renderPreview(currentSource);
        }
      }

      textarea.addEventListener("keydown", (e) => {
        if (e.key === "Escape") {
          renderPreview(currentSource);
        } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
          commit();
        }
      });
      textarea.addEventListener("blur", commit);
    }

    if (editor.isEditable) {
      toggleBtn.addEventListener("click", () => {
        if (sourceMode) {
          const textarea = content.querySelector("textarea");
          if (textarea) {
            const val = textarea.value;
            if (val !== currentSource) updateSource(val);
            else renderPreview(currentSource);
          } else {
            renderPreview(currentSource);
          }
        } else {
          showSourceEditor();
        }
      });
    } else {
      toggleBtn.style.display = "none";
    }

    renderPreview(currentSource);

    return {
      dom,
      stopEvent: (event: Event) => dom.contains(event.target as globalThis.Node),
      update(updatedNode: any) {
        if (updatedNode.type.name !== ext.extension_id) return false;
        const newSource = (updatedNode.attrs.source as string) || "";
        if (newSource !== currentSource) {
          currentSource = newSource;
          if (!sourceMode) renderPreview(newSource);
        }
        return true;
      },
      destroy() {},
    };
  };
}
