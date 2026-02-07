/**
 * Web Worker for Typst WASM compilation (typst source → PDF).
 *
 * Uses @myriaddreamin/typst.ts to compile Typst markup into PDF bytes.
 * The compiler WASM module (~28 MB) is loaded lazily on first use.
 *
 * Messages IN:
 *   { type: 'init' }
 *   { type: 'compile', id: number, source: string }
 *
 * Messages OUT:
 *   { type: 'ready' }
 *   { type: 'result', id: number, pdf: Uint8Array }
 *   { type: 'error', id?: number, error: string }
 */

// Use ?url to get the WASM binary URL as a static asset (same pattern as pandocWorker.ts)
// @ts-expect-error - Vite ?url import
import wasmUrl from '/node_modules/@myriaddreamin/typst-ts-web-compiler/pkg/typst_ts_web_compiler_bg.wasm?url';

let typst: any = null;

async function initTypst() {
  const { TypstSnippet } = await import(
    '@myriaddreamin/typst.ts/dist/esm/contrib/snippet.mjs'
  );

  typst = new TypstSnippet();

  typst.setCompilerInitOptions({
    getModule: () => fetch(wasmUrl),
  });

  // Warm up: compile an empty document to fully initialize the compiler
  await typst.pdf({ mainContent: '' });

  self.postMessage({ type: 'ready' });
}

self.onmessage = async (e: MessageEvent) => {
  const { type, id, ...params } = e.data;

  if (type === 'init') {
    try {
      await initTypst();
    } catch (err) {
      self.postMessage({
        type: 'error',
        error: `Failed to load Typst WASM: ${err}`,
      });
    }
    return;
  }

  if (type === 'compile') {
    if (!typst) {
      try {
        await initTypst();
      } catch (err) {
        self.postMessage({
          type: 'error',
          id,
          error: `Failed to load Typst WASM: ${err}`,
        });
        return;
      }
    }

    try {
      const pdf: Uint8Array | undefined = await typst.pdf({
        mainContent: params.source,
      });
      if (!pdf) {
        self.postMessage({
          type: 'error',
          id,
          error: 'Typst compilation produced no output',
        });
        return;
      }
      self.postMessage(
        { type: 'result', id, pdf },
        { transfer: [pdf.buffer] },
      );
    } catch (err) {
      self.postMessage({
        type: 'error',
        id,
        error: `Typst compilation failed: ${err}`,
      });
    }
  }
};
