import { Editor, Node } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import Image from "@tiptap/extension-image";
import { NodeSelection } from "@tiptap/pm/state";
import { afterEach, describe, expect, it } from "vitest";

const editors: Editor[] = [];
const elements: HTMLElement[] = [];

const PlaceholderBlock = Node.create({
  name: "placeholderBlock",
  group: "block",
  atom: true,
  selectable: true,

  parseHTML() {
    return [{ tag: "div[data-placeholder-block]" }];
  },

  renderHTML() {
    return ["div", { "data-placeholder-block": "" }];
  },
});

afterEach(() => {
  while (editors.length > 0) {
    editors.pop()?.destroy();
  }
  while (elements.length > 0) {
    elements.pop()?.remove();
  }
});

describe("HTML attachment insertion isolation", () => {
  it("shows inline image replacement wraps the selected block in valid paragraph content", () => {
    const element = document.createElement("div");
    document.body.appendChild(element);
    elements.push(element);

    const editor = new Editor({
      element,
      extensions: [
        StarterKit,
        Image.configure({
          inline: true,
          allowBase64: true,
        }),
        PlaceholderBlock,
      ],
      content: {
        type: "doc",
        content: [{ type: "placeholderBlock" }],
      },
    });

    editors.push(editor);

    editor.view.dispatch(
      editor.state.tr.setSelection(NodeSelection.create(editor.state.doc, 0)),
    );

    editor.commands.setImage({
      src: "./_attachments/widget.html",
      alt: "widget.html",
    });

    expect(editor.getJSON().content ?? []).toEqual([
      {
        type: "paragraph",
        content: [
          {
            type: "image",
            attrs: {
              src: "./_attachments/widget.html",
              alt: "widget.html",
              title: null,
              width: null,
              height: null,
            },
          },
        ],
      },
      { type: "paragraph" },
    ]);

    expect(() => editor.state.doc.check()).not.toThrow();
  });
});
