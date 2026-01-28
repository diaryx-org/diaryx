/**
 * TipTap Mark extension for Discord-style spoiler syntax.
 *
 * Renders ||hidden text|| as a spoiler that can be clicked to reveal.
 * Supports:
 * - Input rule: typing ||text|| converts to spoiler (when enabled)
 * - Paste rule: pasted ||text|| converts to spoiler (when enabled)
 * - Keyboard shortcut: Mod-Shift-S to toggle (when enabled)
 * - Click to reveal/hide behavior (when enabled)
 * - Markdown serialization/parsing via ||content|| (always)
 *
 * When `enabled: false`, the extension still parses and serializes ||text||
 * but displays it as visible text with || characters shown.
 */

import { Mark, mergeAttributes } from "@tiptap/core";
import { markInputRule, markPasteRule } from "@tiptap/core";
import { Plugin as ProseMirrorPlugin, PluginKey } from "@tiptap/pm/state";

// Regex patterns for input/paste rules
// Matches ||text|| but not ||| (three or more pipes) or empty ||
const inputRegex = /\|\|([^|]+)\|\|$/;
const pasteRegex = /\|\|([^|]+)\|\|/g;

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    spoiler: {
      /** Set the spoiler mark */
      setSpoiler: () => ReturnType;
      /** Toggle the spoiler mark */
      toggleSpoiler: () => ReturnType;
      /** Unset the spoiler mark */
      unsetSpoiler: () => ReturnType;
    };
  }
}

export interface SpoilerOptions {
  HTMLAttributes: Record<string, unknown>;
  /** When false, spoilers display as ||text|| visibly instead of hiding */
  enabled: boolean;
}

export const SpoilerMark = Mark.create<SpoilerOptions>({
  name: "spoiler",

  addOptions() {
    return {
      HTMLAttributes: {},
      enabled: true,
    };
  },

  parseHTML() {
    return [
      {
        tag: "span[data-spoiler]",
      },
    ];
  },

  renderHTML({ HTMLAttributes }) {
    // When disabled, show the text with || visible using CSS ::before/::after
    if (!this.options.enabled) {
      return [
        "span",
        mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, {
          "data-spoiler": "",
          "data-spoiler-disabled": "",
          class: "spoiler-mark spoiler-disabled",
        }),
        0,
      ];
    }

    return [
      "span",
      mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, {
        "data-spoiler": "",
        class: "spoiler-mark spoiler-hidden",
      }),
      0,
    ];
  },

  addCommands() {
    return {
      setSpoiler:
        () =>
        ({ commands }) => {
          return commands.setMark(this.name);
        },
      toggleSpoiler:
        () =>
        ({ commands }) => {
          return commands.toggleMark(this.name);
        },
      unsetSpoiler:
        () =>
        ({ commands }) => {
          return commands.unsetMark(this.name);
        },
    };
  },

  addKeyboardShortcuts() {
    // Disable shortcuts when spoilers are disabled
    if (!this.options.enabled) {
      return {} as Record<string, () => boolean>;
    }
    return {
      "Mod-Shift-s": () => this.editor.commands.toggleSpoiler(),
    };
  },

  addInputRules() {
    // Disable input rules when spoilers are disabled
    if (!this.options.enabled) {
      return [];
    }
    return [
      markInputRule({
        find: inputRegex,
        type: this.type,
      }),
    ];
  },

  addPasteRules() {
    // Disable paste rules when spoilers are disabled
    if (!this.options.enabled) {
      return [];
    }
    return [
      markPasteRule({
        find: pasteRegex,
        type: this.type,
      }),
    ];
  },

  addProseMirrorPlugins() {
    // Disable click handling when spoilers are disabled
    if (!this.options.enabled) {
      return [];
    }
    return [
      new ProseMirrorPlugin({
        key: new PluginKey("spoilerClick"),
        props: {
          handleClick: (view, _pos, event) => {
            const target = event.target as HTMLElement;
            const spoilerEl = target.closest("[data-spoiler]") as HTMLElement | null;

            if (spoilerEl) {
              // Toggle reveal state
              if (spoilerEl.classList.contains("spoiler-hidden")) {
                spoilerEl.classList.remove("spoiler-hidden");
                spoilerEl.classList.add("spoiler-revealed");
              } else {
                spoilerEl.classList.remove("spoiler-revealed");
                spoilerEl.classList.add("spoiler-hidden");
              }
              return true;
            }

            // Click elsewhere: hide all revealed spoilers
            const editorDom = view.dom;
            const revealedSpoilers = editorDom.querySelectorAll(".spoiler-revealed");
            revealedSpoilers.forEach((el) => {
              el.classList.remove("spoiler-revealed");
              el.classList.add("spoiler-hidden");
            });

            return false;
          },
        },
      }),
    ];
  },

  // Custom tokenizer for parsing ||text|| from markdown
  // @ts-ignore - markdownTokenizer is a custom field for @tiptap/markdown
  markdownTokenizer: {
    name: "spoiler",
    level: "inline",
    start: "||",
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    tokenize(src: string, _tokens: any[], helper: any) {
      // Match ||content|| where content doesn't contain ||
      const match = /^\|\|([^|]+)\|\|/.exec(src);
      if (!match) {
        return undefined;
      }
      return {
        type: "spoiler",
        raw: match[0],
        tokens: helper.inlineTokens(match[1]),
      };
    },
  },

  // Parse the spoiler token into a mark result
  // @ts-expect-error - parseMarkdown is a custom field for @tiptap/markdown
  parseMarkdown(token: { tokens?: unknown[] }, helpers: { parseInline: (tokens: unknown[]) => unknown[]; applyMark: (markType: string, content: unknown[], attrs?: unknown) => unknown }) {
    const content = token.tokens ? helpers.parseInline(token.tokens) : [];
    return helpers.applyMark("spoiler", content);
  },
}).extend({
  // Render spoiler marks to ||content|| in markdown
  // Using extend() to ensure the property is available to @tiptap/markdown
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  renderMarkdown(node: any, helpers: any) {
    const content = helpers.renderChildren(node);
    return `||${content}||`;
  },
});
