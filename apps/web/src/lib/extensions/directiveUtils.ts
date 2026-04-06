/**
 * Generic markdown directive utilities.
 *
 * Provides factory functions for creating TipTap-compatible tokenizers and
 * renderers for the three standard directive forms:
 *
 *   Inline:  :name[content]{attr1 attr2}
 *   Leaf:    ::name[content]{attr1 attr2}
 *   Block:   :::name{attr1 attr2}\ncontent\n:::
 *
 * These are designed to be reusable by any extension that needs directive
 * syntax. The `vis` (visibility/audience) directive is the first consumer,
 * but the utilities are name-agnostic.
 */

// ---------------------------------------------------------------------------
// Attribute helpers
// ---------------------------------------------------------------------------

/**
 * Parse a space-separated attribute string into an array of values.
 * Handles extra whitespace gracefully.
 *
 *   "family friends"  → ["family", "friends"]
 *   " family "        → ["family"]
 *   ""                → []
 */
export function parseDirectiveAttrs(raw: string): string[] {
  return raw
    .trim()
    .split(/\s+/)
    .filter((s) => s.length > 0);
}

/**
 * Serialize an array of attribute values back to a space-separated string.
 *
 *   ["family", "friends"] → "family friends"
 */
export function serializeDirectiveAttrs(attrs: string[]): string {
  return attrs.join(" ");
}

// ---------------------------------------------------------------------------
// Inline directive tokenizer factory
// ---------------------------------------------------------------------------

/**
 * Create a marked.js tokenizer for inline directives: `:name[content]{attrs}`
 *
 * The returned tokenizer recognizes the pattern at the inline level and
 * produces tokens with parsed `attrs` and nested inline `tokens` for the
 * content between brackets.
 *
 * Usage in a TipTap extension:
 * ```ts
 * markdownTokenizer: createInlineDirectiveTokenizer("vis", "visibilityMark"),
 * ```
 *
 * @param directiveName  The directive name in markdown (e.g. "vis")
 * @param extensionName  The TipTap extension name. The token `type` MUST match
 *                       this so `@tiptap/markdown` routes the token to the
 *                       correct extension's `parseMarkdown` handler.
 */
export function createInlineDirectiveTokenizer(
  directiveName: string,
  extensionName: string,
) {
  const prefix = `:${directiveName}[`;

  return {
    name: extensionName,
    level: "inline" as const,
    start: prefix,
    tokenize(
      src: string,
      _tokens: unknown[],
      helper: { inlineTokens: (src: string) => unknown[] },
    ) {
      if (!src.startsWith(prefix)) return undefined;

      // Manual bracket-depth parse to find the matching ] for the
      // opening [. This handles content containing balanced brackets,
      // e.g. `:vis[text with [link](url) and `code`]{audience}`.
      let depth = 0;
      let contentEnd = -1;
      for (let i = prefix.length - 1; i < src.length; i++) {
        const ch = src[i];
        if (ch === "\\") {
          i++; // skip escaped character
          continue;
        }
        if (ch === "[") {
          depth++;
        } else if (ch === "]") {
          depth--;
          if (depth === 0) {
            contentEnd = i;
            break;
          }
        }
      }

      if (contentEnd === -1) return undefined;

      // After the ], expect {attrs}
      const afterContent = src.slice(contentEnd + 1);
      const attrsMatch = /^\{([^}]*)\}/.exec(afterContent);
      if (!attrsMatch) return undefined;

      const content = src.slice(prefix.length, contentEnd);
      const raw = src.slice(0, contentEnd + 1 + attrsMatch[0].length);

      return {
        type: extensionName,
        raw,
        directiveContent: content,
        attrs: parseDirectiveAttrs(attrsMatch[1]),
        // Parse the content for nested inline marks (bold, italic, etc.)
        tokens: helper.inlineTokens(content),
      };
    },
  };
}

// ---------------------------------------------------------------------------
// Block directive tokenizer factory
// ---------------------------------------------------------------------------

/**
 * Create a marked.js tokenizer for block directives:
 *
 *   Opening: `:::name{attrs}`
 *   Closing: `:::`
 *
 * Each is emitted as a separate block-level token. The pairing of open/close
 * markers happens in the extension's decoration plugin, not in the tokenizer.
 *
 * Usage in a TipTap extension:
 * ```ts
 * markdownTokenizer: createBlockDirectiveTokenizer("vis", "visBlockMarker"),
 * ```
 *
 * @param directiveName  The directive name in markdown (e.g. "vis")
 * @param extensionName  The TipTap extension name. The token `type` MUST match
 *                       this so `@tiptap/markdown` routes the token to the
 *                       correct extension's `parseMarkdown` handler.
 */
export function createBlockDirectiveTokenizer(
  directiveName: string,
  extensionName: string,
) {
  const escapedName = directiveName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

  // Opening pattern: :::name{attrs} (with optional trailing whitespace/newline)
  const openPattern = new RegExp(
    `^:::${escapedName}\\{([^}]*)\\}\\s*\\n?`,
  );

  // Closing pattern: ::: on its own line (with optional trailing whitespace/newline)
  // Must not be followed by a word char (which would be another directive opening)
  const closePattern = /^:::(?!\w)\s*\n?/;

  return {
    name: extensionName,
    level: "block" as const,
    start(src: string) {
      // Quick check: find the first ::: that could be ours
      const openMatch = src.match(new RegExp(`^:::${escapedName}\\{`, "m"));
      const closeMatch = src.match(/^:::(?!\w)/m);
      const openIdx = openMatch?.index ?? -1;
      const closeIdx = closeMatch?.index ?? -1;

      if (openIdx === -1 && closeIdx === -1) return -1;
      if (openIdx === -1) return closeIdx;
      if (closeIdx === -1) return openIdx;
      return Math.min(openIdx, closeIdx);
    },
    tokenize(src: string) {
      // Try opening first
      let match = openPattern.exec(src);
      if (match) {
        return {
          type: extensionName,
          raw: match[0],
          variant: "open" as const,
          attrs: parseDirectiveAttrs(match[1]),
        };
      }

      // Try closing
      match = closePattern.exec(src);
      if (match) {
        return {
          type: extensionName,
          raw: match[0],
          variant: "close" as const,
          attrs: [] as string[],
        };
      }

      return undefined;
    },
  };
}

// ---------------------------------------------------------------------------
// Render helpers
// ---------------------------------------------------------------------------

/**
 * Render an inline directive back to markdown.
 *
 *   renderInlineDirective("vis", "some text", ["family", "friends"])
 *   → ":vis[some text]{family friends}"
 */
export function renderInlineDirective(
  name: string,
  content: string,
  attrs: string[],
): string {
  return `:${name}[${content}]{${serializeDirectiveAttrs(attrs)}}`;
}

/**
 * Render a block directive marker back to markdown.
 *
 *   renderBlockDirectiveOpen("vis", ["family"]) → ":::vis{family}\n"
 *   renderBlockDirectiveClose()                 → ":::\n"
 */
export function renderBlockDirectiveOpen(
  name: string,
  attrs: string[],
): string {
  return `:::${name}{${serializeDirectiveAttrs(attrs)}}\n`;
}

export function renderBlockDirectiveClose(): string {
  return ":::\n";
}
