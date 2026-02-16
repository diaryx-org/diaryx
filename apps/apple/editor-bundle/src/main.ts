import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import Link from "@tiptap/extension-link";
import TaskList from "@tiptap/extension-task-list";
import TaskItem from "@tiptap/extension-task-item";
import Placeholder from "@tiptap/extension-placeholder";
import CodeBlock from "@tiptap/extension-code-block";
import Image from "@tiptap/extension-image";
import Typography from "@tiptap/extension-typography";
import { Table } from "@tiptap/extension-table";
import { TableRow } from "@tiptap/extension-table-row";
import { TableHeader } from "@tiptap/extension-table-header";
import { TableCell } from "@tiptap/extension-table-cell";
import { Markdown } from "@tiptap/markdown";

import "./style.css";

// Extend Window to include our bridge and webkit message handlers
declare global {
  interface Window {
    editorBridge: EditorBridge;
    webkit?: {
      messageHandlers?: {
        bridge?: {
          postMessage(message: BridgeMessage): void;
        };
      };
    };
  }
}

interface EditorBridge {
  setMarkdown(markdown: string): void;
  getMarkdown(): string;
  setJSON(json: string): void;
  getJSON(): string;
  // Compatibility aliases for existing Swift bridge calls.
  setContent(markdown: string): void;
  getContent(): string;
  setEditable(editable: boolean): void;
}

type BridgeMessage =
  | { type: "ready" }
  | { type: "contentChanged"; markdown: string }
  | { type: "linkClicked"; href: string };

function postMessage(message: BridgeMessage) {
  window.webkit?.messageHandlers?.bridge?.postMessage(message);
}

try {
  const editorElement = document.getElementById("editor");
  if (!editorElement) {
    throw new Error("Could not find #editor element");
  }

  const editor = new Editor({
    element: editorElement,
    extensions: [
      StarterKit.configure({
        codeBlock: false,
        link: false,
      }),
      Markdown.configure({
        markedOptions: { gfm: true },
      }),
      Link.configure({
        openOnClick: false,
        HTMLAttributes: {
          class: "editor-link",
        },
      }),
      TaskList,
      TaskItem.configure({
        nested: true,
      }),
      Placeholder.configure({
        placeholder: "Start writing...",
      }),
      CodeBlock.configure({
        HTMLAttributes: {
          class: "editor-code-block",
        },
      }),
      Image.configure({
        inline: true,
        allowBase64: true,
        HTMLAttributes: {
          class: "editor-image",
        },
      }),
      Typography,
      Table.configure({ resizable: false }),
      TableRow,
      TableHeader,
      TableCell,
    ],
    content: "",
    onUpdate: () => {
      const markdown = editor.getMarkdown();
      postMessage({ type: "contentChanged", markdown });
    },
    editorProps: {
      handleClick: (_view, _pos, event) => {
        const target = event.target as HTMLElement;
        const link = target.closest("a");
        if (link) {
          event.preventDefault();
          const href = link.getAttribute("href") || "";
          postMessage({ type: "linkClicked", href });
          return true;
        }
        return false;
      },
    },
  });

  // Expose bridge API for Swift to call via evaluateJavaScript
  window.editorBridge = {
    setMarkdown(markdown: string) {
      editor.commands.setContent(markdown, { contentType: "markdown" });
    },
    getMarkdown(): string {
      return editor.getMarkdown();
    },
    setJSON(json: string) {
      try {
        const doc = JSON.parse(json);
        editor.commands.setContent(doc);
      } catch (error) {
        console.error("Invalid JSON passed to setJSON:", error);
      }
    },
    getJSON(): string {
      return JSON.stringify(editor.getJSON());
    },
    setContent(markdown: string) {
      this.setMarkdown(markdown);
    },
    getContent(): string {
      return this.getMarkdown();
    },
    setEditable(editable: boolean) {
      editor.setEditable(editable);
    },
  };

  // Notify Swift that the editor is ready
  postMessage({ type: "ready" });
} catch (err) {
  // Show error visually so it's obvious what went wrong
  const el = document.getElementById("editor") || document.body;
  el.innerHTML = `<pre style="color:red;padding:2rem;font-size:14px;">Editor failed to initialize:\n${err}</pre>`;
  console.error("Editor init failed:", err);
}
