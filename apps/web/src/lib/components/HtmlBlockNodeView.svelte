<script lang="ts">
  import DOMPurify from "dompurify";
  import { Code, Check } from "@lucide/svelte";
  import type { Api } from "$lib/backend/api";
  import { resolveImageSrc } from "@/models/services/attachmentService";

  const ALLOWED_IFRAME_ORIGINS = [
    'https://www.youtube.com',
    'https://www.youtube-nocookie.com',
    'https://player.vimeo.com',
    'https://docs.google.com',
    'https://drive.google.com',
    'https://sheets.google.com',
    'https://slides.google.com',
    'https://calendar.google.com',
    'https://www.google.com',
    'https://codepen.io',
    'https://codesandbox.io',
    'https://stackblitz.com',
    'https://open.spotify.com',
    'https://w.soundcloud.com',
    'https://bandcamp.com',
    'https://www.figma.com',
    'https://excalidraw.com',
    'https://gist.github.com',
  ];

  // Remove iframes whose src doesn't match the whitelist
  DOMPurify.addHook('uponSanitizeElement', (node, data) => {
    if (data.tagName === 'iframe' && node instanceof Element) {
      const src = node.getAttribute('src') || '';
      if (!ALLOWED_IFRAME_ORIGINS.some(origin => src.startsWith(origin + '/'))) {
        node.parentNode?.removeChild(node);
      }
    }
  });

  interface Props {
    content: string;
    readonly: boolean;
    entryPath: string;
    api: Api | null;
    onUpdate: (html: string) => void;
  }

  let { content, readonly, entryPath, api, onUpdate }: Props = $props();

  let mode = $state<"preview" | "source">("preview");
  let sourceText = $state("");

  // HTML with <img src> paths resolved to blob URLs for preview display only.
  // The stored content always keeps original paths.
  let previewHtml = $state("");

  // Sync sourceText when content prop changes externally (undo/redo/initial)
  $effect(() => {
    sourceText = content;
  });

  // Resolve <img src> local paths to blob URLs for preview
  $effect(() => {
    resolvePreviewHtml(content);
  });

  async function resolvePreviewHtml(html: string) {
    if (!api || !entryPath) {
      previewHtml = html;
      return;
    }

    const imgSrcRegex = /<img\s[^>]*?\bsrc\s*=\s*(["'])((?:(?!\1).)+)\1/gi;
    let result = html;
    const replacements: { original: string; replacement: string }[] = [];

    let match: RegExpExecArray | null;
    while ((match = imgSrcRegex.exec(html)) !== null) {
      const [fullMatch, quote, rawSrc] = match;

      const blobUrl = await resolveImageSrc(rawSrc.trim(), entryPath, api);
      if (blobUrl && blobUrl !== rawSrc.trim()) {
        replacements.push({
          original: fullMatch,
          replacement: fullMatch.replace(
            `${quote}${rawSrc}${quote}`,
            `${quote}${blobUrl}${quote}`,
          ),
        });
      }
    }

    for (const { original, replacement } of replacements) {
      result = result.replace(original, replacement);
    }

    previewHtml = result;
  }

  const sanitized = $derived(
    DOMPurify.sanitize(previewHtml, {
      ADD_TAGS: ["style", "iframe"],
      ADD_ATTR: ["style", "class", "src", "width", "height", "frameborder",
        "allow", "allowfullscreen", "title", "loading", "referrerpolicy"],
      FORBID_TAGS: ["script"],
      FORBID_ATTR: ["onerror", "onclick", "onload", "onmouseover"],
      // Allow blob: URIs so resolved attachment images render in preview
      ALLOWED_URI_REGEXP:
        /^(?:(?:(?:f|ht)tps?|mailto|tel|callto|sms|cid|xmpp|blob):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i,
    }),
  );

  function commitSource() {
    if (sourceText !== content) {
      onUpdate(sourceText);
    }
    mode = "preview";
  }

  function handleKeydown(e: KeyboardEvent) {
    // Escape to cancel, Cmd/Ctrl+Enter to save
    if (e.key === "Escape") {
      sourceText = content;
      mode = "preview";
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      commitSource();
    }
  }
</script>

<div class="html-block-container">
  <div class="html-block-header">
    <span class="html-block-label">HTML</span>
    {#if !readonly}
      {#if mode === "preview"}
        <button
          type="button"
          class="html-block-toggle"
          onclick={() => (mode = "source")}
          title="Edit source"
        >
          <Code class="size-3.5" />
        </button>
      {:else}
        <button
          type="button"
          class="html-block-toggle"
          onclick={commitSource}
          title="Done editing"
        >
          <Check class="size-3.5" />
        </button>
      {/if}
    {/if}
  </div>

  {#if mode === "source" && !readonly}
    <textarea
      class="html-block-source"
      bind:value={sourceText}
      onblur={commitSource}
      onkeydown={handleKeydown}
      spellcheck="false"
    ></textarea>
  {:else}
    <div class="html-block-preview">
      {#if sanitized}
        {@html sanitized}
      {:else}
        <span class="html-block-empty">Empty HTML block</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .html-block-container {
    border: 1px dashed var(--border);
    border-radius: 6px;
    margin: 0.5em 0;
    overflow: hidden;
  }

  .html-block-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 8px;
    background: var(--muted);
    border-bottom: 1px solid var(--border);
  }

  .html-block-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--muted-foreground);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .html-block-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 2px;
    border: none;
    background: transparent;
    border-radius: 3px;
    cursor: pointer;
    color: var(--muted-foreground);
  }

  .html-block-toggle:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .html-block-preview {
    padding: 12px;
  }

  .html-block-empty {
    color: var(--muted-foreground);
    font-style: italic;
    font-size: 13px;
  }

  .html-block-source {
    width: 100%;
    min-height: 80px;
    padding: 12px;
    border: none;
    background: var(--card);
    color: var(--foreground);
    font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
    font-size: 13px;
    line-height: 1.5;
    resize: vertical;
    outline: none;
    field-sizing: content;
  }
</style>
