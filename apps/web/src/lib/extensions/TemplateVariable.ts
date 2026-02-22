/**
 * TipTap Node extension for render-time template variables.
 *
 * Models `{{ variable }}` expressions as inline atom nodes displayed as styled
 * pills in the editor. The variable name is stored as an attribute.
 *
 * Markdown round-trip:
 * - Parse: Inline tokenizer matches `{{ name }}` (but NOT block helpers like
 *   `{{#each}}`, `{{/each}}`, `{{#if}}`, `{{else}}`, `{{#for-audience}}`).
 * - Serialize: `renderMarkdown` outputs `{{ name }}` preserving the original syntax.
 *
 * This is purely a visual enhancement — the raw `{{ }}` syntax is preserved in
 * the markdown file and processed by the Handlebars engine at render/publish time.
 */

import { Node, mergeAttributes } from "@tiptap/core";
import TemplateVariableNodeView from "../components/TemplateVariableNodeView.svelte";
import { mount, unmount } from "svelte";

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    templateVariable: {
      /** Insert a template variable at the current cursor position */
      insertTemplateVariable: (name: string) => ReturnType;
    };
  }
}

export const TemplateVariable = Node.create({
  name: "templateVariable",

  group: "inline",

  inline: true,

  atom: true,

  selectable: true,

  addAttributes() {
    return {
      name: { default: "" },
    };
  },

  parseHTML() {
    return [{ tag: "span[data-template-variable]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return [
      "span",
      mergeAttributes(HTMLAttributes, {
        "data-template-variable": "",
        class: "template-variable",
      }),
      `{{ ${HTMLAttributes.name} }}`,
    ];
  },

  addCommands() {
    return {
      insertTemplateVariable:
        (name: string) =>
        ({ editor, tr, dispatch }) => {
          if (dispatch) {
            const node = editor.schema.nodes.templateVariable.create({
              name,
            });
            tr.replaceSelectionWith(node);
            dispatch(tr);
          }
          return true;
        },
    };
  },

  addNodeView() {
    return ({ node, editor }) => {
      const dom = document.createElement("span");
      dom.classList.add("template-variable");
      dom.setAttribute("data-template-variable", "");
      dom.setAttribute("contenteditable", "false");

      let currentName = node.attrs.name as string;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      let svelteComponent: Record<string, any> | null = null;

      function mountComponent(name: string) {
        svelteComponent = mount(TemplateVariableNodeView, {
          target: dom,
          props: {
            name,
            readonly: !editor.isEditable,
          },
        });
      }

      mountComponent(currentName);

      return {
        dom,
        stopEvent(event: Event) {
          return dom.contains(event.target as globalThis.Node);
        },
        update(updatedNode) {
          if (updatedNode.type.name !== "templateVariable") return false;
          const newName = updatedNode.attrs.name as string;
          if (newName !== currentName) {
            currentName = newName;
            if (svelteComponent) {
              unmount(svelteComponent);
            }
            mountComponent(newName);
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

  // Custom inline tokenizer for {{ variable }}
  // Matches simple variable references but NOT block helpers ({{#...}}, {{/...}}, {{else}})
  // @ts-ignore - markdownTokenizer is a custom field for @tiptap/markdown
  markdownTokenizer: {
    name: "templateVariable",
    level: "inline",
    start: "{{",
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    tokenize(src: string, _tokens: any[]) {
      // Match {{ variable }} but NOT:
      // - {{#each}}, {{#if}}, {{#for-audience}}, {{#unless}}, {{#with}} (block openers)
      // - {{/each}}, {{/if}}, {{/for-audience}}, {{/unless}}, {{/with}} (block closers)
      // - {{else}} (block alternation)
      // - {{> partial}} (partials)
      // - {{! comment}} (comments)
      // - Subexpressions like (contains audience "public") inside {{#if ...}}
      const match = /^\{\{\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*\}\}/.exec(src);
      if (!match) return undefined;

      const name = match[1];

      // Reject Handlebars keywords that look like simple variables
      if (name === "else" || name === "this") return undefined;

      return {
        type: "templateVariable",
        raw: match[0],
        name,
      };
    },
  },

  // Parse the templateVariable token into a node
  // @ts-ignore - parseMarkdown is a custom field for @tiptap/markdown
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  parseMarkdown(token: any, helpers: any) {
    return helpers.createNode("templateVariable", {
      name: token.name,
    });
  },
}).extend({
  // Render template variable back to {{ name }} in markdown
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  renderMarkdown(node: any) {
    return `{{ ${node.attrs?.name ?? ""} }}`;
  },
});
