/**
 * TipTap Node extension for conditional block markers.
 *
 * Models Handlebars block helpers ({{#if}}, {{#for-audience}}, {{else}}, {{/if}},
 * {{/for-audience}}) as block-level atom nodes that act as visual separators.
 * Content between markers is normal fully-editable TipTap content.
 *
 * A ProseMirror decoration plugin adds a colored left border to content nodes
 * between paired markers, with active/inactive branch highlighting based on
 * the current frontmatter context.
 *
 * Markdown round-trip:
 * - Parse: Block tokenizer matches lines that are purely Handlebars block helpers.
 * - Serialize: `renderMarkdown` outputs the original Handlebars syntax.
 *
 * Does NOT conflict with the inline TemplateVariable tokenizer, which already
 * skips {{#...}}, {{/...}}, and {{else}}.
 */

import { Node, mergeAttributes } from "@tiptap/core";
import {
  Plugin as ProseMirrorPlugin,
  PluginKey,
  TextSelection,
} from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import type { Node as PmNode } from "@tiptap/pm/model";
import ConditionalMarkerNodeView from "../components/ConditionalMarkerNodeView.svelte";
import { mount, unmount } from "svelte";
import { getTemplateContextStore } from "../stores/templateContextStore.svelte";

export interface ConditionalBlockOptions {
  enabled: boolean;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    conditionalBlock: {
      /** Insert a conditional block with open + else + close markers */
      insertConditionalBlock: (opts: {
        helperType: "if" | "for-audience";
        condition: string;
      }) => ReturnType;
    };
  }
}

// ---------------------------------------------------------------------------
// Decoration helpers
// ---------------------------------------------------------------------------

type MarkerInfo = {
  pos: number;
  nodeSize: number;
  variant: string;
  helperType: string;
  condition: string;
};

type ConditionalBlockInfo = {
  open: MarkerInfo;
  elseMarker: MarkerInfo | null;
  close: MarkerInfo;
};

/**
 * Evaluate whether a condition is truthy against the current template context.
 */
function evaluateCondition(
  helperType: string,
  condition: string,
  context: Record<string, unknown>,
): boolean {
  if (helperType === "for-audience") {
    const audience = context.audience;
    if (Array.isArray(audience)) {
      return audience.includes(condition);
    }
    return false;
  }
  // Generic "if" — truthy check on frontmatter variable
  const value = context[condition];
  return !!value;
}

/**
 * Add node decorations to block nodes within a position range.
 */
function addBranchDecorations(
  doc: PmNode,
  decorations: Decoration[],
  from: number,
  to: number,
  className: string,
) {
  doc.nodesBetween(from, to, (node, pos) => {
    if (pos >= from && pos < to && node.isBlock && !node.isAtom) {
      decorations.push(
        Decoration.node(pos, pos + node.nodeSize, { class: className }),
      );
      return false; // don't recurse into children
    }
    return true;
  });
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

const conditionalDecorationsKey = new PluginKey("conditionalBlockDecorations");

export const ConditionalBlock = Node.create<ConditionalBlockOptions>({
  name: "conditionalMarker",

  group: "block",

  atom: true,

  draggable: false,

  selectable: true,

  addOptions() {
    return { enabled: true };
  },

  addAttributes() {
    return {
      variant: { default: "open" }, // "open" | "else" | "close"
      helperType: { default: "if" }, // "if" | "for-audience"
      condition: { default: "" }, // the condition expression
    };
  },

  parseHTML() {
    return [{ tag: "div[data-conditional-marker]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return [
      "div",
      mergeAttributes(HTMLAttributes, {
        "data-conditional-marker": "",
        "data-variant": HTMLAttributes.variant,
        class: "conditional-marker",
      }),
    ];
  },

  addCommands() {
    return {
      insertConditionalBlock:
        ({ helperType, condition }) =>
        ({ editor, tr, dispatch }) => {
          if (!dispatch) return true;

          const { schema } = editor;
          const markerType = schema.nodes.conditionalMarker;
          const paragraphType = schema.nodes.paragraph;

          const openMarker = markerType.create({
            variant: "open",
            helperType,
            condition,
          });
          const elseMarker = markerType.create({
            variant: "else",
            helperType: "",
            condition: "",
          });
          const closeMarker = markerType.create({
            variant: "close",
            helperType,
            condition: "",
          });

          const emptyParagraph1 = paragraphType.create();
          const emptyParagraph2 = paragraphType.create();

          // Node selections on top-level block atoms resolve at doc depth 0,
          // so `end(1)` is invalid there. Fall back to inserting after the
          // selected node/current cursor position in that case.
          const insertPos = tr.selection.$from.depth >= 1
            ? tr.selection.$from.after(1)
            : tr.selection.to;

          const fragment = [
            openMarker,
            emptyParagraph1,
            elseMarker,
            emptyParagraph2,
            closeMarker,
          ];

          let offset = insertPos;
          for (const node of fragment) {
            tr.insert(offset, node);
            offset += node.nodeSize;
          }

          // Position cursor inside the first empty paragraph
          const cursorPos = insertPos + openMarker.nodeSize + 1;
          tr.setSelection(TextSelection.create(tr.doc, cursorPos));

          dispatch(tr);
          return true;
        },
    };
  },

  addNodeView() {
    return ({ node, getPos, editor }) => {
      const dom = document.createElement("div");
      dom.classList.add("conditional-marker-wrapper");
      dom.setAttribute("contenteditable", "false");

      let currentAttrs = { ...node.attrs };
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      let svelteComponent: Record<string, any> | null = null;

      const onDelete = () => {
        const pos = getPos();
        if (typeof pos !== "number") return;
        const tr = editor.view.state.tr.delete(pos, pos + node.nodeSize);
        editor.view.dispatch(tr);
      };

      function mountComponent(
        attrs: Record<string, unknown>,
      ) {
        svelteComponent = mount(ConditionalMarkerNodeView, {
          target: dom,
          props: {
            variant: attrs.variant as "open" | "else" | "close",
            helperType: attrs.helperType as string,
            condition: attrs.condition as string,
            readonly: !editor.isEditable,
            onDelete,
          },
        });
      }

      mountComponent(currentAttrs);

      return {
        dom,
        stopEvent(event: Event) {
          return dom.contains(event.target as globalThis.Node);
        },
        update(updatedNode) {
          if (updatedNode.type.name !== "conditionalMarker") return false;
          const newAttrs = { ...updatedNode.attrs };
          if (
            newAttrs.variant !== currentAttrs.variant ||
            newAttrs.condition !== currentAttrs.condition ||
            newAttrs.helperType !== currentAttrs.helperType
          ) {
            currentAttrs = newAttrs;
            if (svelteComponent) unmount(svelteComponent);
            mountComponent(newAttrs);
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

  addProseMirrorPlugins() {
    if (!this.options.enabled) return [];

    const templateContextStore = getTemplateContextStore();

    return [
      new ProseMirrorPlugin({
        key: conditionalDecorationsKey,
        props: {
          decorations(state) {
            const decorations: Decoration[] = [];
            const doc = state.doc;

            // Collect all conditional markers with positions
            const markers: MarkerInfo[] = [];
            doc.forEach((node, offset) => {
              if (node.type.name === "conditionalMarker") {
                markers.push({
                  pos: offset,
                  nodeSize: node.nodeSize,
                  variant: node.attrs.variant as string,
                  helperType: node.attrs.helperType as string,
                  condition: node.attrs.condition as string,
                });
              }
            });

            // Pair markers using a stack (handles nesting)
            const stack: {
              open: MarkerInfo;
              elseMarker: MarkerInfo | null;
            }[] = [];
            const completedBlocks: ConditionalBlockInfo[] = [];

            for (const m of markers) {
              if (m.variant === "open") {
                stack.push({ open: m, elseMarker: null });
              } else if (m.variant === "else") {
                if (stack.length > 0) {
                  stack[stack.length - 1].elseMarker = m;
                }
              } else if (m.variant === "close") {
                if (stack.length > 0) {
                  const block = stack.pop()!;
                  completedBlocks.push({
                    open: block.open,
                    elseMarker: block.elseMarker,
                    close: m,
                  });
                }
              }
            }

            // Decorate branches with active/inactive/hidden classes
            const context = templateContextStore.context;
            const preview = templateContextStore.previewAudience;
            const isPreview = preview !== null;

            // In preview mode, override audience context
            const evalContext = isPreview
              ? { ...context, audience: preview }
              : context;

            for (const block of completedBlocks) {
              const isActive = evaluateCondition(
                block.open.helperType,
                block.open.condition,
                evalContext,
              );

              // In preview mode, hide all marker nodes
              if (isPreview) {
                decorations.push(
                  Decoration.node(
                    block.open.pos,
                    block.open.pos + block.open.nodeSize,
                    { class: "conditional-marker-hidden" },
                  ),
                );
                if (block.elseMarker) {
                  decorations.push(
                    Decoration.node(
                      block.elseMarker.pos,
                      block.elseMarker.pos + block.elseMarker.nodeSize,
                      { class: "conditional-marker-hidden" },
                    ),
                  );
                }
                decorations.push(
                  Decoration.node(
                    block.close.pos,
                    block.close.pos + block.close.nodeSize,
                    { class: "conditional-marker-hidden" },
                  ),
                );
              }

              // "if" branch: from after open marker to before else/close
              const ifStart = block.open.pos + block.open.nodeSize;
              const ifEnd = block.elseMarker
                ? block.elseMarker.pos
                : block.close.pos;

              if (isActive) {
                // Active branch: no decoration in preview (looks normal), colored border otherwise
                if (!isPreview) {
                  addBranchDecorations(
                    doc, decorations, ifStart, ifEnd,
                    "conditional-branch-active",
                  );
                }
              } else {
                addBranchDecorations(
                  doc, decorations, ifStart, ifEnd,
                  isPreview ? "conditional-branch-hidden" : "conditional-branch-inactive",
                );
              }

              // "else" branch (if present): from after else marker to before close
              if (block.elseMarker) {
                const elseStart =
                  block.elseMarker.pos + block.elseMarker.nodeSize;
                const elseEnd = block.close.pos;

                if (isActive) {
                  // Else is inactive when if-branch is active
                  addBranchDecorations(
                    doc, decorations, elseStart, elseEnd,
                    isPreview ? "conditional-branch-hidden" : "conditional-branch-inactive",
                  );
                } else {
                  // Else is active when if-branch is inactive
                  if (!isPreview) {
                    addBranchDecorations(
                      doc, decorations, elseStart, elseEnd,
                      "conditional-branch-active",
                    );
                  }
                }
              }
            }

            return DecorationSet.create(doc, decorations);
          },
        },
      }),
    ];
  },

  // Block-level tokenizer for Handlebars block helpers
  // @ts-ignore - markdownTokenizer is a custom field for @tiptap/markdown
  markdownTokenizer: {
    name: "conditionalMarker",
    level: "block",
    start(src: string) {
      const match = src.match(/^\{\{[#/]|^\{\{else\}\}/m);
      return match ? match.index! : -1;
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    tokenize(src: string, _tokens: any[]) {
      // Open: {{#if condition}} or {{#for-audience "value"}}
      let match = /^\{\{#(if|for-audience)\s+"?([^}"]+)"?\s*\}\}\s*\n?/.exec(
        src,
      );
      if (match) {
        return {
          type: "conditionalMarker",
          raw: match[0],
          variant: "open",
          helperType: match[1],
          condition: match[2].trim(),
        };
      }

      // Else: {{else}}
      match = /^\{\{else\}\}\s*\n?/.exec(src);
      if (match) {
        return {
          type: "conditionalMarker",
          raw: match[0],
          variant: "else",
          helperType: "",
          condition: "",
        };
      }

      // Close: {{/if}} or {{/for-audience}}
      match = /^\{\{\/(if|for-audience)\}\}\s*\n?/.exec(src);
      if (match) {
        return {
          type: "conditionalMarker",
          raw: match[0],
          variant: "close",
          helperType: match[1],
          condition: "",
        };
      }

      return undefined;
    },
  },

  // Parse the conditionalMarker token into a node
  // @ts-ignore - parseMarkdown is a custom field for @tiptap/markdown
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  parseMarkdown(token: any, helpers: any) {
    return helpers.createNode("conditionalMarker", {
      variant: token.variant,
      helperType: token.helperType,
      condition: token.condition,
    });
  },
}).extend({
  // Render conditional marker back to Handlebars syntax
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  renderMarkdown(node: any) {
    const { variant, helperType, condition } = node.attrs ?? {};
    switch (variant) {
      case "open": {
        if (helperType === "for-audience") {
          return `{{#for-audience "${condition}"}}\n`;
        }
        return `{{#if ${condition}}}\n`;
      }
      case "else":
        return `{{else}}\n`;
      case "close":
        return `{{/${helperType || "if"}}}\n`;
      default:
        return "";
    }
  },
});
