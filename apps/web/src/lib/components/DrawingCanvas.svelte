<script lang="ts">
  import { onMount } from "svelte";
  import { getStroke } from "perfect-freehand";
  import {
    Check,
    X,
    Undo2,
    Redo2,
    Eraser,
    Pen,
    Trash2,
    ImagePlus,
  } from "@lucide/svelte";
  import type { Api } from "$lib/backend/api";
  import {
    bytesToBase64,
    trackBlobUrl,
  } from "@/models/services/attachmentService";
  import {
    enqueueIncrementalAttachmentUpload,
  } from "@/controllers/attachmentController";

  // =========================================================================
  // Types
  // =========================================================================

  interface StrokeData {
    points: number[][]; // [x, y, pressure]
    color: string;
    size: number;
  }

  interface DrawingData {
    v: number;
    strokes: StrokeData[];
    backgroundSvg?: string;
  }

  interface Props {
    src: string;
    width: number;
    height: number;
    entryPath: string;
    api: Api | null;
    onSave: (result: {
      blobUrl: string;
      attachmentPath: string;
      svgWidth: number;
      svgHeight: number;
    }) => void;
    onCancel: () => void;
  }

  let { src, width, height, entryPath, api, onSave, onCancel }: Props =
    $props();

  // =========================================================================
  // State
  // =========================================================================

  let svgElement: SVGSVGElement | undefined = $state();
  // Canvas dimensions start from props but can change when loading SVG metadata.
  // We intentionally capture the initial prop values here.
  let canvasWidth = $state(0);
  let canvasHeight = $state(0);
  $effect(() => {
    if (canvasWidth === 0) canvasWidth = width || 600;
    if (canvasHeight === 0) canvasHeight = height || 300;
  });

  // Strokes
  let strokes: StrokeData[] = $state([]);
  let currentStroke = $state<StrokeData | null>(null);
  let backgroundSvg = $state<string | null>(null);

  // Undo/redo
  let undoStack: StrokeData[][] = $state([]);
  let redoStack: StrokeData[][] = $state([]);

  // Tool state
  let activeTool = $state<"pen" | "eraser">("pen");
  let activeColor = $state("#000000");
  let activeSize = $state(3);
  let isDrawing = $state(false);
  let saving = $state(false);

  const COLORS = [
    "#000000",
    "#e53e3e",
    "#dd6b20",
    "#d69e2e",
    "#38a169",
    "#3182ce",
    "#805ad5",
    "#d53f8c",
  ];

  const SIZES = [
    { label: "S", value: 2 },
    { label: "M", value: 4 },
    { label: "L", value: 8 },
  ];

  // =========================================================================
  // perfect-freehand helpers
  // =========================================================================

  function getStrokeOptions(size: number) {
    return {
      size: size * 2,
      thinning: 0.5,
      smoothing: 0.5,
      streamline: 0.5,
      easing: (t: number) => t,
      start: { taper: 0, cap: true },
      end: { taper: 0, cap: true },
    };
  }

  /**
   * Convert the outline points returned by getStroke into an SVG path `d` attribute.
   */
  function getSvgPathFromStroke(outlinePoints: number[][]): string {
    if (outlinePoints.length === 0) return "";

    const d: string[] = [];
    const [first, ...rest] = outlinePoints;

    d.push(`M ${first[0].toFixed(2)} ${first[1].toFixed(2)}`);

    for (let i = 0; i < rest.length; i++) {
      const [x, y] = rest[i];
      d.push(`L ${x.toFixed(2)} ${y.toFixed(2)}`);
    }

    d.push("Z");
    return d.join(" ");
  }

  function strokeToPath(stroke: StrokeData): string {
    const outline = getStroke(stroke.points, getStrokeOptions(stroke.size));
    return getSvgPathFromStroke(outline);
  }

  // =========================================================================
  // SVG Metadata Parsing
  // =========================================================================

  /**
   * Parse a Diaryx drawing SVG to extract stroke metadata.
   * Returns null if the SVG doesn't contain Diaryx drawing metadata.
   */
  function parseDrawingSvg(svgText: string): DrawingData | null {
    try {
      const parser = new DOMParser();
      const doc = parser.parseFromString(svgText, "image/svg+xml");
      const svg = doc.querySelector("svg");
      if (!svg) return null;

      const metadata = svg.querySelector("metadata");
      if (!metadata) return null;

      const jsonText = metadata.textContent?.trim();
      if (!jsonText) return null;

      const data = JSON.parse(jsonText) as DrawingData;
      if (!data.v || !Array.isArray(data.strokes)) return null;

      // Extract dimensions from viewBox
      const viewBox = svg.getAttribute("viewBox");
      if (viewBox) {
        const parts = viewBox.split(/\s+/).map(Number);
        if (parts.length === 4) {
          canvasWidth = parts[2];
          canvasHeight = parts[3];
        }
      }

      return data;
    } catch {
      return null;
    }
  }

  /**
   * Parse an external (non-Diaryx) SVG and return it as background SVG markup.
   */
  function parseExternalSvg(svgText: string): string | null {
    try {
      const parser = new DOMParser();
      const doc = parser.parseFromString(svgText, "image/svg+xml");
      const svg = doc.querySelector("svg");
      if (!svg) return null;

      // Extract dimensions from the external SVG
      const viewBox = svg.getAttribute("viewBox");
      if (viewBox) {
        const parts = viewBox.split(/\s+/).map(Number);
        if (parts.length === 4) {
          canvasWidth = parts[2];
          canvasHeight = parts[3];
        }
      } else {
        const w = svg.getAttribute("width");
        const h = svg.getAttribute("height");
        if (w && h) {
          canvasWidth = parseFloat(w);
          canvasHeight = parseFloat(h);
        }
      }

      // Return the inner content of the SVG as a background group
      return svg.innerHTML;
    } catch {
      return null;
    }
  }

  // =========================================================================
  // Load existing drawing
  // =========================================================================

  onMount(async () => {
    if (!src || !api) return;

    try {
      // src is a blob URL — fetch the SVG content from it
      const response = await fetch(src);
      const svgText = await response.text();

      const drawingData = parseDrawingSvg(svgText);
      if (drawingData) {
        // Diaryx drawing — load strokes for editing
        strokes = drawingData.strokes;
        if (drawingData.backgroundSvg) {
          backgroundSvg = drawingData.backgroundSvg;
        }
      } else {
        // External SVG — load as background
        const bg = parseExternalSvg(svgText);
        if (bg) {
          backgroundSvg = bg;
        }
      }
    } catch (e) {
      console.warn("[DrawingCanvas] Failed to load existing SVG:", e);
    }
  });

  // =========================================================================
  // Drawing handlers
  // =========================================================================

  function getPointerPos(event: PointerEvent): number[] {
    if (!svgElement) return [0, 0, 0.5];
    const rect = svgElement.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / rect.width) * canvasWidth;
    const y = ((event.clientY - rect.top) / rect.height) * canvasHeight;
    const pressure = event.pressure || 0.5;
    return [x, y, pressure];
  }

  function handlePointerDown(event: PointerEvent) {
    if (activeTool === "eraser") {
      handleEraserDown(event);
      return;
    }

    isDrawing = true;
    const point = getPointerPos(event);
    currentStroke = {
      points: [point],
      color: activeColor,
      size: activeSize,
    };

    svgElement?.setPointerCapture(event.pointerId);
  }

  function handlePointerMove(event: PointerEvent) {
    if (activeTool === "eraser" && isDrawing) {
      handleEraserMove(event);
      return;
    }

    if (!isDrawing || !currentStroke) return;
    const point = getPointerPos(event);
    currentStroke = {
      ...currentStroke,
      points: [...currentStroke.points, point],
    };
  }

  function handlePointerUp(event: PointerEvent) {
    if (activeTool === "eraser") {
      isDrawing = false;
      return;
    }

    if (!isDrawing || !currentStroke) return;
    isDrawing = false;

    // Only add stroke if it has enough points
    if (currentStroke.points.length >= 2) {
      // Save undo state
      undoStack = [...undoStack, [...strokes]];
      redoStack = [];
      strokes = [...strokes, currentStroke];
    }
    currentStroke = null;

    svgElement?.releasePointerCapture(event.pointerId);
  }

  // =========================================================================
  // Eraser
  // =========================================================================

  function handleEraserDown(event: PointerEvent) {
    isDrawing = true;
    const point = getPointerPos(event);
    eraseAt(point[0], point[1]);
    svgElement?.setPointerCapture(event.pointerId);
  }

  function handleEraserMove(event: PointerEvent) {
    if (!isDrawing) return;
    const point = getPointerPos(event);
    eraseAt(point[0], point[1]);
  }

  function eraseAt(x: number, y: number) {
    const eraseRadius = 10;
    const remaining = strokes.filter((stroke) => {
      // Check if any point is close to the eraser
      return !stroke.points.some((p) => {
        const dx = p[0] - x;
        const dy = p[1] - y;
        return dx * dx + dy * dy < eraseRadius * eraseRadius;
      });
    });
    if (remaining.length !== strokes.length) {
      undoStack = [...undoStack, [...strokes]];
      redoStack = [];
      strokes = remaining;
    }
  }

  // =========================================================================
  // Undo / Redo
  // =========================================================================

  function undo() {
    if (undoStack.length === 0) return;
    const prev = undoStack[undoStack.length - 1];
    undoStack = undoStack.slice(0, -1);
    redoStack = [...redoStack, [...strokes]];
    strokes = prev;
  }

  function redo() {
    if (redoStack.length === 0) return;
    const next = redoStack[redoStack.length - 1];
    redoStack = redoStack.slice(0, -1);
    undoStack = [...undoStack, [...strokes]];
    strokes = next;
  }

  function clearAll() {
    if (strokes.length === 0) return;
    undoStack = [...undoStack, [...strokes]];
    redoStack = [];
    strokes = [];
  }

  // =========================================================================
  // Import SVG
  // =========================================================================

  function handleImportSvg() {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".svg,image/svg+xml";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      const text = await file.text();
      const bg = parseExternalSvg(text);
      if (bg) {
        backgroundSvg = bg;
      }
    };
    input.click();
  }

  // =========================================================================
  // Keyboard shortcuts
  // =========================================================================

  function handleKeydown(event: KeyboardEvent) {
    const mod = event.metaKey || event.ctrlKey;

    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    } else if (mod && event.key === "z" && !event.shiftKey) {
      event.preventDefault();
      undo();
    } else if (
      (mod && event.key === "z" && event.shiftKey) ||
      (mod && event.key === "y")
    ) {
      event.preventDefault();
      redo();
    } else if (mod && event.key === "Enter") {
      event.preventDefault();
      handleSave();
    }
  }

  // =========================================================================
  // Save
  // =========================================================================

  function generateSvgString(): string {
    const paths = strokes
      .map((s) => {
        const d = strokeToPath(s);
        return `  <path d="${d}" fill="${s.color}" />`;
      })
      .join("\n");

    const bgGroup = backgroundSvg
      ? `  <g class="background" opacity="0.3">\n    ${backgroundSvg}\n  </g>\n`
      : "";

    const metadata: DrawingData = {
      v: 1,
      strokes,
      ...(backgroundSvg ? { backgroundSvg } : {}),
    };

    return [
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${canvasWidth} ${canvasHeight}" data-diaryx-drawing="1">`,
      `  <rect width="100%" height="100%" fill="transparent" />`,
      bgGroup,
      paths,
      `  <metadata>${JSON.stringify(metadata)}</metadata>`,
      `</svg>`,
    ].join("\n");
  }

  async function handleSave() {
    if (!api || saving) return;
    saving = true;

    try {
      const svgString = generateSvgString();
      const encoder = new TextEncoder();
      const bytes = new Uint8Array(encoder.encode(svgString));
      const dataBase64 = bytesToBase64(bytes);

      // Generate a filename
      const timestamp = Date.now().toString(36);
      const filename = `drawing-${timestamp}.svg`;

      // Upload as attachment
      const attachmentPath = await api.uploadAttachment(
        entryPath,
        filename,
        dataBase64,
      );
      const canonicalPath = await api.canonicalizeLink(
        attachmentPath,
        entryPath,
      );

      // Format the path relative to the entry
      let entryRelativePath: string;
      try {
        entryRelativePath = await api.formatLink(
          canonicalPath,
          filename,
          "plain_relative",
          entryPath,
        );
      } catch {
        entryRelativePath = attachmentPath;
      }

      // Enqueue for sync
      const file = new File([bytes], filename, { type: "image/svg+xml" });
      await enqueueIncrementalAttachmentUpload(
        entryPath,
        canonicalPath,
        file,
      );

      // Create blob URL for display
      const blob = new Blob([svgString], { type: "image/svg+xml" });
      const blobUrl = URL.createObjectURL(blob);
      trackBlobUrl(entryRelativePath, blobUrl);

      onSave({
        blobUrl,
        attachmentPath: entryRelativePath,
        svgWidth: canvasWidth,
        svgHeight: canvasHeight,
      });
    } catch (e) {
      console.error("[DrawingCanvas] Save failed:", e);
    } finally {
      saving = false;
    }
  }

  // =========================================================================
  // Computed paths for rendering
  // =========================================================================

  const renderedPaths: Array<{ d: string; color: string }> = $derived(
    strokes.map((s: StrokeData) => ({
      d: strokeToPath(s),
      color: s.color,
    })),
  );

  const currentPath: { d: string; color: string } | null = $derived(
    currentStroke
      ? { d: strokeToPath(currentStroke), color: currentStroke.color }
      : null,
  );
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="drawing-canvas-container" onkeydown={handleKeydown} tabindex="-1">
  <!-- Toolbar -->
  <div class="drawing-toolbar">
    <div class="toolbar-group">
      <button
        type="button"
        class="toolbar-btn"
        class:active={activeTool === "pen"}
        title="Pen"
        onclick={() => (activeTool = "pen")}
      >
        <Pen class="size-4" />
      </button>
      <button
        type="button"
        class="toolbar-btn"
        class:active={activeTool === "eraser"}
        title="Eraser"
        onclick={() => (activeTool = "eraser")}
      >
        <Eraser class="size-4" />
      </button>
    </div>

    <div class="toolbar-divider"></div>

    <!-- Colors -->
    <div class="toolbar-group colors">
      {#each COLORS as color}
        <button
          type="button"
          class="color-btn"
          class:active={activeColor === color && activeTool === "pen"}
          style="--swatch-color: {color}"
          title={color}
          onclick={() => {
            activeColor = color;
            activeTool = "pen";
          }}
        >
          <span class="color-swatch"></span>
        </button>
      {/each}
    </div>

    <div class="toolbar-divider"></div>

    <!-- Sizes -->
    <div class="toolbar-group">
      {#each SIZES as s}
        <button
          type="button"
          class="toolbar-btn size-btn"
          class:active={activeSize === s.value}
          title="{s.label} stroke"
          onclick={() => (activeSize = s.value)}
        >
          {s.label}
        </button>
      {/each}
    </div>

    <div class="toolbar-divider"></div>

    <!-- Actions -->
    <div class="toolbar-group">
      <button
        type="button"
        class="toolbar-btn"
        title="Undo (Ctrl+Z)"
        disabled={undoStack.length === 0}
        onclick={undo}
      >
        <Undo2 class="size-4" />
      </button>
      <button
        type="button"
        class="toolbar-btn"
        title="Redo (Ctrl+Shift+Z)"
        disabled={redoStack.length === 0}
        onclick={redo}
      >
        <Redo2 class="size-4" />
      </button>
      <button
        type="button"
        class="toolbar-btn"
        title="Import SVG as background"
        onclick={handleImportSvg}
      >
        <ImagePlus class="size-4" />
      </button>
      <button
        type="button"
        class="toolbar-btn danger"
        title="Clear all"
        disabled={strokes.length === 0 && !backgroundSvg}
        onclick={clearAll}
      >
        <Trash2 class="size-4" />
      </button>
    </div>

    <div class="toolbar-spacer"></div>

    <!-- Save / Cancel -->
    <div class="toolbar-group">
      <button
        type="button"
        class="toolbar-btn"
        title="Cancel (Esc)"
        onclick={onCancel}
      >
        <X class="size-4" />
      </button>
      <button
        type="button"
        class="toolbar-btn save-btn"
        title="Save (Ctrl+Enter)"
        disabled={saving || (strokes.length === 0 && !backgroundSvg)}
        onclick={handleSave}
      >
        <Check class="size-4" />
        <span>Save</span>
      </button>
    </div>
  </div>

  <!-- Canvas -->
  <svg
    bind:this={svgElement}
    class="drawing-svg"
    class:eraser-cursor={activeTool === "eraser"}
    viewBox="0 0 {canvasWidth} {canvasHeight}"
    style="aspect-ratio: {canvasWidth} / {canvasHeight}"
    onpointerdown={handlePointerDown}
    onpointermove={handlePointerMove}
    onpointerup={handlePointerUp}
    onpointerleave={handlePointerUp}
  >
    <!-- Background -->
    <rect width="100%" height="100%" fill="white" />

    <!-- Background SVG layer (imported, non-editable) -->
    {#if backgroundSvg}
      <g class="background-layer" opacity="0.3">
        {@html backgroundSvg}
      </g>
    {/if}

    <!-- Completed strokes -->
    {#each renderedPaths as path}
      <path d={path.d} fill={path.color} />
    {/each}

    <!-- Active stroke being drawn -->
    {#if currentPath}
      <path d={currentPath.d} fill={currentPath.color} />
    {/if}
  </svg>
</div>

<style>
  .drawing-canvas-container {
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    outline: none;
  }

  .drawing-canvas-container:focus-within {
    border-color: var(--primary);
    box-shadow: 0 0 0 2px color-mix(in oklch, var(--primary) 20%, transparent);
  }

  .drawing-toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    background: var(--muted);
    border-bottom: 1px solid var(--border);
  }

  .toolbar-group {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .toolbar-group.colors {
    gap: 3px;
  }

  .toolbar-divider {
    width: 1px;
    height: 20px;
    background: var(--border);
    margin: 0 4px;
    opacity: 0.5;
  }

  .toolbar-spacer {
    flex: 1;
  }

  .toolbar-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    width: 30px;
    height: 30px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--foreground);
    cursor: pointer;
    transition: all 0.1s ease;
    -webkit-user-select: none;
    user-select: none;
  }

  .toolbar-btn:hover:not(:disabled) {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .toolbar-btn:active:not(:disabled) {
    transform: scale(0.95);
  }

  .toolbar-btn.active {
    background: var(--primary);
    color: var(--primary-foreground);
  }

  .toolbar-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .toolbar-btn.danger:hover:not(:disabled) {
    background: oklch(0.6 0.2 25);
    color: white;
  }

  .toolbar-btn.save-btn {
    width: auto;
    padding: 0 10px;
    background: var(--primary);
    color: var(--primary-foreground);
    font-size: 12px;
    font-weight: 500;
  }

  .toolbar-btn.save-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .size-btn {
    font-size: 11px;
    font-weight: 600;
  }

  .color-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 50%;
    background: transparent;
    cursor: pointer;
    transition: border-color 0.1s ease;
  }

  .color-btn.active {
    border-color: var(--primary);
  }

  .color-swatch {
    display: block;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--swatch-color);
  }

  .drawing-svg {
    display: block;
    width: 100%;
    max-height: 60vh;
    cursor: crosshair;
    touch-action: none;
  }

  .drawing-svg.eraser-cursor {
    cursor:
      url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='20' height='20'%3E%3Ccircle cx='10' cy='10' r='8' fill='none' stroke='%23666' stroke-width='1.5'/%3E%3C/svg%3E")
        10 10,
      crosshair;
  }

  @media (max-width: 767px) {
    .drawing-toolbar {
      gap: 2px;
      padding: 4px;
    }

    .toolbar-btn {
      width: 34px;
      height: 34px;
    }

    .toolbar-divider {
      margin: 0 2px;
    }

    .color-btn {
      width: 26px;
      height: 26px;
    }

    .color-swatch {
      width: 16px;
      height: 16px;
    }
  }
</style>
