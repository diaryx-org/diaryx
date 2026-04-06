/**
 * VisibilityMark — Inline audience-visibility directive as a TipTap Mark.
 *
 * Markdown syntax:  :vis[visible text]{audience1 audience2}
 *
 * Semantics:
 * - The wrapped text is visible only to the listed audiences (union).
 * - When nested, the effective audience is the intersection of all
 *   enclosing visibility marks.
 * - In filter/preview mode, non-matching content is hidden.
 *
 * Visual design:
 * - Subtle dotted bottom border in the audience's color (distinct from
 *   highlights which use background fill, and links which use solid underline).
 * - Colored dot in the gutter on lines containing visibility marks.
 * - On hover: faint background tint to show the full extent of the span.
 *
 * Integrates with:
 * - EditorGutter for gutter dots
 * - VisibilityFilter for preview/filter mode hiding
 * - templateContextStore for preview audience state
 * - audienceColorStore for audience colors
 */

import { Mark, mergeAttributes } from "@tiptap/core";
import {
  Plugin as ProseMirrorPlugin,
  PluginKey,
} from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import type { Node as PmNode } from "@tiptap/pm/model";
import {
  createInlineDirectiveTokenizer,
  renderInlineDirective,
  serializeDirectiveAttrs,
  parseDirectiveAttrs,
} from "./directiveUtils";
import { createGutterMultiDot, createGutterEyeIcon } from "./EditorGutter";
import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
import { getAudienceColor } from "$lib/utils/audienceDotColor";
import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";

// ---------------------------------------------------------------------------
// Plugin keys
// ---------------------------------------------------------------------------

const visMarkGutterKey = new PluginKey("visibilityMarkGutter");
const visMarkHoverKey = new PluginKey("visibilityMarkHover");
const visMarkFilterKey = new PluginKey("visibilityMarkFilter");
const visMarkPointerKey = new PluginKey("visibilityMarkPointer");

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface VisibilityMarkOptions {
  /** Whether the mark's interactive features are active. Tokenizer is always active. */
  enabled: boolean;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    visibilityMark: {
      /** Apply a visibility mark to the current selection. */
      setVisibility: (opts: { audiences: string[] }) => ReturnType;
      /** Remove visibility mark from the current selection. */
      unsetVisibility: () => ReturnType;
      /** Toggle visibility mark on the current selection. */
      toggleVisibility: (opts: { audiences: string[] }) => ReturnType;
    };
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Collect all visibility mark ranges in the document, along with the resolved
 * audiences and the primary color for each range.
 *
 * The `code` mark (and potentially other marks with `excludes: "_"`) strip
 * the visibilityMark from their text nodes. This means a vis-mark span that
 * wraps inline code gets split into two (or more) separate ranges with a gap.
 * After the initial collection, we bridge these gaps: consecutive ranges in
 * the same block parent with the same audiences are merged into one range
 * that spans the full extent, including any code gaps.
 */
function audiencesEqual(a: string[], b: string[]): boolean {
  return (
    a.length === b.length && a.every((v, i) => v === b[i])
  );
}

function rangeMatchesPreview(
  audiences: string[],
  previewAudience: string | null,
): boolean {
  if (!previewAudience) return true;
  return audiences.some(
    (a) => a.toLowerCase() === previewAudience.toLowerCase(),
  );
}

export function collectVisRanges(
  doc: PmNode,
  markType: any, // eslint-disable-line @typescript-eslint/no-explicit-any
  colorMap: Record<string, string>,
): { from: number; to: number; audiences: string[]; color: string }[] {
  // Collect every inline node carrying the vis mark.
  const raw: { from: number; to: number; audiences: string[]; color: string }[] = [];

  doc.descendants((node, pos) => {
    if (!node.isInline) return;
    for (const mark of node.marks) {
      if (mark.type === markType) {
        const audiences: string[] = mark.attrs.audiences ?? [];
        const primaryAudience = audiences[0] ?? "";
        const color = primaryAudience
          ? getAudienceColor(primaryAudience, colorMap)
          : "bg-slate-500";

        raw.push({
          from: pos,
          to: pos + node.nodeSize,
          audiences,
          color,
        });
      }
    }
  });

  if (raw.length === 0) return raw;

  // Single merge pass: combine consecutive ranges in the same block
  // parent with the same audiences. This handles both:
  // - Contiguous ranges (ProseMirror splits one mark across multiple
  //   text nodes, e.g. around code marks)
  // - Small gaps (if code strips the vis mark, leaving a hole)
  const merged: typeof raw = [raw[0]];

  for (let i = 1; i < raw.length; i++) {
    const prev = merged[merged.length - 1];
    const curr = raw[i];

    if (!audiencesEqual(prev.audiences, curr.audiences)) {
      merged.push(curr);
      continue;
    }

    // Same audiences — merge if in the same block parent
    const $prev = doc.resolve(prev.from);
    const $curr = doc.resolve(curr.from);
    const sameBlock =
      $prev.depth > 0 &&
      $curr.depth > 0 &&
      $prev.before($prev.depth) === $curr.before($curr.depth);

    if (sameBlock) {
      prev.to = curr.to;
    } else {
      merged.push(curr);
    }
  }

  return merged;
}

function findVisRangeAtPos(
  doc: PmNode,
  markType: any, // eslint-disable-line @typescript-eslint/no-explicit-any
  colorMap: Record<string, string>,
  pos: number,
): { from: number; to: number; audiences: string[]; color: string } | null {
  const ranges = collectVisRanges(doc, markType, colorMap);
  return (
    ranges.find((range) => pos >= range.from && pos <= range.to) ?? null
  );
}

/**
 * Convert a Tailwind bg class to a CSS color value for inline styles.
 * Returns a reasonable approximation for common palette colors.
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

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export const VisibilityMark = Mark.create<VisibilityMarkOptions>({
  name: "visibilityMark",

  // Allow other marks inside (bold, italic, etc.)
  inclusive: true,

  addOptions() {
    return { enabled: true };
  },

  addAttributes() {
    return {
      audiences: {
        default: [],
        // Store as space-separated string in HTML
        parseHTML: (element) => {
          const raw = element.getAttribute("data-vis") ?? "";
          return parseDirectiveAttrs(raw);
        },
        renderHTML: (attributes) => {
          return {
            "data-vis": serializeDirectiveAttrs(attributes.audiences ?? []),
          };
        },
      },
    };
  },

  parseHTML() {
    return [{ tag: "span[data-vis]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return [
      "span",
      mergeAttributes(HTMLAttributes, {
        class: "vis-mark",
      }),
      0,
    ];
  },

  addCommands() {
    return {
      setVisibility:
        ({ audiences }) =>
        ({ commands }) => {
          return commands.setMark(this.name, { audiences });
        },
      unsetVisibility:
        () =>
        ({ commands }) => {
          return commands.unsetMark(this.name);
        },
      toggleVisibility:
        ({ audiences }) =>
        ({ commands }) => {
          return commands.toggleMark(this.name, { audiences });
        },
    };
  },

  addKeyboardShortcuts() {
    return {
      "Mod-Shift-v": () => {
        if (!this.options.enabled) return false;
        // If already active, remove it. Otherwise, we'd need a picker.
        // For keyboard shortcut, toggle off if active; otherwise it's a no-op
        // (the bubble menu picker is the primary way to apply audiences).
        if (this.editor.isActive(this.name)) {
          return this.editor.commands.unsetVisibility();
        }
        return false;
      },
    };
  },

  addProseMirrorPlugins() {
    if (!this.options.enabled) return [];

    const markType = this.type;
    const colorStore = getAudienceColorStore();
    const templateContextStore = getTemplateContextStore();

    // Captured by the gutter plugin's view() lifecycle so that
    // gutter dot click handlers can dispatch transactions.
    let gutterEditorView: import("@tiptap/pm/view").EditorView | null = null;

    /** Build gutter decorations from the current editor state. */
    function buildGutterDecorations(
      state: import("@tiptap/pm/state").EditorState,
    ): DecorationSet {
      const preview = templateContextStore.previewAudience;
      const ranges = collectVisRanges(
        state.doc,
        markType,
        colorStore.audienceColors,
      );
      if (ranges.length === 0) return DecorationSet.empty;

      const lineInfo = new Map<
        number,
        {
          colors: string[];
          audiences: Set<string>;
          anyMatches: boolean;
          ranges: { from: number; to: number; color: string }[];
        }
      >();

      for (const range of ranges) {
        const $pos = state.doc.resolve(range.from);
        let blockPos = $pos.before($pos.depth);
        if (blockPos < 0) blockPos = 0;

        let info = lineInfo.get(blockPos);
        if (!info) {
          info = {
            colors: [],
            audiences: new Set(),
            anyMatches: false,
            ranges: [],
          };
          lineInfo.set(blockPos, info);
        }

        const colorValue = tailwindBgToColor(range.color);
        if (!info.colors.includes(colorValue)) {
          info.colors.push(colorValue);
        }
        for (const a of range.audiences) info.audiences.add(a);
        info.ranges.push({
          from: range.from,
          to: range.to,
          color: colorValue,
        });

        if (
          rangeMatchesPreview(range.audiences, preview)
        ) {
          info.anyMatches = true;
        }
      }

      const hoverState = visMarkHoverKey.getState(state);
      const revealedBlock = hoverState?.revealedBlockPos ?? null;

      const decorations: Decoration[] = [];

      for (const [blockPos, info] of lineInfo) {
        const tooltip = `Visible to: ${[...info.audiences].join(", ")}`;
        const isRevealed = revealedBlock === blockPos;

        const toggleReveal = () => {
          if (!gutterEditorView) return;
          const curState = visMarkHoverKey.getState(
            gutterEditorView.state,
          );
          const alreadyRevealed =
            curState?.revealedBlockPos === blockPos;
          gutterEditorView.dispatch(
            gutterEditorView.state.tr.setMeta(visMarkHoverKey, {
              revealedBlockPos: alreadyRevealed ? null : blockPos,
              revealedRanges: alreadyRevealed ? null : info.ranges,
            }),
          );
        };

        let indicator: HTMLElement;

        if (preview) {
          indicator = createGutterEyeIcon(tooltip, toggleReveal);
          if (isRevealed) {
            indicator.classList.add("gutter-eye--active");
          }
        } else {
          indicator = createGutterMultiDot(
            info.colors,
            tooltip,
            toggleReveal,
          );
          if (isRevealed) {
            indicator.classList.add("gutter-dot--revealed");
          }
        }

        decorations.push(
          Decoration.widget(blockPos + 1, indicator, {
            side: -1,
            key: `vis-gutter-${blockPos}`,
          }),
        );
      }

      return DecorationSet.create(state.doc, decorations);
    }

    return [
      // ── Gutter dots ───────────────────────────────────────────────
      // For each line containing vis marks, place colored dot(s) in
      // the gutter. Clicking a dot reveals (highlights) all vis-mark
      // ranges on that line. Multiple vis marks with different audiences
      // on the same line produce a stacked multi-dot indicator.
      //
      // Uses plugin `state` (not `props.decorations`) so that the
      // decorations rebuild when the `templateContextChanged` meta is
      // dispatched — this is how the sidebar audience picker triggers
      // an immediate switch from dots → eye icons.
      new ProseMirrorPlugin({
        key: visMarkGutterKey,
        state: {
          init(_config, state) {
            return buildGutterDecorations(state);
          },
          apply(tr, _value, _oldState, newState) {
            // Rebuild on doc changes, reveal state changes, or
            // template context changes (preview audience toggled).
            if (
              tr.docChanged ||
              tr.getMeta(visMarkHoverKey) !== undefined ||
              tr.getMeta("templateContextChanged")
            ) {
              return buildGutterDecorations(newState);
            }
            return _value;
          },
        },
        view(editorView) {
          gutterEditorView = editorView;

          function syncPreviewClass() {
            const el = editorView.dom.closest(".editor-content");
            if (!el) return;
            if (templateContextStore.previewAudience) {
              el.classList.add("editor-vis-preview");
            } else {
              el.classList.remove("editor-vis-preview");
            }
          }

          syncPreviewClass();

          return {
            update(view) {
              gutterEditorView = view;
              syncPreviewClass();
            },
            destroy() {
              gutterEditorView = null;
              const el = editorView.dom.closest(".editor-content");
              el?.classList.remove("editor-vis-preview");
            },
          };
        },
        props: {
          decorations(state) {
            return visMarkGutterKey.getState(state) ?? DecorationSet.empty;
          },
        },
      }),

      // ── Pointer hover → whole-range hover state ───────────────────
      // Track the bridged vis range under the pointer so hover styling
      // applies to the whole directive, not just the single DOM fragment
      // the browser happens to be hovering.
      new ProseMirrorPlugin({
        key: visMarkPointerKey,
        state: {
          init() {
            return {
              hoveredRange: null as
                | { from: number; to: number; audiences: string[]; color: string }
                | null,
            };
          },
          apply(tr, value) {
            const meta = tr.getMeta(visMarkPointerKey);
            if (meta !== undefined) return meta;
            if (!tr.docChanged || !value.hoveredRange) return value;
            return {
              hoveredRange: {
                ...value.hoveredRange,
                from: tr.mapping.map(value.hoveredRange.from),
                to: tr.mapping.map(value.hoveredRange.to),
              },
            };
          },
        },
        props: {
          handleDOMEvents: {
            mousemove(view, event) {
              const target = event.target as HTMLElement | null;
              if (target?.closest(".gutter-indicator")) {
                return false;
              }

              const coords = view.posAtCoords({
                left: event.clientX,
                top: event.clientY,
              });
              const nextHovered = coords
                ? findVisRangeAtPos(
                    view.state.doc,
                    markType,
                    colorStore.audienceColors,
                    coords.pos,
                  )
                : null;
              const current =
                visMarkPointerKey.getState(view.state)?.hoveredRange ?? null;
              const unchanged =
                current?.from === nextHovered?.from &&
                current?.to === nextHovered?.to;

              if (!unchanged) {
                view.dispatch(
                  view.state.tr.setMeta(visMarkPointerKey, {
                    hoveredRange: nextHovered,
                  }),
                );
              }
              return false;
            },
            mouseleave(view) {
              const current =
                visMarkPointerKey.getState(view.state)?.hoveredRange ?? null;
              if (current) {
                view.dispatch(
                  view.state.tr.setMeta(visMarkPointerKey, {
                    hoveredRange: null,
                  }),
                );
              }
              return false;
            },
          },
        },
      }),

      // ── Gutter click → Reveal highlight ────────────────────────────
      // Clicking a gutter dot/eye persistently highlights all vis-mark
      // ranges on that line. Click again or click elsewhere to dismiss.
      //
      // In preview mode, matching ranges get an "included" highlight;
      // non-matching ranges get strikethrough (via the filter plugin).
      // In normal mode, all ranges get the standard revealed highlight.
      new ProseMirrorPlugin({
        key: visMarkHoverKey,
        state: {
          init() {
            return {
              revealedBlockPos: null as number | null,
              revealedRanges: null as
                | { from: number; to: number; color: string }[]
                | null,
            };
          },
          apply(tr, value) {
            const meta = tr.getMeta(visMarkHoverKey);
            if (meta !== undefined) return meta;
            if (!tr.docChanged) return value;
            return {
              revealedBlockPos: value.revealedBlockPos,
              revealedRanges: value.revealedRanges?.map((r) => ({
                from: tr.mapping.map(r.from),
                to: tr.mapping.map(r.to),
                color: r.color,
              })) ?? null,
            };
          },
        },
        props: {
          handleClick(view, _pos, event) {
            // Clicking anywhere in the editor clears the revealed state,
            // unless the click was on a gutter indicator (which has its
            // own toggle handler and calls stopPropagation).
            const target = event.target as HTMLElement;
            if (target.closest(".gutter-indicator")) return false;

            const current = visMarkHoverKey.getState(view.state);
            if (current?.revealedRanges) {
              view.dispatch(
                view.state.tr.setMeta(visMarkHoverKey, {
                  revealedBlockPos: null,
                  revealedRanges: null,
                }),
              );
            }
            return false;
          },
          decorations(state) {
            const pluginState = visMarkHoverKey.getState(state);
            if (!pluginState?.revealedRanges) return DecorationSet.empty;

            const preview = templateContextStore.previewAudience;
            const decorations: Decoration[] = [];

            for (const range of pluginState.revealedRanges) {
              if (
                range.from >= range.to ||
                range.from < 0 ||
                range.to > state.doc.content.size
              ) {
                continue;
              }

              if (preview) {
                // In preview mode, check if this range matches.
                const bridgedRanges = collectVisRanges(
                  state.doc,
                  markType,
                  colorStore.audienceColors,
                );
                const matched = bridgedRanges.find(
                  (r) => range.from >= r.from && range.to <= r.to,
                );
                const audiences: string[] = matched?.audiences ?? [];
                const matches = rangeMatchesPreview(audiences, preview);

                if (matches) {
                  decorations.push(
                    Decoration.inline(range.from, range.to, {
                      class: "vis-mark--revealed-included",
                      style: `--vis-hover-color: ${range.color};`,
                    }),
                  );
                }
                // Non-matching: filter plugin handles strikethrough
              } else {
                decorations.push(
                  Decoration.inline(range.from, range.to, {
                    class: "vis-mark--revealed",
                    style: `--vis-hover-color: ${range.color};`,
                  }),
                );
              }
            }

            if (decorations.length === 0) return DecorationSet.empty;
            return DecorationSet.create(state.doc, decorations);
          },
        },
      }),

      // ── Filter mode ───────────────────────────────────────────────
      // When previewAudience is set, hide non-matching vis-marked text.
      // If the user clicks a gutter dot to reveal hidden text, the
      // hidden class is replaced with a strikethrough class so the
      // content is visible but clearly marked as excluded.
      new ProseMirrorPlugin({
        key: visMarkFilterKey,
        props: {
          decorations(state) {
            const preview = templateContextStore.previewAudience;
            if (!preview) return DecorationSet.empty;

            const ranges = collectVisRanges(
              state.doc,
              markType,
              colorStore.audienceColors,
            );
            if (ranges.length === 0) return DecorationSet.empty;

            // Check which ranges are currently revealed via gutter click
            const hoverState = visMarkHoverKey.getState(state);
            const revealedRanges = hoverState?.revealedRanges ?? null;

            const decorations: Decoration[] = [];

            for (const range of ranges) {
              const matches = rangeMatchesPreview(
                range.audiences,
                preview,
              );

              if (!matches) {
                // Is this range currently revealed by a gutter click?
                const isRevealed =
                  revealedRanges?.some(
                    (r: { from: number; to: number }) =>
                      r.from === range.from && r.to === range.to,
                  ) ?? false;

                if (isRevealed) {
                  // Show with strikethrough instead of hiding
                  const colorValue = tailwindBgToColor(range.color);
                  decorations.push(
                    Decoration.inline(range.from, range.to, {
                      class: "vis-mark--revealed-filtered",
                      style: `--vis-hover-color: ${colorValue};`,
                    }),
                  );
                } else {
                  // Fully hide
                  decorations.push(
                    Decoration.inline(range.from, range.to, {
                      class: "vis-mark--hidden",
                    }),
                  );
                }
              }
            }

            return DecorationSet.create(state.doc, decorations);
          },
        },
      }),

      // ── Underline decoration ─────────────────────────────────────
      // Applies the colored underline as a decoration across the full
      // bridged range (including code gaps where the mark is stripped).
      // This replaces the CSS border-bottom on `.vis-mark` which can't
      // cover code spans. Also adds `vis-mark-selected` when the
      // selection overlaps, brightening the underline.
      new ProseMirrorPlugin({
        key: new PluginKey("visibilityMarkUnderline"),
        props: {
          decorations(state) {
            const preview = templateContextStore.previewAudience;
            const { from, to } = state.selection;
            const hoveredRange =
              visMarkPointerKey.getState(state)?.hoveredRange ?? null;
            const ranges = collectVisRanges(
              state.doc,
              markType,
              colorStore.audienceColors,
            );
            if (ranges.length === 0) return DecorationSet.empty;

            const decorations: Decoration[] = [];
            for (const range of ranges) {
              if (!rangeMatchesPreview(range.audiences, preview)) {
                continue;
              }

              const colorValue = tailwindBgToColor(range.color);
              const isSelected =
                !preview && from <= range.to && to >= range.from;
              const isHovered =
                hoveredRange?.from === range.from &&
                hoveredRange?.to === range.to;
              const classes = ["vis-underline"];

              if (preview) {
                classes.push("vis-underline--preview");
              } else if (isSelected) {
                classes.push("vis-underline--selected");
              }

              if (isHovered) {
                classes.push("vis-underline--hovered");
              }

              decorations.push(
                Decoration.inline(range.from, range.to, {
                  class: classes.join(" "),
                  style: `--vis-color: ${colorValue};`,
                }),
              );
            }

            return DecorationSet.create(state.doc, decorations);
          },
        },
      }),
    ];
  },

  // ── Markdown tokenizer ──────────────────────────────────────────
  // @ts-expect-error - custom field for @tiptap/markdown
  markdownTokenizer: createInlineDirectiveTokenizer("vis", "visibilityMark"),

  // @ts-expect-error - custom field for @tiptap/markdown
  parseMarkdown(
    token: { tokens?: unknown[]; attrs: string[] },
    helpers: {
      parseInline: (tokens: unknown[]) => unknown[];
      applyMark: (
        markType: string,
        content: unknown[],
        attrs?: unknown,
      ) => unknown;
    },
  ) {
    const content = token.tokens ? helpers.parseInline(token.tokens) : [];
    return helpers.applyMark("visibilityMark", content, {
      audiences: token.attrs ?? [],
    });
  },
}).extend({
  // renderMarkdown must be in .extend() to be discoverable by @tiptap/markdown
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  renderMarkdown(node: any, helpers: { renderChildren: (node: any) => string }) {
    const content = helpers.renderChildren(node);
    const audiences: string[] = node.attrs?.audiences ?? [];
    return renderInlineDirective("vis", content, audiences);
  },
});
