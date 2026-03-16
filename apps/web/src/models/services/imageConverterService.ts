/**
 * Image Converter Service
 *
 * Registry for plugin-provided media transcoders. Plugins that declare the
 * `MediaTranscoder` capability are registered here so attachment handling
 * code can convert unsupported formats (e.g. HEIC) on the fly.
 *
 * Wire format (binary, 8-byte header + payload):
 *
 * **Input**
 * ```
 * [u8  output_format]   0=JPEG, 1=PNG, 2=WebP
 * [u8  quality]         0–100
 * [u8  reserved]
 * [u8  reserved]
 * [u32 payload_len]     little-endian
 * [... raw source bytes]
 * ```
 *
 * **Output**
 * ```
 * [u8  status]          0=ok, 1=unsupported_format, 2=decode_error, 3=encode_error
 * [u8  output_format]   echo back
 * [u16 reserved]
 * [u32 payload_len]     little-endian
 * [... converted bytes (or UTF-8 error on failure)]
 * ```
 */

import type { BrowserExtismPlugin } from "$lib/plugins/extismBrowserLoader";

// ============================================================================
// Types
// ============================================================================

interface RegisteredTranscoder {
  pluginId: string;
  plugin: BrowserExtismPlugin;
}

const OUTPUT_FORMAT_JPEG = 0;
const OUTPUT_FORMAT_PNG = 1;
const OUTPUT_FORMAT_WEBP = 2;

const STATUS_OK = 0;

// ============================================================================
// State
// ============================================================================

/** Map of conversion string (e.g. "heic:jpeg") -> transcoder */
const transcoders = new Map<string, RegisteredTranscoder>();

// ============================================================================
// Wire format helpers
// ============================================================================

function encodeOutputFormat(format: string): number {
  switch (format.toLowerCase()) {
    case "jpeg":
    case "jpg":
      return OUTPUT_FORMAT_JPEG;
    case "png":
      return OUTPUT_FORMAT_PNG;
    case "webp":
      return OUTPUT_FORMAT_WEBP;
    default:
      return OUTPUT_FORMAT_JPEG;
  }
}

function encodeRequest(
  inputBytes: Uint8Array,
  outputFormat: string,
  quality: number,
): Uint8Array {
  const header = new ArrayBuffer(8);
  const view = new DataView(header);
  view.setUint8(0, encodeOutputFormat(outputFormat));
  view.setUint8(1, Math.min(100, Math.max(0, Math.round(quality))));
  view.setUint8(2, 0); // reserved
  view.setUint8(3, 0); // reserved
  view.setUint32(4, inputBytes.byteLength, true); // little-endian

  const result = new Uint8Array(8 + inputBytes.byteLength);
  result.set(new Uint8Array(header), 0);
  result.set(inputBytes, 8);
  return result;
}

function decodeResponse(output: Uint8Array): {
  ok: boolean;
  status: number;
  payload: Uint8Array;
} {
  if (output.byteLength < 8) {
    return { ok: false, status: 255, payload: new Uint8Array() };
  }
  const view = new DataView(
    output.buffer,
    output.byteOffset,
    output.byteLength,
  );
  const status = view.getUint8(0);
  const payloadLen = view.getUint32(4, true);
  const payload = output.subarray(8, 8 + payloadLen);
  return { ok: status === STATUS_OK, status, payload };
}

// ============================================================================
// Public API
// ============================================================================

/**
 * Register a transcoder plugin for the given conversion pairs.
 */
export function registerTranscoder(
  pluginId: string,
  plugin: BrowserExtismPlugin,
  conversions: string[],
): void {
  for (const conversion of conversions) {
    transcoders.set(conversion, { pluginId, plugin });
  }
}

/**
 * Remove all transcoders registered by a specific plugin.
 */
export function unregisterTranscodersByPlugin(pluginId: string): void {
  for (const [key, entry] of transcoders) {
    if (entry.pluginId === pluginId) {
      transcoders.delete(key);
    }
  }
}

/**
 * Remove all registered transcoders.
 */
export function clearAllTranscoders(): void {
  transcoders.clear();
}

/**
 * Check if a conversion is available.
 */
export function canConvert(sourceExt: string, targetFormat: string): boolean {
  const key = `${sourceExt.toLowerCase()}:${targetFormat.toLowerCase()}`;
  return transcoders.has(key);
}

/**
 * Convert raw image bytes using the registered transcoder plugin.
 * Returns null if no transcoder is available or conversion fails.
 */
export async function convertImage(
  inputBytes: Uint8Array,
  sourceExt: string,
  targetFormat: string,
  quality: number = 92,
): Promise<Uint8Array | null> {
  const key = `${sourceExt.toLowerCase()}:${targetFormat.toLowerCase()}`;
  const transcoder = transcoders.get(key);
  if (!transcoder) return null;

  const request = encodeRequest(inputBytes, targetFormat, quality);
  const output = await transcoder.plugin.callBinary(
    "transcode_image",
    request,
  );
  if (!output) return null;

  const response = decodeResponse(output);
  if (!response.ok) {
    const errorMsg = new TextDecoder().decode(response.payload);
    console.warn(
      `[imageConverterService] Transcoding failed (status=${response.status}): ${errorMsg}`,
    );
    return null;
  }

  return response.payload;
}

/**
 * Convert a Blob image using the registered transcoder plugin.
 *
 * Drop-in replacement for the old `convertHeicToJpeg`. Returns the original
 * blob if no converter is available or conversion fails.
 */
export async function convertBlobImage(
  blob: Blob,
  sourcePath: string,
  targetFormat: string = "jpeg",
  quality: number = 92,
): Promise<Blob> {
  const ext = sourcePath.split(".").pop()?.toLowerCase() ?? "";
  if (!canConvert(ext, targetFormat)) {
    return blob;
  }

  try {
    const inputBytes = new Uint8Array(await blob.arrayBuffer());
    const outputBytes = await convertImage(
      inputBytes,
      ext,
      targetFormat,
      quality,
    );
    if (!outputBytes) return blob;

    const mimeMap: Record<string, string> = {
      jpeg: "image/jpeg",
      jpg: "image/jpeg",
      png: "image/png",
      webp: "image/webp",
    };
    const mime = mimeMap[targetFormat.toLowerCase()] ?? "image/jpeg";
    return new Blob([outputBytes.buffer as ArrayBuffer], { type: mime });
  } catch (e) {
    console.warn("[imageConverterService] Blob conversion failed:", e);
    return blob;
  }
}
