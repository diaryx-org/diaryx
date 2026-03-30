import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { afterEach, describe, expect, it } from "vitest";

import {
  createMarkFromManifest,
  scopePluginCss,
  type EditorExtensionManifest,
} from "./editorExtensionFactory";

const editors: Editor[] = [];
const elements: HTMLElement[] = [];

afterEach(() => {
  while (editors.length > 0) {
    editors.pop()?.destroy();
  }
  while (elements.length > 0) {
    elements.pop()?.remove();
  }
});

const spoilerManifest: EditorExtensionManifest = {
  slot: "EditorExtension",
  extension_id: "spoiler",
  node_type: "InlineMark",
  markdown: {
    level: "Inline",
    open: "||",
    close: "||",
  },
  render_export: null,
  edit_mode: null,
  css: `
    .spoiler-mark { border-radius: 4px; }
    .spoiler-hidden { color: transparent; }
    .spoiler-revealed { color: inherit; }
  `,
  click_behavior: {
    ToggleClass: {
      hidden_class: "spoiler-hidden",
      revealed_class: "spoiler-revealed",
    },
  },
  insert_command: null,
  keyboard_shortcut: null,
};

describe("editorExtensionFactory", () => {
  it("scopes plugin selectors without dropping mark and descendant matches", () => {
    const scoped = scopePluginCss(
      "coloredHighlight",
      `
        .highlight-red { background: red; }
        .dark .highlight-blue:hover { background: blue; }
        .picker-swatch { border: 1px solid currentColor; }
      `,
    );

    expect(scoped).toContain(".highlight-red.plugin-ext-coloredHighlight");
    expect(scoped).toContain(".plugin-ext-coloredHighlight .highlight-red");
    expect(scoped).toContain(".dark .highlight-blue.plugin-ext-coloredHighlight:hover");
    expect(scoped).toContain(".plugin-ext-coloredHighlight .picker-swatch");
    expect(scoped).not.toContain(".plugin-ext-coloredHighlight {");
  });

  it("drops unsafe at-rules while preserving nested media rules", () => {
    const scoped = scopePluginCss(
      "coloredHighlight",
      `
        @import url("https://example.com/evil.css");
        @font-face { font-family: "Bad"; src: url("bad.woff2"); }
        @media (prefers-color-scheme: dark) {
          .highlight-red { background: maroon; }
        }
      `,
    );

    expect(scoped).not.toContain("@import");
    expect(scoped).not.toContain("@font-face");
    expect(scoped).toContain("@media (prefers-color-scheme: dark)");
    expect(scoped).toContain(".highlight-red.plugin-ext-coloredHighlight");
  });

  it("toggles spoiler marks on pointer press", () => {
    const element = document.createElement("div");
    document.body.appendChild(element);
    elements.push(element);

    const editor = new Editor({
      element,
      extensions: [
        StarterKit,
        createMarkFromManifest(spoilerManifest),
      ],
      content: {
        type: "doc",
        content: [
          {
            type: "paragraph",
            content: [
              {
                type: "text",
                text: "secret",
                marks: [{ type: "spoiler" }],
              },
            ],
          },
        ],
      },
    });

    editors.push(editor);

    const markEl = element.querySelector("[data-spoiler]") as HTMLElement | null;

    expect(markEl).not.toBeNull();
    expect(markEl?.classList.contains("spoiler-hidden")).toBe(true);

    markEl?.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, cancelable: true }),
    );

    expect(markEl?.classList.contains("spoiler-hidden")).toBe(false);
    expect(markEl?.classList.contains("spoiler-revealed")).toBe(true);

    markEl?.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, cancelable: true }),
    );

    expect(markEl?.classList.contains("spoiler-hidden")).toBe(true);
    expect(markEl?.classList.contains("spoiler-revealed")).toBe(false);
  });
});
