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
import type { MarkType } from "@tiptap/pm/model";
import { TemplateVariable } from "$lib/extensions/TemplateVariable";
import { ConditionalBlock } from "$lib/extensions/ConditionalBlock";
import type { Api } from "$lib/backend/api";
import {
  trackBlobUrl,
  getBlobUrl,
  queueResolveAttachment,
  formatMarkdownDestination,
} from "@/models/services/attachmentService";
import {
  enqueueIncrementalAttachmentUpload,
} from "@/controllers/attachmentController";

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

export interface MarkAttributeManifest {
  name: string;
  default: string;
  html_attribute: string;
  valid_values?: string[];
  css_class_prefix?: string | null;
}

export interface MarkdownAttributeSyntaxManifest {
  attribute: string;
  open: string;
  close: string;
  position?: string; // default "after_open"
}

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
    attribute_syntax?: MarkdownAttributeSyntaxManifest | null;
    single_line?: boolean;
  };
  render_export: string | null;
  edit_mode: "Popover" | "SourceToggle" | "Iframe" | null;
  iframe_component_id?: string | null;
  css: string | null;
  insert_command?: {
    label: string;
    icon?: string | null;
    description?: string | null;
  } | null;
  host_capabilities?: string[] | null;
  keyboard_shortcut?: string | null;
  click_behavior?: {
    ToggleClass: {
      hidden_class: string;
      revealed_class: string;
    };
  } | null;
  html_tag?: string | null;
  base_css_class?: string | null;
  attributes?: MarkAttributeManifest[] | null;
  toolbar?: {
    icon: string;
    label: string;
  } | null;
}

export interface EditorExtensionFactoryOptions {
  preserveOnly?: boolean;
  pluginName?: string;
}

/**
 * Per-plugin context for iframe-based editor extensions.
 * Captured at extension creation time (pluginId and getComponentHtml don't
 * change per entry).
 */
export interface EditorExtensionContext {
  pluginId: string;
  getComponentHtml: (componentId: string) => Promise<string | null>;
}

/**
 * Global context for iframe editor extensions that changes per entry.
 * Set via `setEditorExtensionIframeContext()` and read at render time by
 * iframe node views (late binding avoids stale closures in cached extensions).
 */
export interface EditorExtensionIframeContext {
  entryPath: string;
  api: Api | null;
}

let _iframeCtx: EditorExtensionIframeContext | null = null;

/** Set the global iframe context. Called by the host before editor creation. */
export function setEditorExtensionIframeContext(
  ctx: EditorExtensionIframeContext | null,
): void {
  _iframeCtx = ctx;
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
  renderFn: RenderFn | null,
  options: EditorExtensionFactoryOptions = {},
  context?: EditorExtensionContext,
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
      if (ext.edit_mode === "Iframe" && ext.iframe_component_id && context) {
        return createIframeBlockView(ext, context);
      }
      if (isInline) {
        return createInlineAtomView(ext, renderFn!);
      }
      return createBlockAtomView(ext, renderFn!);
    },

    // Markdown tokenizer — generated from open/close delimiters
    // @ts-ignore - custom field for @tiptap/markdown
    markdownTokenizer: {
      name: ext.extension_id,
      level: ext.markdown.level === "Inline" ? "inline" : "block",
      start: ext.markdown.open,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      tokenize(src: string, _tokens: any[]) {
        if (isBlock && ext.markdown.single_line) {
          // Single-line block: match open...close on one line (no newlines between delimiters)
          const re = new RegExp(
            `^${openEsc}(.+?)${closeEsc}(?:\\n|$)`,
          );
          const match = re.exec(src);
          if (!match) return undefined;
          return {
            type: ext.extension_id,
            raw: match[0],
            source: match[1],
          };
        } else if (isBlock) {
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
      if (isBlock && ext.markdown.single_line) {
        return `${ext.markdown.open}${source}${ext.markdown.close}\n`;
      }
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

  // Attribute support
  const hasAttrs = !!ext.attributes?.length;
  const attrSyntax = ext.markdown.attribute_syntax ?? null;
  const attr = hasAttrs && attrSyntax
    ? ext.attributes!.find(a => a.name === attrSyntax.attribute) ?? null
    : null;
  const attrDefaults = hasAttrs
    ? Object.fromEntries(ext.attributes!.map(a => [a.name, a.default]))
    : undefined;

  // Input rule: matches ||text|| at end of input (only for simple marks)
  const inputRegex = new RegExp(`${openEsc}([^${escapeRegex(ext.markdown.open[0])}]+)${closeEsc}$`);
  // Paste rule: matches ||text|| globally (only for simple marks)
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

  // Click behavior ProseMirror plugins (shared between simple and attribute marks)
  const clickBehaviorPlugins: ProseMirrorPlugin[] = [];
  if (!preserveOnly && clickBehavior) {
    const dataAttr = `data-${ext.extension_id}`;
    clickBehaviorPlugins.push(
      new ProseMirrorPlugin({
        key: new PluginKey(`${ext.extension_id}Click`),
        props: {
          handleDOMEvents: {
            mousedown: (view, event) => {
              const markEl = findClosestElement(event.target, `[${dataAttr}]`);
              if (markEl) {
                event.preventDefault();
                if (markEl.classList.contains(hiddenClass)) {
                  markEl.classList.remove(hiddenClass);
                  markEl.classList.add(revealedClass);
                } else {
                  markEl.classList.remove(revealedClass);
                  markEl.classList.add(hiddenClass);
                }
                return true;
              }
              const editorDom = view.dom;
              const revealed = editorDom.querySelectorAll(`.${revealedClass}`);
              revealed.forEach((el) => {
                el.classList.remove(revealedClass);
                el.classList.add(hiddenClass);
              });
              return false;
            },
          },
        },
      }),
    );
  }

  return Mark.create({
    name: ext.extension_id,

    addAttributes() {
      if (!hasAttrs) return {};
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const attrs: Record<string, any> = {};
      for (const a of ext.attributes!) {
        attrs[a.name] = {
          default: a.default,
          parseHTML: (el: HTMLElement) => el.getAttribute(a.html_attribute) || a.default,
          renderHTML: (attributes: Record<string, string>) => ({
            [a.html_attribute]: attributes[a.name],
          }),
        };
      }
      return attrs;
    },

    parseHTML() {
      const tag = ext.html_tag || "span";
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const results: any[] = [];
      if (hasAttrs) {
        const firstAttr = ext.attributes![0];
        results.push({ tag: `${tag}[${firstAttr.html_attribute}]` });
        results.push({
          tag,
          getAttrs: () => Object.fromEntries(
            ext.attributes!.map(a => [a.name, a.default]),
          ),
        });
      } else {
        results.push({ tag: `${tag}[data-${ext.extension_id}]` });
      }
      return results;
    },

    renderHTML({ HTMLAttributes }) {
      const tag = ext.html_tag || "span";
      const classNames: string[] = [];
      if (ext.base_css_class) classNames.push(ext.base_css_class);
      if (preserveOnly) {
        classNames.push("plugin-missing-mark");
      } else if (hasAttrs) {
        for (const a of ext.attributes!) {
          if (a.css_class_prefix) {
            classNames.push(`${a.css_class_prefix}${HTMLAttributes[a.html_attribute] || a.default}`);
          }
        }
      } else {
        classNames.push(`${ext.extension_id}-mark`, hiddenClass);
      }

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const merged: Record<string, any> = {};
      if (!hasAttrs) merged[`data-${ext.extension_id}`] = "";
      if (classNames.length) merged.class = classNames.join(" ");

      return [tag, mergeAttributes(HTMLAttributes, merged), 0];
    },

    addCommands() {
      if (preserveOnly) {
        return {};
      }
      return {
        [`set${pascalName}`]:
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          (attrs?: Record<string, string>) =>
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          ({ commands }: any) => {
            return commands.setMark(ext.extension_id, attrs ?? attrDefaults);
          },
        [`toggle${pascalName}`]:
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          (attrs?: Record<string, string>) =>
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          ({ commands }: any) => {
            return commands.toggleMark(ext.extension_id, attrs ?? attrDefaults);
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
        [ext.keyboard_shortcut]: () => (this as any).editor.commands[`toggle${pascalName}`](attrDefaults),
      };
    },

    addInputRules() {
      if (preserveOnly || (attrSyntax && attr)) return [];
      return [
        markInputRule({
          find: inputRegex,
          type: this.type,
        }),
      ];
    },

    addPasteRules() {
      if (preserveOnly || (attrSyntax && attr)) return [];
      return [
        markPasteRule({
          find: pasteRegex,
          type: this.type,
        }),
      ];
    },

    addProseMirrorPlugins() {
      const plugins = [...clickBehaviorPlugins];

      if (!preserveOnly && attrSyntax && attr) {
        plugins.push(createAttributeInputRule(ext, attr, attrSyntax, this.type));
        plugins.push(createAttributePasteRule(ext, attr, attrSyntax, this.type));
      }

      return plugins;
    },

    // Custom tokenizer for parsing delimited text from markdown
    // @ts-ignore - markdownTokenizer is a custom field for @tiptap/markdown
    markdownTokenizer: {
      name: ext.extension_id,
      level: "inline",
      start: ext.markdown.open,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      tokenize(src: string, _tokens: any[], helper: any) {
        if (attrSyntax && attr) {
          const attrOpenEsc = escapeRegex(attrSyntax.open);
          const attrCloseEsc = escapeRegex(attrSyntax.close);
          const re = new RegExp(
            `^${openEsc}(?:${attrOpenEsc}(\\w+)${attrCloseEsc})?([^${escapeRegex(ext.markdown.open[0])}]+)${closeEsc}`,
          );
          const match = re.exec(src);
          if (!match) return undefined;

          let attrValue = match[1] || attr.default;
          if (attr.valid_values?.length && !attr.valid_values.includes(attrValue)) {
            attrValue = attr.default;
          }

          return {
            type: ext.extension_id,
            raw: match[0],
            [attr.name]: attrValue,
            tokens: helper.inlineTokens(match[2]),
          };
        }

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
      const markAttrs = hasAttrs
        ? Object.fromEntries(ext.attributes!.map(a => [a.name, token[a.name] || a.default]))
        : undefined;
      return helpers.applyMark(ext.extension_id, content, markAttrs);
    },
  }).extend({
    // Render mark → markdown
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    renderMarkdown(node: any, helpers: any) {
      const content = helpers.renderChildren(node);
      if (attrSyntax && attr) {
        const value = node.attrs?.[attr.name] || attr.default;
        if (value !== attr.default) {
          return `${ext.markdown.open}${attrSyntax.open}${value}${attrSyntax.close}${content}${ext.markdown.close}`;
        }
      }
      return `${ext.markdown.open}${content}${ext.markdown.close}`;
    },
  });
}

// ============================================================================
// Attribute input/paste rule helpers
// ============================================================================

/**
 * ProseMirror plugin that handles attribute-aware input rules.
 * Generalizes the pattern from ColoredHighlightMark: triggers on typing the
 * last char of the closing delimiter, looks backwards for the full pattern,
 * extracts attribute value, and applies the mark.
 */
function createAttributeInputRule(
  ext: EditorExtensionManifest,
  attr: MarkAttributeManifest,
  attrSyntax: MarkdownAttributeSyntaxManifest,
  markType: MarkType,
): ProseMirrorPlugin {
  const lastChar = ext.markdown.close[ext.markdown.close.length - 1];
  const openEsc = escapeRegex(ext.markdown.open);
  const attrOpenEsc = escapeRegex(attrSyntax.open);
  const attrCloseEsc = escapeRegex(attrSyntax.close);

  // Pattern to match before the final character: ==(?:{(\w+)})?(.+?)=$ (when close is ==)
  // We need to match everything except the last char of the close delimiter
  const closeWithoutLast = ext.markdown.close.slice(0, -1);
  const closeWithoutLastEsc = closeWithoutLast ? escapeRegex(closeWithoutLast) : "";
  const partialPattern = new RegExp(
    `${openEsc}(?:${attrOpenEsc}(\\w+)${attrCloseEsc})?([^${escapeRegex(ext.markdown.open[0])}]+)${closeWithoutLastEsc}$`,
  );

  return new ProseMirrorPlugin({
    key: new PluginKey(`${ext.extension_id}AttrInputRule`),
    props: {
      handleTextInput(view, from, to, text) {
        if (text !== lastChar) return false;

        const { state } = view;
        const $from = state.doc.resolve(from);

        const textBefore = $from.parent.textBetween(
          Math.max(0, $from.parentOffset - 100),
          $from.parentOffset,
          undefined,
          "\ufffc",
        );

        const partialMatch = partialPattern.exec(textBefore);
        if (!partialMatch) return false;

        const colorMatch = partialMatch[1];
        const content = partialMatch[2];

        let attrValue = attr.default;
        if (colorMatch) {
          if (attr.valid_values?.length && !attr.valid_values.includes(colorMatch)) {
            return false;
          }
          attrValue = colorMatch;
        }

        const fullMatchLength = partialMatch[0].length + 1;
        const matchStart = from - (fullMatchLength - 1);

        const tr = state.tr;
        tr.delete(matchStart, to);
        const mark = markType.create({ [attr.name]: attrValue });
        tr.insert(matchStart, state.schema.text(content, [mark]));
        view.dispatch(tr);
        return true;
      },
    },
  });
}

/**
 * ProseMirror plugin that handles attribute-aware paste rules.
 * Transforms pasted content to apply marks with extracted attributes.
 */
function createAttributePasteRule(
  ext: EditorExtensionManifest,
  attr: MarkAttributeManifest,
  attrSyntax: MarkdownAttributeSyntaxManifest,
  markType: MarkType,
): ProseMirrorPlugin {
  const openEsc = escapeRegex(ext.markdown.open);
  const closeEsc = escapeRegex(ext.markdown.close);
  const attrOpenEsc = escapeRegex(attrSyntax.open);
  const attrCloseEsc = escapeRegex(attrSyntax.close);

  const pasteRegex = new RegExp(
    `${openEsc}(?:${attrOpenEsc}(\\w+)${attrCloseEsc})?([^${escapeRegex(ext.markdown.open[0])}]+)${closeEsc}`,
    "g",
  );

  return new ProseMirrorPlugin({
    key: new PluginKey(`${ext.extension_id}AttrPasteRule`),
    props: {
      transformPasted(slice) {
        const { content } = slice;
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const newContent: any[] = [];

        content.forEach((node) => {
          if (node.isText && node.text) {
            const text = node.text;
            let lastIndex = 0;
            let match;
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            const fragments: { text: string; marks: any }[] = [];

            pasteRegex.lastIndex = 0;

            while ((match = pasteRegex.exec(text)) !== null) {
              if (match.index > lastIndex) {
                fragments.push({
                  text: text.slice(lastIndex, match.index),
                  marks: node.marks,
                });
              }

              const colorMatch = match[1];
              let attrValue = attr.default;
              if (colorMatch && (!attr.valid_values?.length || attr.valid_values.includes(colorMatch))) {
                attrValue = colorMatch;
              }

              const highlightMark = markType.create({ [attr.name]: attrValue });
              fragments.push({
                text: match[2],
                marks: [...node.marks, highlightMark],
              });

              lastIndex = match.index + match[0].length;
            }

            if (lastIndex < text.length) {
              fragments.push({
                text: text.slice(lastIndex),
                marks: node.marks,
              });
            }

            if (fragments.length > 0) {
              fragments.forEach((frag) => {
                if (frag.text) {
                  newContent.push(node.type.schema.text(frag.text, frag.marks));
                }
              });
            } else {
              newContent.push(node);
            }
          } else {
            newContent.push(node);
          }
        });

        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const SliceCtor = slice.constructor as any;
        const FragCtor = slice.content.constructor as any;
        return new SliceCtor(FragCtor.from(newContent), slice.openStart, slice.openEnd);
      },
    },
  });
}

// ============================================================================
// Helpers
// ============================================================================

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function findClosestElement(
  target: EventTarget | null,
  selector: string,
): HTMLElement | null {
  if (target instanceof HTMLElement) {
    return target.closest(selector) as HTMLElement | null;
  }

  if (target instanceof Text) {
    return target.parentElement?.closest(selector) as HTMLElement | null;
  }

  return null;
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

// ============================================================================
// Iframe block node view (vanilla DOM — no Svelte dependency)
// ============================================================================

/** CSS variable names forwarded to iframe for theming. */
const IFRAME_CSS_VAR_NAMES = [
  "--background", "--foreground", "--card", "--card-foreground",
  "--popover", "--popover-foreground", "--primary", "--primary-foreground",
  "--secondary", "--secondary-foreground", "--muted", "--muted-foreground",
  "--accent", "--accent-foreground", "--destructive",
  "--border", "--input", "--ring", "--radius",
];

function collectIframeCssVars(): Record<string, string> {
  const computed = getComputedStyle(document.documentElement);
  const vars: Record<string, string> = {};
  for (const name of IFRAME_CSS_VAR_NAMES) {
    const value = computed.getPropertyValue(name).trim();
    if (value) vars[name] = value;
  }
  return vars;
}

/** Parse the compound `alt](path` source into separate alt and src. */
function parseDrawingSource(source: string): { alt: string; src: string } {
  const bracketIdx = source.indexOf("](");
  if (bracketIdx >= 0) {
    let src = source.slice(bracketIdx + 2);
    // Strip angle brackets (paths with spaces)
    if (src.startsWith("<") && src.endsWith(">")) {
      src = src.slice(1, -1);
    }
    return { alt: source.slice(0, bracketIdx), src };
  }
  return { alt: "", src: source };
}

let iframeCssInjected = false;
function injectIframeNodeViewCss() {
  if (iframeCssInjected) return;
  iframeCssInjected = true;
  const style = document.createElement("style");
  style.setAttribute("data-iframe-node-views", "");
  style.textContent = `
    .iframe-block-preview { position: relative; display: inline-block; max-width: 100%; border-radius: 6px; overflow: hidden; }
    .iframe-block-preview img { display: block; max-width: 100%; height: auto; border-radius: 6px; }
    .iframe-block-edit-btn {
      position: absolute; top: 8px; right: 8px;
      display: flex; align-items: center; gap: 4px;
      padding: 4px 10px; border: none; border-radius: 6px;
      background: var(--popover); color: var(--foreground);
      font-size: 12px; cursor: pointer; opacity: 0;
      transition: opacity 0.15s ease;
      box-shadow: 0 1px 3px rgba(0,0,0,0.12), 0 1px 2px rgba(0,0,0,0.06);
    }
    .iframe-block-preview:hover .iframe-block-edit-btn { opacity: 1; }
    .iframe-block-edit-btn:hover { background: var(--accent); color: var(--accent-foreground); }
    .iframe-block-iframe { width: 100%; border: none; min-height: 360px; display: block; }
    .iframe-block-preview audio { display: block; max-width: 100%; border-radius: 6px; margin: 4px 0; }
  `;
  document.head.appendChild(style);
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function createIframeBlockView(ext: EditorExtensionManifest, context: EditorExtensionContext): any {
  const blockLabel = ext.insert_command?.label || ext.extension_id;
  const componentId = ext.iframe_component_id!;

  injectIframeNodeViewCss();

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return ({ node, getPos, editor }: { node: any; getPos: () => number | undefined; editor: any }) => {
    const dom = document.createElement("div");
    dom.className = "atom-block-container";
    dom.setAttribute("contenteditable", "false");

    let currentSource = (node.attrs.source as string) || "";
    let editingIframe = false;
    let iframeEl: HTMLIFrameElement | null = null;
    let iframeBlobUrl: string | null = null;

    // Audio capture state (host-mediated)
    const hostCapabilities: string[] = ext.host_capabilities ?? [];
    const hasAudioCapture = hostCapabilities.includes("audio_capture");
    let mediaRecorder: MediaRecorder | null = null;
    let audioContext: AudioContext | null = null;
    let analyser: AnalyserNode | null = null;
    let waveformAnimId: number | null = null;
    let audioStream: MediaStream | null = null;

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

    const editBtn = document.createElement("button");
    editBtn.type = "button";
    editBtn.className = "atom-block-toggle";
    editBtn.textContent = "Edit";
    editBtn.title = `Edit ${blockLabel.toLowerCase()}`;

    header.append(label, editBtn);

    // Content area
    const content = document.createElement("div");
    dom.append(header, content);

    function cleanupAudioCapture() {
      if (waveformAnimId != null) {
        cancelAnimationFrame(waveformAnimId);
        waveformAnimId = null;
      }
      if (mediaRecorder && mediaRecorder.state !== "inactive") {
        try { mediaRecorder.stop(); } catch { /* ignore */ }
      }
      mediaRecorder = null;
      if (analyser) { analyser.disconnect(); analyser = null; }
      if (audioContext) {
        try { audioContext.close(); } catch { /* ignore */ }
        audioContext = null;
      }
      if (audioStream) {
        audioStream.getTracks().forEach(t => t.stop());
        audioStream = null;
      }
    }

    function cleanupIframe() {
      cleanupAudioCapture();
      if (iframeBlobUrl) {
        URL.revokeObjectURL(iframeBlobUrl);
        iframeBlobUrl = null;
      }
      iframeEl = null;
    }

    // ---- Preview mode ----
    const AUDIO_EXTS = [".webm", ".mp3", ".ogg", ".wav", ".m4a", ".aac"];

    function getFileExt(path: string): string {
      const dot = path.lastIndexOf(".");
      return dot >= 0 ? path.slice(dot).toLowerCase() : "";
    }

    function isAudioFile(path: string): boolean {
      return AUDIO_EXTS.includes(getFileExt(path));
    }

    function renderPreview(source: string) {
      editingIframe = false;
      cleanupIframe();
      content.textContent = "";
      content.className = "atom-block-preview";
      editBtn.textContent = "Edit";
      editBtn.title = `Edit ${blockLabel.toLowerCase()}`;

      const { alt, src } = parseDrawingSource(source);

      if (!src.trim()) {
        content.className = "atom-block-preview";
        const empty = document.createElement("span");
        empty.className = "atom-block-empty";
        empty.textContent = `Empty ${blockLabel.toLowerCase()} block`;
        content.appendChild(empty);
        return;
      }

      // Wrap in a preview container for hover-edit button
      const previewWrap = document.createElement("div");
      previewWrap.className = "iframe-block-preview";

      const isAudio = isAudioFile(src);

      function resolveUrl(callback: (url: string) => void) {
        const isBlobUrl = src.startsWith("blob:");
        if (isBlobUrl) {
          callback(src);
        } else {
          const cached = getBlobUrl(src);
          if (cached) {
            callback(cached);
          } else {
            const api = _iframeCtx?.api;
            const entryPath = _iframeCtx?.entryPath ?? "";
            if (api) {
              queueResolveAttachment(api, entryPath, src).then((resolved) => {
                if (resolved) callback(resolved);
              });
            }
          }
        }
      }

      if (isAudio) {
        const audioPreview = document.createElement("audio");
        audioPreview.controls = true;
        audioPreview.preload = "metadata";
        resolveUrl((url) => { audioPreview.src = url; });
        previewWrap.appendChild(audioPreview);
      } else {
        const img = document.createElement("img");
        img.alt = alt || blockLabel;
        img.draggable = false;
        resolveUrl((url) => { img.src = url; });
        previewWrap.appendChild(img);
      }

      if (editor.isEditable) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "iframe-block-edit-btn";
        btn.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/></svg> Edit`;
        btn.addEventListener("click", (e) => {
          e.stopPropagation();
          startEditing();
        });
        previewWrap.appendChild(btn);
      }

      content.appendChild(previewWrap);
    }

    // ---- Edit mode ----
    async function startEditing() {
      if (!editor.isEditable || editingIframe) return;
      editingIframe = true;
      content.textContent = "";
      content.className = "";
      editBtn.textContent = "Cancel";
      editBtn.title = "Cancel editing";

      // Show loading
      const loadingMsg = document.createElement("div");
      loadingMsg.className = "atom-block-preview";
      loadingMsg.innerHTML = `<span class="atom-block-loading">Loading editor\u2026</span>`;
      content.appendChild(loadingMsg);

      const html = await context.getComponentHtml(componentId);
      if (!html || !editingIframe) {
        if (editingIframe) {
          content.textContent = "";
          const err = document.createElement("span");
          err.className = "atom-block-error-msg";
          err.textContent = "Failed to load editor";
          content.appendChild(err);
        }
        return;
      }

      content.textContent = "";

      const blob = new Blob([html], { type: "text/html" });
      iframeBlobUrl = URL.createObjectURL(blob);

      iframeEl = document.createElement("iframe");
      iframeEl.src = iframeBlobUrl;
      iframeEl.sandbox.add("allow-scripts");
      iframeEl.className = "iframe-block-iframe";
      iframeEl.title = `${blockLabel} editor`;

      iframeEl.addEventListener("load", async () => {
        if (!iframeEl?.contentWindow) return;

        const { alt, src } = parseDrawingSource(currentSource);
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const initData: Record<string, any> = { alt: alt || "" };

        if (src) {
          const isAudio = isAudioFile(src);
          const isBlobUrl = src.startsWith("blob:");
          const blobUrl = isBlobUrl ? src : getBlobUrl(src);

          async function fetchAttachment(): Promise<Response | null> {
            if (blobUrl) {
              try { return await fetch(blobUrl); } catch { return null; }
            } else if (_iframeCtx?.api) {
              const resolved = await queueResolveAttachment(_iframeCtx.api, _iframeCtx.entryPath ?? "", src);
              if (resolved) {
                try { return await fetch(resolved); } catch { return null; }
              }
            }
            return null;
          }

          if (isAudio) {
            const resp = await fetchAttachment();
            if (resp) {
              try {
                const buf = await resp.arrayBuffer();
                const bytes = new Uint8Array(buf);
                let binary = "";
                for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
                initData.audio = btoa(binary);
                initData.mimeType = resp.headers.get("content-type") || "audio/webm";
              } catch { /* ignore */ }
            }
          } else {
            const resp = await fetchAttachment();
            if (resp) {
              try { initData.svg = await resp.text(); } catch { /* ignore */ }
            }
            initData.width = 600;
            initData.height = 300;
          }
        } else if (!hasAudioCapture) {
          initData.width = 600;
          initData.height = 300;
        }

        iframeEl?.contentWindow?.postMessage({
          type: "init",
          theme: document.documentElement.classList.contains("dark") ? "dark" : "light",
          cssVars: collectIframeCssVars(),
          data: initData,
        }, "*");
      });

      content.appendChild(iframeEl);
    }

    // ---- Audio capture (host-mediated) ----

    async function startHostRecording() {
      try {
        audioStream = await navigator.mediaDevices.getUserMedia({ audio: true });
      } catch (err) {
        iframeEl?.contentWindow?.postMessage({
          type: "recording-error",
          error: err instanceof DOMException && err.name === "NotAllowedError"
            ? "Microphone access denied"
            : "Could not access microphone",
        }, "*");
        return;
      }

      // Set up analyser for waveform data
      audioContext = new AudioContext();
      const sourceNode = audioContext.createMediaStreamSource(audioStream);
      analyser = audioContext.createAnalyser();
      analyser.fftSize = 256;
      sourceNode.connect(analyser);
      const dataArray = new Uint8Array(analyser.frequencyBinCount);

      function streamWaveform() {
        if (!analyser || !iframeEl?.contentWindow) return;
        analyser.getByteTimeDomainData(dataArray);
        iframeEl.contentWindow.postMessage({
          type: "waveform-data",
          data: Array.from(dataArray),
        }, "*");
        waveformAnimId = requestAnimationFrame(streamWaveform);
      }

      // Choose MIME type with fallback
      const mimeType = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
        ? "audio/webm;codecs=opus"
        : MediaRecorder.isTypeSupported("audio/webm")
          ? "audio/webm"
          : "";

      const chunks: Blob[] = [];
      const recordingStart = Date.now();
      mediaRecorder = new MediaRecorder(audioStream, mimeType ? { mimeType } : undefined);

      mediaRecorder.ondataavailable = (e) => {
        if (e.data.size > 0) chunks.push(e.data);
      };

      mediaRecorder.onstop = async () => {
        if (waveformAnimId != null) {
          cancelAnimationFrame(waveformAnimId);
          waveformAnimId = null;
        }
        const duration = (Date.now() - recordingStart) / 1000;
        const blob = new Blob(chunks, { type: mediaRecorder?.mimeType || "audio/webm" });
        const buf = await blob.arrayBuffer();
        const bytes = new Uint8Array(buf);
        let binary = "";
        for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
        const base64 = btoa(binary);

        iframeEl?.contentWindow?.postMessage({
          type: "recording-stopped",
          audio: base64,
          mimeType: mediaRecorder?.mimeType || "audio/webm",
          duration,
        }, "*");

        // Clean up stream tracks (mic indicator)
        if (audioStream) {
          audioStream.getTracks().forEach(t => t.stop());
          audioStream = null;
        }
      };

      mediaRecorder.start(100); // collect data every 100ms
      waveformAnimId = requestAnimationFrame(streamWaveform);

      iframeEl?.contentWindow?.postMessage({ type: "recording-started" }, "*");
    }

    function stopHostRecording() {
      if (mediaRecorder && mediaRecorder.state !== "inactive") {
        mediaRecorder.stop();
      }
    }

    // ---- Handle messages from iframe ----
    function handleMessage(event: MessageEvent) {
      if (!iframeEl || event.source !== iframeEl.contentWindow) return;
      const data = event.data;
      if (!data || typeof data !== "object") return;

      if (data.type === "save") {
        handleSave(data);
      } else if (data.type === "cancel") {
        renderPreview(currentSource);
      } else if (data.type === "start-recording" && hasAudioCapture) {
        startHostRecording();
      } else if (data.type === "stop-recording" && hasAudioCapture) {
        stopHostRecording();
      }
    }

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    async function handleSave(data: any) {
      const api = _iframeCtx?.api;
      const entryPath = _iframeCtx?.entryPath ?? "";
      if (!api) return;

      let uint8: Uint8Array;
      let filename: string;
      let mimeType: string;

      const { src } = parseDrawingSource(currentSource);
      const existingFilename = src ? src.split("/").pop() : null;

      if (data.audio) {
        // Audio save path
        const binary = atob(data.audio);
        uint8 = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) uint8[i] = binary.charCodeAt(i);
        mimeType = data.mimeType || "audio/webm";
        const ext = mimeType.includes("webm") ? ".webm" : mimeType.includes("mp3") ? ".mp3" : mimeType.includes("ogg") ? ".ogg" : ".webm";
        filename = (existingFilename && AUDIO_EXTS.includes(getFileExt(existingFilename)))
          ? existingFilename
          : `audio-${Date.now().toString(36)}${ext}`;
      } else {
        // SVG/drawing save path (existing behavior)
        const svgString = data.svg as string;
        uint8 = new TextEncoder().encode(svgString);
        mimeType = "image/svg+xml";
        filename = (existingFilename && existingFilename.endsWith(".svg"))
          ? existingFilename
          : `drawing-${Date.now().toString(36)}.svg`;
      }

      try {
        const attachmentPath = await api.uploadAttachment(entryPath, filename, new Uint8Array(uint8));
        const canonical = await api.canonicalizeLink(attachmentPath, entryPath);

        let relative: string;
        try {
          relative = await api.formatLink(canonical, filename, "plain_relative", entryPath);
        } catch {
          relative = attachmentPath;
        }

        // Create blob URL for display
        const displayBlob = new Blob([uint8 as BlobPart], { type: mimeType });
        const displayBlobUrl = URL.createObjectURL(displayBlob);
        trackBlobUrl(relative, displayBlobUrl);

        // Enqueue sync upload
        const file = new File([uint8 as BlobPart], filename, { type: mimeType });
        await enqueueIncrementalAttachmentUpload(entryPath, canonical, file, new Uint8Array(uint8));

        // Update node source: alt](path
        const newAlt = data.alt || "";
        const formattedPath = formatMarkdownDestination(relative);
        updateSource(`${newAlt}](${formattedPath}`);

        editingIframe = false;
        cleanupIframe();
      } catch (e) {
        console.error("[IframeBlockView] Save failed:", e);
      }
    }

    // Wire up listeners
    window.addEventListener("message", handleMessage);

    if (editor.isEditable) {
      editBtn.addEventListener("click", () => {
        if (editingIframe) {
          renderPreview(currentSource);
        } else {
          startEditing();
        }
      });
    } else {
      editBtn.style.display = "none";
    }

    // Auto-open for new (empty source) drawings
    if (!currentSource.trim() && editor.isEditable) {
      startEditing();
    } else {
      renderPreview(currentSource);
    }

    return {
      dom,
      stopEvent: (event: Event) => dom.contains(event.target as globalThis.Node),
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      update(updatedNode: any) {
        if (updatedNode.type.name !== ext.extension_id) return false;
        const newSource = (updatedNode.attrs.source as string) || "";
        if (newSource !== currentSource) {
          currentSource = newSource;
          if (!editingIframe) renderPreview(newSource);
        }
        return true;
      },
      destroy() {
        window.removeEventListener("message", handleMessage);
        cleanupIframe();
      },
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
