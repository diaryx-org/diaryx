/**
 * Typst WASM compilation service for the web app.
 *
 * Manages a Web Worker that lazily loads the Typst compiler (~28 MB WASM).
 * Provides a promise-based API for compiling Typst source to PDF.
 */

export class TypstService {
  private worker: Worker | null = null;
  private ready = false;
  private pendingRequests = new Map<
    number,
    { resolve: (value: Uint8Array) => void; reject: (reason: Error) => void }
  >();
  private nextId = 0;
  private initPromise: Promise<void> | null = null;

  /** Ensure the worker is loaded and ready. */
  async ensureReady(): Promise<void> {
    if (this.ready) return;
    if (this.initPromise) return this.initPromise;

    this.initPromise = new Promise<void>((resolve, reject) => {
      this.worker = new Worker(new URL('./typstWorker.ts', import.meta.url), {
        type: 'module',
      });

      const onMessage = (e: MessageEvent) => {
        const msg = e.data;

        if (msg.type === 'ready') {
          this.ready = true;
          resolve();
          return;
        }

        if (msg.type === 'error' && !this.ready) {
          reject(new Error(msg.error));
          return;
        }

        if (msg.type === 'result' || msg.type === 'error') {
          const pending = this.pendingRequests.get(msg.id);
          if (pending) {
            this.pendingRequests.delete(msg.id);
            if (msg.type === 'result') {
              pending.resolve(msg.pdf);
            } else {
              pending.reject(new Error(msg.error));
            }
          }
        }
      };

      this.worker.onmessage = onMessage;
      this.worker.onerror = (e) => {
        if (!this.ready) {
          reject(new Error(`Worker error: ${e.message}`));
        }
      };

      this.worker.postMessage({ type: 'init' });
    });

    return this.initPromise;
  }

  /**
   * Compile Typst source to PDF.
   *
   * @param source - Typst markup source text.
   * @returns PDF file contents as Uint8Array.
   */
  async compile(source: string): Promise<Uint8Array> {
    await this.ensureReady();

    const id = this.nextId++;

    return new Promise<Uint8Array>((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });
      this.worker!.postMessage({ type: 'compile', id, source });
    });
  }

  /** Terminate the worker and free resources. */
  dispose() {
    this.worker?.terminate();
    this.worker = null;
    this.ready = false;
    this.initPromise = null;
    this.pendingRequests.clear();
  }
}
