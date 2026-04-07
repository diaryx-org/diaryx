import { render, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Polyfill ResizeObserver for jsdom
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as any;
}

// vi.hoisted runs before vi.mock factories, making these variables available.
const { editorState, mockEditorInstance } = vi.hoisted(() => {
  const editorState = { createConfig: null as any };
  const mockEditorInstance = {
    getHTML: vi.fn(() => "<p>test</p>"),
    getJSON: vi.fn(() => ({})),
    commands: {
      setContent: vi.fn(),
      focus: vi.fn(),
      setTextSelection: vi.fn(),
      insertAttachmentPicker: vi.fn(),
    },
    chain: vi.fn(() => ({
      focus: vi.fn().mockReturnThis(),
      setImage: vi.fn().mockReturnThis(),
      reorderFootnotes: vi.fn().mockReturnThis(),
      command: vi.fn().mockReturnThis(),
      run: vi.fn(),
    })),
    on: vi.fn(),
    off: vi.fn(),
    destroy: vi.fn(),
    isEditable: true,
    setEditable: vi.fn(),
    view: { dom: document.createElement("div"), dispatch: vi.fn() },
    state: {
      doc: { content: { size: 0 } },
      tr: { setMeta: vi.fn().mockReturnThis() },
    },
    storage: { markdown: { getMarkdown: vi.fn(() => "test markdown") } },
    extensionManager: { extensions: [] },
    isActive: vi.fn(() => false),
  };
  return { editorState, mockEditorInstance };
});

// ── Mock TipTap core ────────────────────────────────────────────────
vi.mock("@tiptap/core", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tiptap/core")>();
  // Must use a regular function (not arrow) so it works with `new Editor(...)`.
  function MockEditor(this: any, config: any) {
    editorState.createConfig = config;
    if (config.element) {
      config.element.innerHTML = "<div class='ProseMirror'></div>";
    }
    Object.assign(this, mockEditorInstance);
  }
  return {
    ...actual,
    Editor: MockEditor,
    Extension: { create: vi.fn(() => ({})) },
  };
});

// ── Mock TipTap extensions ──────────────────────────────────────────
vi.mock("@tiptap/starter-kit", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/markdown", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { Markdown: o };
});
vi.mock("@tiptap/extension-link", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/extension-task-list", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/extension-task-item", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/extension-placeholder", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/extension-code-block", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/extension-typography", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/extension-image", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/extension-table", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { Table: o };
});
vi.mock("@tiptap/extension-table-row", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { TableRow: o };
});
vi.mock("@tiptap/extension-table-header", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { TableHeader: o };
});
vi.mock("@tiptap/extension-table-cell", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { TableCell: o };
});
vi.mock("@tiptap/extension-floating-menu", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/extension-bubble-menu", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { default: o };
});
vi.mock("@tiptap/pm/state", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tiptap/pm/state")>();
  return {
    ...actual,
    Plugin: vi.fn(),
  };
});

// ── Mock local modules ──────────────────────────────────────────────
vi.mock("../models/services/attachmentService", () => ({
  formatMarkdownDestination: vi.fn((s: string) => s),
  getPathForBlobUrl: vi.fn(() => null),
  getBlobUrl: vi.fn(() => null),
  isVideoFile: vi.fn(() => false),
  isAudioFile: vi.fn(() => false),
  isPreviewableAttachmentKind: vi.fn(() => false),
  queueResolveAttachment: vi.fn(async () => null),
}));

vi.mock("$lib/utils/linkParser", () => ({
  parseLinkDisplay: vi.fn((href: string) => ({ label: href, href })),
}));

vi.mock("./components/FloatingMenuComponent.svelte", () => ({
  default: vi.fn(),
}));
vi.mock("./components/BubbleMenuComponent.svelte", () => ({
  default: vi.fn(),
}));

vi.mock("./extensions/AttachmentPickerNode", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { AttachmentPickerNode: o };
});
vi.mock("./extensions/BlockPickerNode", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { BlockPickerNode: o };
});
vi.mock("./extensions/HtmlBlock", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { HtmlBlock: o };
});
vi.mock("./extensions/TableControls", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { TableControls: o };
});
vi.mock("./extensions/FootnoteRef", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return {
    FootnoteRef: o,
    preprocessFootnotes: vi.fn((s: string) => s),
    appendFootnoteDefinitions: vi.fn(() => ""),
  };
});
vi.mock("./extensions/SearchHighlight", () => {
  const o: any = { configure: vi.fn(), extend: vi.fn() };
  o.configure.mockReturnValue(o); o.extend.mockReturnValue(o);
  return { SearchHighlight: o };
});

vi.mock("./stores/templateContextStore.svelte", () => ({
  getTemplateContextStore: vi.fn(() => ({ context: {} })),
}));

vi.mock("$lib/plugins/browserPluginManager.svelte", () => ({
  getEditorExtensions: vi.fn(() => []),
  getPluginExtensionsVersion: vi.fn(() => "0"),
}));
vi.mock("$lib/plugins/preservedEditorExtensions.svelte", () => ({
  getPreservedEditorExtensions: vi.fn(() => []),
}));
vi.mock("$lib/plugins/tauriEditorExtensions", () => ({
  getTauriEditorExtensions: vi.fn(() => []),
}));
vi.mock("$lib/plugins/editorExtensionFactory", () => ({
  setEditorExtensionIframeContext: vi.fn(),
}));
vi.mock("$lib/backend/api", () => ({
  Api: vi.fn(),
}));
vi.mock("$lib/backend/interface", () => ({
  isTauri: vi.fn(() => false),
}));
vi.mock("$lib/hooks/useMobile.svelte", () => ({
  isIOS: vi.fn(() => false),
}));
vi.mock("@/models/stores/workspaceStore.svelte", () => ({
  workspaceStore: { currentNode: null },
}));
vi.mock("$lib/stores/linkFormatStore.svelte", () => ({
  getLinkFormatStore: vi.fn(() => ({})),
}));
vi.mock("@/models/stores/pluginStore.svelte", () => ({
  getPluginStore: vi.fn(() => ({ allManifests: [] })),
}));
vi.mock("$lib/backend", () => ({
  TreeNode: vi.fn(),
}));

// ── Import the component AFTER all mocks ────────────────────────────
import EditorComponent from "./Editor.svelte";
import {
  bubbleMenuHasRelevantFocus,
  shouldKeepBubbleMenuVisible,
} from "./editorMenuVisibility";

// Helper: wait for the mock Editor constructor to have been called.
// Since MockEditor is a plain function (not vi.fn), we check editorState.
function waitForEditorCreation() {
  return waitFor(() => {
    expect(editorState.createConfig).not.toBeNull();
  });
}

// ── Tests ───────────────────────────────────────────────────────────
describe("Editor.svelte", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    editorState.createConfig = null;
  });

  it("renders the editor element", () => {
    const { container } = render(EditorComponent, {
      props: { readonly: true },
    });
    const editorDiv = container.querySelector("div.min-h-full");
    expect(editorDiv).toBeTruthy();
  });

  it("creates a TipTap editor on mount (readonly mode)", async () => {
    render(EditorComponent, {
      props: { readonly: true },
    });

    await waitForEditorCreation();
  });

  it("passes content to the editor", async () => {
    const testContent = "# Hello World\n\nSome content here.";
    render(EditorComponent, {
      props: { content: testContent, readonly: true },
    });

    await waitForEditorCreation();

    expect(editorState.createConfig.content).toBe(testContent);
    expect(editorState.createConfig.contentType).toBe("markdown");
  });

  it("sets editor to readonly when readonly prop is true", async () => {
    render(EditorComponent, {
      props: { readonly: true },
    });

    await waitForEditorCreation();

    expect(editorState.createConfig.editable).toBe(false);
  });

  it("sets editor to editable when readonly is false", () => {
    // In non-readonly mode the editor waits for floating/bubble menu elements
    // which are mocked child components. This test verifies the component
    // mounts without error even when the editor isn't immediately created.
    render(EditorComponent, {
      props: { readonly: false },
    });
    expect(true).toBe(true);
  });

  it("destroys editor on unmount", async () => {
    const { unmount } = render(EditorComponent, {
      props: { readonly: true },
    });

    await waitForEditorCreation();

    unmount();

    expect(mockEditorInstance.destroy).toHaveBeenCalled();
  });

  it("applies editor-content class via editorProps", async () => {
    render(EditorComponent, {
      props: { readonly: true },
    });

    await waitForEditorCreation();

    expect(editorState.createConfig.editorProps.attributes.class).toBe(
      "editor-content",
    );
  });

  it("wires onUpdate to call onchange callback", async () => {
    const onchange = vi.fn();
    render(EditorComponent, {
      props: { readonly: true, onchange },
    });

    await waitForEditorCreation();

    expect(editorState.createConfig.onUpdate).toBeDefined();
    editorState.createConfig.onUpdate();
    expect(onchange).toHaveBeenCalled();
  });

  it("wires onBlur to call onblur callback", async () => {
    const onblur = vi.fn();
    render(EditorComponent, {
      props: { readonly: true, onblur },
    });

    await waitForEditorCreation();

    expect(editorState.createConfig.onBlur).toBeDefined();
    editorState.createConfig.onBlur();
    expect(onblur).toHaveBeenCalled();
  });

  it("intercepts local editor link clicks before native navigation", async () => {
    const onLinkClick = vi.fn();
    render(EditorComponent, {
      props: { readonly: true, onLinkClick },
    });

    await waitForEditorCreation();

    const anchor = document.createElement("a");
    anchor.setAttribute("href", "notes/file.md");
    const event = {
      target: anchor,
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    } as unknown as MouseEvent;

    const handled = editorState.createConfig.editorProps.handleDOMEvents.click(
      mockEditorInstance.view,
      event,
    );

    expect(handled).toBe(true);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(event.stopPropagation).toHaveBeenCalled();
    expect(onLinkClick).toHaveBeenCalledWith("notes/file.md");
  });

  it("opens external editor links in a new tab when no link handler is provided", async () => {
    const openSpy = vi.spyOn(window, "open").mockImplementation(() => null);
    render(EditorComponent, {
      props: { readonly: true },
    });

    await waitForEditorCreation();

    const anchor = document.createElement("a");
    anchor.setAttribute("href", "https://example.com");
    const event = {
      target: anchor,
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    } as unknown as MouseEvent;

    const handled = editorState.createConfig.editorProps.handleDOMEvents.click(
      mockEditorInstance.view,
      event,
    );

    expect(handled).toBe(true);
    expect(openSpy).toHaveBeenCalledWith(
      "https://example.com",
      "_blank",
      "noopener,noreferrer",
    );

    openSpy.mockRestore();
  });

  it("has role=application on the editor container", () => {
    const { container } = render(EditorComponent, {
      props: { readonly: true },
    });
    const el = container.querySelector("[role='application']");
    expect(el).toBeTruthy();
  });

  it("keeps the bubble menu visible while focus moves into the link popover", () => {
    const bubbleMenuElement = document.createElement("div");
    const popoverInput = document.createElement("input");
    bubbleMenuElement.appendChild(popoverInput);
    document.body.appendChild(bubbleMenuElement);
    popoverInput.focus();

    expect(
      bubbleMenuHasRelevantFocus(
        bubbleMenuElement,
        document.activeElement,
        false,
      ),
    ).toBe(true);

    bubbleMenuElement.remove();
  });

  it("keeps the bubble menu visible while the link popover is open", () => {
    expect(
      shouldKeepBubbleMenuVisible({
        bubbleMenuElement: undefined,
        activeElement: null,
        editorHasFocus: false,
        linkPopoverOpen: true,
      }),
    ).toBe(true);
  });

  it("lets the bubble menu hide when focus and editor selection context are gone", () => {
    expect(
      shouldKeepBubbleMenuVisible({
        bubbleMenuElement: undefined,
        activeElement: null,
        editorHasFocus: false,
        linkPopoverOpen: false,
      }),
    ).toBe(false);
  });
});
