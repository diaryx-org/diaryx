import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { afterEach, describe, expect, it, vi } from "vitest";

const { mountSpy, unmountSpy } = vi.hoisted(() => ({
  mountSpy: vi.fn(() => ({})),
  unmountSpy: vi.fn(),
}));

vi.mock("svelte", async () => {
  const actual = await vi.importActual<typeof import("svelte")>("svelte");
  return {
    ...actual,
    mount: mountSpy,
    unmount: unmountSpy,
  };
});

import { BlockPickerNode } from "./BlockPickerNode";
import { ConditionalBlock } from "./ConditionalBlock";

const editors: Editor[] = [];
const elements: HTMLElement[] = [];

afterEach(() => {
  while (editors.length > 0) {
    editors.pop()?.destroy();
  }
  while (elements.length > 0) {
    elements.pop()?.remove();
  }
  mountSpy.mockClear();
  unmountSpy.mockClear();
});

describe("BlockPickerNode", () => {
  it("runs follow-up editor commands after deleting the picker node", async () => {
    const element = document.createElement("div");
    document.body.appendChild(element);
    elements.push(element);

    const editor = new Editor({
      element,
      extensions: [
        StarterKit,
        BlockPickerNode,
        ConditionalBlock.configure({ enabled: false }),
      ],
      content: {
        type: "doc",
        content: [
          { type: "blockPickerNode" },
          { type: "paragraph" },
        ],
      },
    });

    editors.push(editor);

    const firstMountCall = mountSpy.mock.calls[0] as unknown[] | undefined;
    const mountOptions = firstMountCall?.[1] as
      | { props?: { onSelect?: (action: () => void) => void } }
      | undefined;
    const onSelect = mountOptions?.props?.onSelect;

    expect(typeof onSelect).toBe("function");

    onSelect?.(() => {
      // Match the picker view behavior: resolve the command after the picker
      // atom removes itself so the command executes against fresh state.
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const nextCommands = editor.commands as Record<string, any>;
      const nextCommandFn = nextCommands.insertConditionalBlock;
      expect(typeof nextCommandFn).toBe("function");
      nextCommandFn({
        helperType: "if",
        condition: "draft",
      });
    });

    await Promise.resolve();

    expect(editor.getJSON().content ?? []).toEqual([
      { type: "paragraph" },
      {
        type: "conditionalMarker",
        attrs: {
          variant: "open",
          helperType: "if",
          condition: "draft",
        },
      },
      { type: "paragraph" },
      {
        type: "conditionalMarker",
        attrs: {
          variant: "else",
          helperType: "",
          condition: "",
        },
      },
      { type: "paragraph" },
      {
        type: "conditionalMarker",
        attrs: {
          variant: "close",
          helperType: "if",
          condition: "",
        },
      },
      { type: "paragraph" },
    ]);
  });
});
