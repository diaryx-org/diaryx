/**
 * Editor Extension Factory — generates TipTap extensions from plugin manifest declarations.
 *
 * When a plugin declares `EditorExtension` entries in its manifest, this factory
 * creates corresponding TipTap extensions:
 * - **Atom nodes** (`InlineAtom`, `BlockAtom`): TipTap `Node` with Svelte node views
 *   and async rendering via the plugin's exported render function.
 * - **Inline marks** (`InlineMark`): TipTap `Mark` that wraps rich text with
 *   input/paste rules, keyboard shortcuts, and optional click behavior.
 */

import { Mark, Node, mergeAttributes } from "@tiptap/core";
import { markInputRule, markPasteRule } from "@tiptap/core";
import { Plugin as ProseMirrorPlugin, PluginKey } from "@tiptap/pm/state";
import { mount, unmount } from "svelte";
import type { BrowserExtismPlugin } from "./extismBrowserLoader";
import MathInlineNodeView from "$lib/components/MathInlineNodeView.svelte";
import MathBlockNodeView from "$lib/components/MathBlockNodeView.svelte";
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

/**
 * Create a TipTap Node extension from a manifest declaration and plugin reference.
 *
 * The generated extension:
 * 1. Defines a Node with `{ source: string }` attribute
 * 2. Generates markdown tokenizer from open/close delimiters
 * 3. Mounts a Svelte node view that calls the plugin's render export
 * 4. Supports source editing (popover for inline, source toggle for block)
 */
export function createExtensionFromManifest(
  ext: EditorExtensionManifest,
  plugin: BrowserExtismPlugin,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
): any {
  const isInline = ext.node_type === "InlineAtom";
  const isBlock = ext.node_type === "BlockAtom";

  // Escape delimiters for use in regex
  const openEsc = escapeRegex(ext.markdown.open);
  const closeEsc = escapeRegex(ext.markdown.close);

  // Inject CSS if declared
  if (ext.css) {
    injectCss(ext.extension_id, ext.css);
  }

  // Build the render function that calls the plugin
  const renderFn = async (
    source: string,
    displayMode: boolean,
  ): Promise<{ html?: string; error?: string }> => {
    return plugin.callRender(ext.render_export!, source, {
      display_mode: displayMode,
    });
  };

  const NodeComponent = isInline ? MathInlineNodeView : MathBlockNodeView;

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
          class: isInline ? "math-inline" : "math-block",
        },
        HTMLAttributes.source || "",
      ];
    },

    addNodeView() {
      return ({ node, getPos, editor }) => {
        const dom = document.createElement(isInline ? "span" : "div");
        dom.classList.add(
          isInline ? "math-inline-wrapper" : "math-block-wrapper",
        );
        if (isInline) {
          dom.style.display = "inline";
        }
        dom.setAttribute("contenteditable", "false");

        let currentSource = (node.attrs.source as string) || "";
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        let svelteComponent: Record<string, any> | null = null;

        const onUpdate = (newSource: string) => {
          const pos = getPos();
          if (typeof pos !== "number") return;
          const tr = editor.view.state.tr.setNodeMarkup(pos, null, {
            source: newSource,
          });
          editor.view.dispatch(tr);
        };

        function mountComponent(source: string) {
          svelteComponent = mount(NodeComponent, {
            target: dom,
            props: {
              source,
              readonly: !editor.isEditable,
              onUpdate,
              renderFn,
            },
          });
        }

        mountComponent(currentSource);

        return {
          dom,
          stopEvent(event: Event) {
            return dom.contains(event.target as globalThis.Node);
          },
          update(updatedNode) {
            if (updatedNode.type.name !== ext.extension_id) return false;
            const newSource = (updatedNode.attrs.source as string) || "";
            if (newSource !== currentSource) {
              currentSource = newSource;
              if (svelteComponent) {
                unmount(svelteComponent);
              }
              mountComponent(newSource);
            }
            return true;
          },
          destroy() {
            if (svelteComponent) {
              unmount(svelteComponent);
              svelteComponent = null;
            }
          },
        };
      };
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
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
): any {
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
  if (ext.css) {
    injectCss(ext.extension_id, ext.css);
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
          class: `${ext.extension_id}-mark ${hiddenClass}`,
        }),
        0,
      ];
    },

    addCommands() {
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
      if (!ext.keyboard_shortcut) return {};
      return {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        [ext.keyboard_shortcut]: () => (this as any).editor.commands[`toggle${pascalName}`](),
      };
    },

    addInputRules() {
      return [
        markInputRule({
          find: inputRegex,
          type: this.type,
        }),
      ];
    },

    addPasteRules() {
      return [
        markPasteRule({
          find: pasteRegex,
          type: this.type,
        }),
      ];
    },

    addProseMirrorPlugins() {
      if (!clickBehavior) return [];

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

function injectCss(id: string, css: string) {
  if (injectedCss.has(id)) return;
  injectedCss.add(id);
  const style = document.createElement("style");
  style.setAttribute("data-plugin-css", id);
  style.textContent = css;
  document.head.appendChild(style);
}
