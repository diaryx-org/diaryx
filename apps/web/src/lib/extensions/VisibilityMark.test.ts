import { Editor } from "@tiptap/core";
import Code from "@tiptap/extension-code";
import StarterKit from "@tiptap/starter-kit";
import { afterEach, describe, expect, it } from "vitest";

import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";

import { VisibilityMark, collectVisRanges } from "./VisibilityMark";

const editors: Editor[] = [];
const elements: HTMLElement[] = [];
const templateContextStore = getTemplateContextStore();

afterEach(() => {
  templateContextStore.setPreviewAudience(null);
  templateContextStore.clear();

  while (editors.length > 0) {
    editors.pop()?.destroy();
  }

  while (elements.length > 0) {
    elements.pop()?.remove();
  }
});

function createEditor(content: Record<string, unknown>): {
  editor: Editor;
  element: HTMLDivElement;
} {
  const element = document.createElement("div");
  document.body.appendChild(element);
  elements.push(element);

  const editor = new Editor({
    element,
    extensions: [
      StarterKit.configure({
        code: false,
      }),
      Code.extend({
        excludes: "bold italic strike",
      }),
      VisibilityMark,
    ],
    content,
  });

  editors.push(editor);
  return { editor, element };
}

describe("VisibilityMark", () => {
  it("bridges a visibility span across inline code into one range", () => {
    const { editor } = createEditor({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            {
              type: "text",
              text: "before ",
              marks: [
                {
                  type: "visibilityMark",
                  attrs: { audiences: ["family"] },
                },
              ],
            },
            {
              type: "text",
              text: "code",
              marks: [
                { type: "code" },
                {
                  type: "visibilityMark",
                  attrs: { audiences: ["family"] },
                },
              ],
            },
            {
              type: "text",
              text: " after",
              marks: [
                {
                  type: "visibilityMark",
                  attrs: { audiences: ["family"] },
                },
              ],
            },
          ],
        },
      ],
    });

    const textNodes: { from: number; to: number }[] = [];
    editor.state.doc.descendants((node, pos) => {
      if (!node.isText) return;
      textNodes.push({ from: pos, to: pos + node.nodeSize });
    });

    const markType = editor.schema.marks.visibilityMark;
    const ranges = collectVisRanges(editor.state.doc, markType, {});

    expect(ranges).toHaveLength(1);
    expect(ranges[0]).toMatchObject({
      from: textNodes[0].from,
      to: textNodes[textNodes.length - 1].to,
      audiences: ["family"],
    });
  });

  it("keeps matching inline visibility indicators visible in preview mode", () => {
    const { editor, element } = createEditor({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            {
              type: "text",
              text: "family only",
              marks: [
                {
                  type: "visibilityMark",
                  attrs: { audiences: ["family"] },
                },
              ],
            },
          ],
        },
      ],
    });

    expect(element.querySelectorAll(".vis-underline").length).toBeGreaterThan(0);

    templateContextStore.setPreviewAudience("family");
    editor.view.dispatch(
      editor.state.tr.setMeta("templateContextChanged", true),
    );

    expect(element.querySelectorAll(".vis-underline--preview").length)
      .toBeGreaterThan(0);
    expect(element.querySelector(".vis-mark--hidden")).toBeNull();
  });
});
