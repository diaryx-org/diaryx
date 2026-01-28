/**
 * TipTap Mark extension for colored highlight syntax.
 *
 * Renders ==text== or =={color}text== as a colored highlight.
 * Supports:
 * - Input rule: typing ==text== or =={red}text== converts to highlight
 * - Paste rule: pasted ==text== converts to highlight
 * - Keyboard shortcut: Mod-Shift-H to toggle yellow highlight
 * - Markdown serialization/parsing via ==content== or =={color}content==
 * - 10 predefined colors: red, orange, yellow (default), green, cyan, blue, violet, pink, brown, grey
 */

import { Mark, mergeAttributes } from "@tiptap/core";
import { Plugin as ProseMirrorPlugin, PluginKey } from "@tiptap/pm/state";
import type { MarkType } from "@tiptap/pm/model";

// Valid highlight colors
export const HIGHLIGHT_COLORS = [
  "red",
  "orange",
  "yellow",
  "green",
  "cyan",
  "blue",
  "violet",
  "pink",
  "brown",
  "grey",
] as const;

export type HighlightColor = (typeof HIGHLIGHT_COLORS)[number];

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    coloredHighlight: {
      /** Set the highlight mark with a specific color */
      setColoredHighlight: (color?: HighlightColor) => ReturnType;
      /** Toggle the highlight mark with a specific color */
      toggleColoredHighlight: (color?: HighlightColor) => ReturnType;
      /** Remove the highlight mark */
      unsetColoredHighlight: () => ReturnType;
    };
  }
}

export interface ColoredHighlightOptions {
  HTMLAttributes: Record<string, unknown>;
}

/**
 * Custom input rule that handles =={color}text== syntax.
 * TipTap's built-in markInputRule doesn't support capturing attributes from the match,
 * so we need a custom implementation.
 */
function coloredHighlightInputRule(markType: MarkType) {
  return new ProseMirrorPlugin({
    key: new PluginKey("coloredHighlightInputRule"),
    props: {
      handleTextInput(view, from, to, text) {
        // Only trigger when typing the closing ==
        if (text !== "=") return false;

        const { state } = view;
        const $from = state.doc.resolve(from);

        // Get text before cursor in current text block
        const textBefore = $from.parent.textBetween(
          Math.max(0, $from.parentOffset - 100),
          $from.parentOffset,
          undefined,
          "\ufffc"
        );

        // Check for pattern: =={color}text= or ==text= (we're about to type the second =)
        // Note: we need ==text= because user is typing the final =
        const partialMatch = /==(?:\{(\w+)\})?([^=]+)=$/.exec(textBefore);
        if (!partialMatch) return false;

        const colorMatch = partialMatch[1];
        const content = partialMatch[2];

        // Validate color if provided
        let color: HighlightColor = "yellow";
        if (colorMatch) {
          if (HIGHLIGHT_COLORS.includes(colorMatch as HighlightColor)) {
            color = colorMatch as HighlightColor;
          } else {
            // Invalid color, don't apply the rule
            return false;
          }
        }

        // Calculate the full match length
        const fullMatchLength = partialMatch[0].length + 1; // +1 for the = we're typing

        // Calculate positions
        const matchStart = from - (fullMatchLength - 1);

        // Create transaction
        const tr = state.tr;

        // Delete the matched syntax
        tr.delete(matchStart, to);

        // Insert the plain text content with the highlight mark
        const mark = markType.create({ color });
        tr.insert(matchStart, state.schema.text(content, [mark]));

        view.dispatch(tr);
        return true;
      },
    },
  });
}

/**
 * Custom paste rule that handles =={color}text== syntax in pasted content.
 */
function coloredHighlightPasteRule(markType: MarkType) {
  const pasteRegex = /==(?:\{(\w+)\})?([^=]+)==/g;

  return new ProseMirrorPlugin({
    key: new PluginKey("coloredHighlightPasteRule"),
    props: {
      transformPasted(slice) {
        const { content } = slice;
        const newContent: typeof content.content = [];

        content.forEach((node) => {
          if (node.isText && node.text) {
            const text = node.text;
            let lastIndex = 0;
            let match;
            const fragments: { text: string; marks: readonly ReturnType<typeof markType.create>[] }[] = [];

            // Reset regex
            pasteRegex.lastIndex = 0;

            while ((match = pasteRegex.exec(text)) !== null) {
              // Add text before match
              if (match.index > lastIndex) {
                fragments.push({
                  text: text.slice(lastIndex, match.index),
                  marks: node.marks as readonly ReturnType<typeof markType.create>[],
                });
              }

              // Determine color
              const colorMatch = match[1];
              let color: HighlightColor = "yellow";
              if (colorMatch && HIGHLIGHT_COLORS.includes(colorMatch as HighlightColor)) {
                color = colorMatch as HighlightColor;
              }

              // Add highlighted text
              const highlightMark = markType.create({ color });
              fragments.push({
                text: match[2],
                marks: [...(node.marks as readonly ReturnType<typeof markType.create>[]), highlightMark],
              });

              lastIndex = match.index + match[0].length;
            }

            // Add remaining text
            if (lastIndex < text.length) {
              fragments.push({
                text: text.slice(lastIndex),
                marks: node.marks as readonly ReturnType<typeof markType.create>[],
              });
            }

            // Convert fragments to nodes
            if (fragments.length > 0) {
              fragments.forEach((frag) => {
                if (frag.text) {
                  // @ts-expect-error - ProseMirror types are complex
                  newContent.push(node.type.schema.text(frag.text, frag.marks));
                }
              });
            } else {
              // @ts-expect-error - ProseMirror types are complex
              newContent.push(node);
            }
          } else {
            // @ts-expect-error - ProseMirror types are complex
            newContent.push(node);
          }
        });

        // @ts-expect-error - ProseMirror slice construction
        return new slice.constructor(slice.content.constructor.from(newContent), slice.openStart, slice.openEnd);
      },
    },
  });
}

export const ColoredHighlightMark = Mark.create<ColoredHighlightOptions>({
  name: "coloredHighlight",

  addOptions() {
    return {
      HTMLAttributes: {},
    };
  },

  addAttributes() {
    return {
      color: {
        default: "yellow",
        parseHTML: (element) => element.getAttribute("data-highlight-color") || "yellow",
        renderHTML: (attributes) => ({
          "data-highlight-color": attributes.color,
        }),
      },
    };
  },

  parseHTML() {
    return [
      {
        tag: "mark[data-highlight-color]",
      },
      {
        tag: "mark",
        getAttrs: () => ({ color: "yellow" }),
      },
    ];
  },

  renderHTML({ HTMLAttributes }) {
    const color = HTMLAttributes["data-highlight-color"] || "yellow";
    return [
      "mark",
      mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, {
        "data-highlight-color": color,
        class: `highlight-mark highlight-${color}`,
      }),
      0,
    ];
  },

  addCommands() {
    return {
      setColoredHighlight:
        (color: HighlightColor = "yellow") =>
        ({ commands }) => {
          return commands.setMark(this.name, { color });
        },
      toggleColoredHighlight:
        (color: HighlightColor = "yellow") =>
        ({ commands }) => {
          return commands.toggleMark(this.name, { color });
        },
      unsetColoredHighlight:
        () =>
        ({ commands }) => {
          return commands.unsetMark(this.name);
        },
    };
  },

  addKeyboardShortcuts() {
    return {
      "Mod-Shift-h": () => this.editor.commands.toggleColoredHighlight("yellow"),
    };
  },

  addProseMirrorPlugins() {
    return [
      coloredHighlightInputRule(this.type),
      coloredHighlightPasteRule(this.type),
    ];
  },

  // Custom tokenizer for parsing =={color}text== or ==text== from markdown
  markdownTokenizer: {
    name: "coloredHighlight",
    level: "inline",
    start: "==",
    // @ts-expect-error - Custom tokenizer signature for @tiptap/markdown
    tokenize(
      src: string,
      _tokens: unknown[],
      helper: { inlineTokens: (src: string) => unknown[] }
    ) {
      // Match =={color}content== or ==content== where content doesn't contain ==
      const match = /^==(?:\{(\w+)\})?([^=]+)==/.exec(src);
      if (!match) {
        return undefined;
      }

      const colorMatch = match[1];
      let color: HighlightColor = "yellow";
      if (colorMatch && HIGHLIGHT_COLORS.includes(colorMatch as HighlightColor)) {
        color = colorMatch as HighlightColor;
      }

      return {
        type: "coloredHighlight",
        raw: match[0],
        color,
        tokens: helper.inlineTokens(match[2]),
      };
    },
  },

  // Parse the coloredHighlight token into a mark result
  // @ts-expect-error - parseMarkdown is a custom field for @tiptap/markdown
  parseMarkdown(
    token: { tokens?: unknown[]; color?: HighlightColor },
    helpers: {
      parseInline: (tokens: unknown[]) => unknown[];
      applyMark: (
        markType: string,
        content: unknown[],
        attrs?: unknown
      ) => unknown;
    }
  ) {
    const content = token.tokens ? helpers.parseInline(token.tokens) : [];
    return helpers.applyMark("coloredHighlight", content, {
      color: token.color || "yellow",
    });
  },
}).extend({
  // Render highlight marks to ==content== or =={color}content== in markdown
  // Using extend() to ensure the property is available to @tiptap/markdown
  // @ts-expect-error - Custom renderMarkdown signature for @tiptap/markdown
  renderMarkdown(
    node: { attrs?: { color?: string } },
    helpers: { renderChildren: (node: unknown) => string }
  ) {
    const content = helpers.renderChildren(node);
    const color = node.attrs?.color || "yellow";

    // Only emit {color} if not the default yellow
    if (color && color !== "yellow") {
      return `=={${color}}${content}==`;
    }
    return `==${content}==`;
  },
});
