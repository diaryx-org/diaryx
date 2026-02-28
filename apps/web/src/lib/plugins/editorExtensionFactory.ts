/**
 * Editor Extension Factory — generates TipTap extensions from plugin manifest declarations.
 *
 * When a plugin declares `EditorExtension` entries in its manifest, this factory
 * creates corresponding TipTap `Node` extensions with:
 * - Markdown tokenizer/parser/renderer (generated from declared open/close delimiters)
 * - Node views (Svelte components for rendering + editing)
 * - Async rendering via the plugin's exported render function
 */

import { Node } from "@tiptap/core";
import { mount, unmount } from "svelte";
import type { BrowserExtismPlugin } from "./extismBrowserLoader";
import MathInlineNodeView from "$lib/components/MathInlineNodeView.svelte";
import MathBlockNodeView from "$lib/components/MathBlockNodeView.svelte";

// ============================================================================
// Types matching the Rust EditorExtension manifest
// ============================================================================

export interface EditorExtensionManifest {
  slot: "EditorExtension";
  extension_id: string;
  node_type: "InlineAtom" | "BlockAtom";
  markdown: {
    level: "Inline" | "Block";
    open: string;
    close: string;
  };
  render_export: string;
  edit_mode: "Popover" | "SourceToggle";
  css: string | null;
  insert_command?: {
    label: string;
    icon?: string | null;
    description?: string | null;
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
    return plugin.callRender(ext.render_export, source, {
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
