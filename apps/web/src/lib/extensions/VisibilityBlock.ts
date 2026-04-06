/**
 * VisibilityBlock — Block-level audience-visibility directive as TipTap Nodes.
 *
 * Markdown syntax:
 *   :::vis{audience1 audience2}
 *   Content paragraphs here...
 *   :::
 *
 * Implementation follows the same open/close marker pattern as ConditionalBlock:
 * two atom nodes act as visual separators, and the content between them is
 * normal fully-editable TipTap content. A ProseMirror decoration plugin adds
 * a colored vertical bar in the gutter spanning from open to close marker.
 *
 * Semantics:
 * - Content between markers is visible only to the listed audiences (union).
 * - Nesting: inner block audiences intersect with outer audiences.
 * - In filter/preview mode, non-matching block content is hidden, with a
 *   gutter collapse indicator.
 */

import { Node, mergeAttributes } from "@tiptap/core";
import {
  Plugin as ProseMirrorPlugin,
  PluginKey,
  type EditorState,
  type Selection,
  TextSelection,
  NodeSelection,
} from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import {
  createBlockDirectiveTokenizer,
  renderBlockDirectiveOpen,
  renderBlockDirectiveClose,
  parseDirectiveAttrs,
  serializeDirectiveAttrs,
} from "./directiveUtils";
import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
import { getAudienceColor } from "$lib/utils/audienceDotColor";
import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";

// ---------------------------------------------------------------------------
// Plugin key
// ---------------------------------------------------------------------------

const visBlockDecoKey = new PluginKey("visibilityBlockDecorations");

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface VisibilityBlockOptions {
  enabled: boolean;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    visibilityBlock: {
      /** Insert a visibility block with open + close markers and an empty paragraph. */
      insertVisibilityBlock: (opts: { audiences: string[] }) => ReturnType;
      /** Wrap the current full-block selection in a visibility block. */
      wrapVisibilityBlock: (opts: { audiences: string[] }) => ReturnType;
      /** Set block visibility on the current selection or enclosing block. */
      setVisibilityBlock: (opts: { audiences: string[] }) => ReturnType;
      /** Remove the enclosing visibility block around the current selection. */
      unsetVisibilityBlock: () => ReturnType;
    };
  }
}

type MarkerInfo = {
  pos: number;
  nodeSize: number;
  variant: string;
  audiences: string[];
};

type VisBlockPair = {
  open: MarkerInfo;
  close: MarkerInfo;
};

type BlockBoundary = {
  depth: number;
  before: number;
  after: number;
  contentFrom: number;
  contentTo: number;
  parentBefore: number;
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Convert a Tailwind bg class to a CSS color value.
 */
function tailwindBgToColor(bgClass: string): string {
  const map: Record<string, string> = {
    "bg-indigo-500": "oklch(0.585 0.233 277.117)",
    "bg-teal-500": "oklch(0.704 0.14 180.72)",
    "bg-rose-500": "oklch(0.645 0.246 16.439)",
    "bg-amber-500": "oklch(0.769 0.188 70.08)",
    "bg-emerald-500": "oklch(0.696 0.17 162.48)",
    "bg-violet-500": "oklch(0.606 0.25 292.717)",
    "bg-cyan-500": "oklch(0.715 0.143 215.221)",
    "bg-orange-500": "oklch(0.702 0.209 41.348)",
    "bg-slate-500": "oklch(0.554 0.022 257.417)",
  };
  return map[bgClass] ?? "oklch(0.554 0.022 257.417)";
}

function collectVisBlockMarkers(
  doc: import("@tiptap/pm/model").Node,
): MarkerInfo[] {
  const markers: MarkerInfo[] = [];
  doc.forEach((node, offset) => {
    if (node.type.name === "visBlockMarker") {
      markers.push({
        pos: offset,
        nodeSize: node.nodeSize,
        variant: node.attrs.variant as string,
        audiences: (node.attrs.audiences as string[]) ?? [],
      });
    }
  });
  return markers;
}

export function collectVisBlockPairs(
  doc: import("@tiptap/pm/model").Node,
): VisBlockPair[] {
  const markers = collectVisBlockMarkers(doc);
  const stack: MarkerInfo[] = [];
  const pairs: VisBlockPair[] = [];

  for (const marker of markers) {
    if (marker.variant === "open") {
      stack.push(marker);
    } else if (marker.variant === "close" && stack.length > 0) {
      const open = stack.pop()!;
      pairs.push({ open, close: marker });
    }
  }

  return pairs;
}

function findBlockBoundary(
  $pos: Selection["$from"],
): BlockBoundary | null {
  for (let depth = $pos.depth; depth > 0; depth--) {
    const node = $pos.node(depth);
    if (!node.isBlock || node.type.name === "visBlockMarker") continue;

    return {
      depth,
      before: $pos.before(depth),
      after: $pos.after(depth),
      contentFrom: $pos.start(depth),
      contentTo: $pos.end(depth),
      parentBefore: depth > 1 ? $pos.before(depth - 1) : 0,
    };
  }

  return null;
}

function getWrapPositions(
  selection: Selection,
): {
  wrapFrom: number;
  wrapTo: number;
  selectionFrom: number;
  selectionTo: number;
} | null {
  if (selection.empty) return null;

  const firstBlock = findBlockBoundary(selection.$from);
  const lastBlock = findBlockBoundary(selection.$to);

  if (!firstBlock || !lastBlock) return null;
  if (firstBlock.depth !== lastBlock.depth) return null;
  if (firstBlock.parentBefore !== lastBlock.parentBefore) return null;
  if (selection.from !== firstBlock.contentFrom) return null;
  if (selection.to !== lastBlock.contentTo) return null;

  return {
    wrapFrom: firstBlock.before,
    wrapTo: lastBlock.after,
    selectionFrom: selection.from,
    selectionTo: selection.to,
  };
}

export function canWrapSelectionInVisibilityBlock(
  state: EditorState,
): boolean {
  return getWrapPositions(state.selection) !== null;
}

export function getVisibilityBlockForSelection(
  state: EditorState,
): VisBlockPair | null {
  const { from, to } = state.selection;
  const containingPairs = collectVisBlockPairs(state.doc)
    .filter((pair) => {
      const contentStart = pair.open.pos + pair.open.nodeSize;
      const contentEnd = pair.close.pos;
      return from >= contentStart && to <= contentEnd;
    })
    .sort(
      (a, b) =>
        (a.close.pos - a.open.pos) - (b.close.pos - b.open.pos),
    );

  return containingPairs[0] ?? null;
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export const VisibilityBlock = Node.create<VisibilityBlockOptions>({
  name: "visBlockMarker",

  group: "block",

  atom: true,

  draggable: false,

  selectable: true,

  addOptions() {
    return { enabled: true };
  },

  addAttributes() {
    return {
      variant: { default: "open" }, // "open" | "close"
      audiences: {
        default: [],
        parseHTML: (element) => {
          const raw = element.getAttribute("data-vis-audiences") ?? "";
          return parseDirectiveAttrs(raw);
        },
        renderHTML: (attributes) => {
          return {
            "data-vis-audiences": serializeDirectiveAttrs(
              attributes.audiences ?? [],
            ),
          };
        },
      },
    };
  },

  parseHTML() {
    return [{ tag: "div[data-vis-block]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return [
      "div",
      mergeAttributes(HTMLAttributes, {
        "data-vis-block": "",
        "data-variant": HTMLAttributes.variant,
        class: "vis-block-marker",
      }),
    ];
  },

  addCommands() {
    return {
      insertVisibilityBlock:
        ({ audiences }) =>
        ({ editor, tr, dispatch }) => {
          if (!dispatch) return true;

          const { schema } = editor;
          const markerType = schema.nodes.visBlockMarker;
          const paragraphType = schema.nodes.paragraph;

          const openMarker = markerType.create({
            variant: "open",
            audiences,
          });
          const closeMarker = markerType.create({
            variant: "close",
            audiences: [],
          });
          const emptyParagraph = paragraphType.create();

          const insertPos =
            tr.selection.$from.depth >= 1
              ? tr.selection.$from.after(1)
              : tr.selection.to;

          const fragment = [openMarker, emptyParagraph, closeMarker];

          let offset = insertPos;
          for (const node of fragment) {
            tr.insert(offset, node);
            offset += node.nodeSize;
          }

          // Position cursor inside the empty paragraph
          const cursorPos = insertPos + openMarker.nodeSize + 1;
          tr.setSelection(TextSelection.create(tr.doc, cursorPos));

          dispatch(tr);
          return true;
        },
      wrapVisibilityBlock:
        ({ audiences }) =>
        ({ editor, tr, dispatch }) => {
          if (!dispatch) return true;

          const wrap = getWrapPositions(tr.selection);
          if (!wrap) return false;

          const markerType = editor.schema.nodes.visBlockMarker;
          const openMarker = markerType.create({
            variant: "open",
            audiences,
          });
          const closeMarker = markerType.create({
            variant: "close",
            audiences: [],
          });

          tr.insert(wrap.wrapTo, closeMarker);
          tr.insert(wrap.wrapFrom, openMarker);
          tr.setSelection(
            TextSelection.create(
              tr.doc,
              wrap.selectionFrom + openMarker.nodeSize,
              wrap.selectionTo + openMarker.nodeSize,
            ),
          );

          dispatch(tr);
          return true;
        },
      setVisibilityBlock:
        ({ audiences }) =>
        ({ editor, state, tr, dispatch }) => {
          if (!dispatch) return true;

          const existing = getVisibilityBlockForSelection(state);
          const markerType = editor.schema.nodes.visBlockMarker;

          if (existing) {
            const openNode = tr.doc.nodeAt(existing.open.pos);
            if (!openNode || openNode.type !== markerType) return false;

            tr.setNodeMarkup(existing.open.pos, markerType, {
              ...openNode.attrs,
              audiences,
            });
            dispatch(tr);
            return true;
          }

          return editor.commands.wrapVisibilityBlock({ audiences });
        },
      unsetVisibilityBlock:
        () =>
        ({ state, tr, dispatch }) => {
          if (!dispatch) return true;

          const existing = getVisibilityBlockForSelection(state);
          if (!existing) return false;

          tr.delete(
            existing.close.pos,
            existing.close.pos + existing.close.nodeSize,
          );
          tr.delete(
            existing.open.pos,
            existing.open.pos + existing.open.nodeSize,
          );

          dispatch(tr);
          return true;
        },
    };
  },

  addNodeView() {
    return ({ node, getPos, editor: viewEditor }) => {
      const dom = document.createElement("div");
      dom.classList.add("vis-block-marker-wrapper");
      dom.setAttribute("contenteditable", "false");

      const variant = node.attrs.variant as string;
      const audiences: string[] = node.attrs.audiences ?? [];

      const colorStore = getAudienceColorStore();
      const primaryAudience = audiences[0] ?? "";
      const bgClass = primaryAudience
        ? getAudienceColor(primaryAudience, colorStore.audienceColors)
        : "";

      if (variant === "open" && audiences.length > 0) {
        // Render a minimal pill showing audience name(s)
        const pill = document.createElement("span");
        pill.className = "vis-block-pill";

        // Colored dot
        const dot = document.createElement("span");
        dot.className = "vis-block-pill-dot";
        if (bgClass) {
          dot.style.backgroundColor = tailwindBgToColor(bgClass);
        }
        pill.appendChild(dot);

        // Audience label
        const label = document.createElement("span");
        label.className = "vis-block-pill-label";
        label.textContent = audiences.join(", ");
        pill.appendChild(label);

        dom.appendChild(pill);
      } else if (variant === "close") {
        // Close marker: minimal visual indicator
        const line = document.createElement("span");
        line.className = "vis-block-close-line";
        dom.appendChild(line);
      }

      // Allow clicking the marker to select it (for deletion via backspace)
      dom.addEventListener("mousedown", (e) => {
        e.preventDefault();
        const pos = getPos();
        if (typeof pos === "number") {
          const { tr } = viewEditor.view.state;
          viewEditor.view.dispatch(
            tr.setSelection(NodeSelection.create(tr.doc, pos)),
          );
          viewEditor.view.focus();
        }
      });

      return {
        dom,
        stopEvent(event: Event) {
          return dom.contains(event.target as globalThis.Node);
        },
        update(updatedNode) {
          return updatedNode.type.name === "visBlockMarker";
        },
        destroy() {
          // No cleanup needed
        },
      };
    };
  },

  addProseMirrorPlugins() {
    if (!this.options.enabled) return [];

    const colorStore = getAudienceColorStore();
    const templateContextStore = getTemplateContextStore();

    return [
      new ProseMirrorPlugin({
        key: visBlockDecoKey,
        props: {
          decorations(state) {
            const decorations: Decoration[] = [];
            const doc = state.doc;
            const preview = templateContextStore.previewAudience;

            const pairs = collectVisBlockPairs(doc);

            for (const pair of pairs) {
              const { open, close } = pair;
              const audiences = open.audiences;
              const primaryAudience = audiences[0] ?? "";
              const bgClass = primaryAudience
                ? getAudienceColor(primaryAudience, colorStore.audienceColors)
                : "bg-slate-500";
              const colorValue = tailwindBgToColor(bgClass);

              const contentStart = open.pos + open.nodeSize;
              const contentEnd = close.pos;

              if (preview) {
                const matches = audiences.some(
                  (a) => a.toLowerCase() === preview.toLowerCase(),
                );

                if (!matches) {
                  // Hide markers and content
                  decorations.push(
                    Decoration.node(open.pos, open.pos + open.nodeSize, {
                      class: "vis-block--hidden",
                    }),
                  );
                  decorations.push(
                    Decoration.node(close.pos, close.pos + close.nodeSize, {
                      class: "vis-block--hidden",
                    }),
                  );

                  // Hide content between markers
                  doc.nodesBetween(contentStart, contentEnd, (node, pos) => {
                    if (
                      pos >= contentStart &&
                      pos < contentEnd &&
                      node.isBlock &&
                      !node.isAtom
                    ) {
                      decorations.push(
                        Decoration.node(pos, pos + node.nodeSize, {
                          class: "vis-block-content--hidden",
                        }),
                      );
                      return false;
                    }
                    return true;
                  });

                  // Insert a collapse indicator
                  decorations.push(
                    Decoration.widget(
                      contentStart,
                      () => {
                        const indicator = document.createElement("div");
                        indicator.className = "vis-block-collapse-indicator";
                        indicator.title = `Hidden content (visible to: ${audiences.join(", ")})`;
                        indicator.setAttribute("contenteditable", "false");

                        const dot = document.createElement("span");
                        dot.className = "vis-block-collapse-dot";
                        dot.style.backgroundColor = colorValue;
                        indicator.appendChild(dot);

                        const label = document.createElement("span");
                        label.className = "vis-block-collapse-label";
                        label.textContent = `${audiences.join(", ")}`;
                        indicator.appendChild(label);

                        return indicator;
                      },
                      { side: -1, key: `vis-block-collapse-${open.pos}` },
                    ),
                  );
                } else {
                  // Matching audience: show content normally, but add subtle
                  // gutter bar to indicate the block boundary
                  doc.nodesBetween(contentStart, contentEnd, (node, pos) => {
                    if (
                      pos >= contentStart &&
                      pos < contentEnd &&
                      node.isBlock &&
                      !node.isAtom
                    ) {
                      decorations.push(
                        Decoration.node(pos, pos + node.nodeSize, {
                          class: "vis-block-content--active",
                          style: `border-left-color: ${colorValue};`,
                        }),
                      );
                      return false;
                    }
                    return true;
                  });

                  // Hide markers in preview mode
                  decorations.push(
                    Decoration.node(open.pos, open.pos + open.nodeSize, {
                      class: "vis-block--hidden",
                    }),
                  );
                  decorations.push(
                    Decoration.node(close.pos, close.pos + close.nodeSize, {
                      class: "vis-block--hidden",
                    }),
                  );
                }
              } else {
                // Normal editing mode: add gutter bar for the content range
                doc.nodesBetween(contentStart, contentEnd, (node, pos) => {
                  if (
                    pos >= contentStart &&
                    pos < contentEnd &&
                    node.isBlock &&
                    !node.isAtom
                  ) {
                    decorations.push(
                      Decoration.node(pos, pos + node.nodeSize, {
                        class: "vis-block-content--active",
                        style: `border-left-color: ${colorValue};`,
                      }),
                    );
                    return false;
                  }
                  return true;
                });
              }
            }

            return DecorationSet.create(doc, decorations);
          },
        },
      }),
    ];
  },

  // ── Markdown tokenizer ──────────────────────────────────────────
  // @ts-ignore - custom field for @tiptap/markdown
  markdownTokenizer: createBlockDirectiveTokenizer("vis", "visBlockMarker"),

  // @ts-ignore - custom field for @tiptap/markdown
  parseMarkdown(
    token: { variant: string; attrs: string[] },
    helpers: {
      createNode: (
        type: string,
        attrs?: Record<string, unknown>,
      ) => unknown;
    },
  ) {
    return helpers.createNode("visBlockMarker", {
      variant: token.variant,
      audiences: token.attrs ?? [],
    });
  },
}).extend({
  // renderMarkdown must be in .extend() to be discoverable
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  renderMarkdown(node: any) {
    const { variant, audiences } = node.attrs ?? {};
    if (variant === "open") {
      return renderBlockDirectiveOpen("vis", audiences ?? []);
    }
    return renderBlockDirectiveClose();
  },
});
