/**
 * Browser WASI runner — executes arbitrary WASI modules using @bjorn3/browser_wasi_shim.
 *
 * This is the browser counterpart to `diaryx_extism::wasi_runner` on native.
 * Guest plugins call `host_run_wasi_module` with a storage key, arguments,
 * optional stdin, virtual filesystem files, and output file paths.
 * The host loads the WASM bytes from plugin storage, runs the module,
 * and captures stdout/stderr/output files.
 */

// Re-export the @bjorn3/browser_wasi_shim types we use
import {
  File,
  OpenFile,
  PreopenDirectory,
  WASI,
} from '@bjorn3/browser_wasi_shim';

export interface WasiRunRequest {
  module_key: string;
  args: string[];
  stdin?: string;                          // base64
  files?: Record<string, string>;          // path → base64
  output_files?: string[];
}

export interface WasiRunResult {
  exit_code: number;
  stdout: string;                          // base64
  stderr: string;                          // UTF-8 text
  files?: Record<string, string>;          // path → base64
}

/**
 * Run a WASI module in the browser.
 *
 * @param wasmBytes - The raw WASM binary
 * @param args - CLI arguments (argv[0] should be the program name)
 * @param stdinBytes - Optional stdin data
 * @param inputFiles - Virtual filesystem files (path → data)
 * @param outputFilePaths - Paths of output files to capture
 */
export async function runWasiModule(
  wasmBytes: ArrayBuffer,
  args: string[],
  stdinBytes?: Uint8Array,
  inputFiles?: Map<string, Uint8Array>,
  outputFilePaths?: string[],
): Promise<WasiRunResult> {
  // Build the virtual filesystem
  const fileSystem = new Map<string, File>();

  // stdin
  const stdinFile = new File(stdinBytes ?? new Uint8Array(), { readonly: true });

  // stdout/stderr capture
  let stdoutData = new Uint8Array();
  let stderrData = new Uint8Array();

  const stdoutFile = new File(new Uint8Array(), { readonly: false });
  const stderrFile = new File(new Uint8Array(), { readonly: false });

  // Add input files to the virtual FS
  if (inputFiles) {
    for (const [path, data] of inputFiles) {
      fileSystem.set(path.replace(/^\//, ''), new File(data, { readonly: true }));
    }
  }

  // Add placeholder output files
  if (outputFilePaths) {
    for (const path of outputFilePaths) {
      const cleanPath = path.replace(/^\//, '');
      if (!fileSystem.has(cleanPath)) {
        fileSystem.set(cleanPath, new File(new Uint8Array(), { readonly: false }));
      }
    }
  }

  // Set up WASI file descriptors:
  // fd 0 = stdin, fd 1 = stdout, fd 2 = stderr, fd 3 = preopened "/"
  const fds = [
    new OpenFile(stdinFile),
    new OpenFile(stdoutFile),
    new OpenFile(stderrFile),
    new PreopenDirectory('/', fileSystem),
  ];

  const fullArgs = ['program', ...args];
  const wasi = new WASI(fullArgs, [], fds, { debug: false });

  // Compile and instantiate
  const module = await WebAssembly.compile(wasmBytes);
  const instance = await WebAssembly.instantiate(module, {
    wasi_snapshot_preview1: wasi.wasiImport,
  });

  // Run the module
  let exitCode = 0;
  try {
    wasi.initialize(instance as any);
    // Call _start if it exists
    const start = (instance.exports as any)._start;
    if (typeof start === 'function') {
      start();
    }
  } catch (e: any) {
    // WASI proc_exit throws with the exit code
    if (e instanceof Error && e.message?.includes('exit')) {
      // Try to extract exit code
      const match = e.message.match(/exit.*?(\d+)/i);
      exitCode = match ? parseInt(match[1], 10) : 1;
    } else if (typeof e === 'object' && 'code' in e) {
      exitCode = (e as any).code;
    } else {
      throw e;
    }
  }

  // Capture stdout
  stdoutData = new Uint8Array(stdoutFile.data);

  // Capture stderr
  stderrData = new Uint8Array(stderrFile.data);

  // Base64 encode stdout
  const stdoutB64 = uint8ArrayToBase64(stdoutData);

  // Decode stderr as UTF-8 text
  const stderrText = new TextDecoder().decode(stderrData);

  // Capture output files
  let capturedFiles: Record<string, string> | undefined;
  if (outputFilePaths && outputFilePaths.length > 0) {
    capturedFiles = {};
    for (const path of outputFilePaths) {
      const cleanPath = path.replace(/^\//, '');
      const file = fileSystem.get(cleanPath);
      if (file && file.data && file.data.length > 0) {
        capturedFiles[path] = uint8ArrayToBase64(file.data);
      }
    }
    if (Object.keys(capturedFiles).length === 0) {
      capturedFiles = undefined;
    }
  }

  return {
    exit_code: exitCode,
    stdout: stdoutB64,
    stderr: stderrText,
    files: capturedFiles,
  };
}

/** Convert a Uint8Array to a base64 string. */
function uint8ArrayToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

/** Convert a base64 string to a Uint8Array. */
function base64ToUint8Array(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/**
 * Handle a `host_run_wasi_module` call from an Extism guest plugin.
 *
 * Loads WASM bytes from localStorage (plugin storage), decodes base64 inputs,
 * runs the WASI module, and returns the result.
 */
export async function handleHostRunWasiModule(
  request: WasiRunRequest,
): Promise<WasiRunResult> {
  // Load WASM bytes from plugin storage (localStorage)
  const storageKey = `diaryx-plugin:${request.module_key}`;
  const raw = localStorage.getItem(storageKey);
  if (!raw) {
    return {
      exit_code: -1,
      stdout: '',
      stderr: `Module not found in storage: ${request.module_key}`,
    };
  }

  // Parse the stored data (same format as host_storage_get: {data: base64})
  let wasmB64: string;
  try {
    const parsed = JSON.parse(raw);
    wasmB64 = parsed.data;
  } catch {
    return {
      exit_code: -1,
      stdout: '',
      stderr: `Invalid storage format for module: ${request.module_key}`,
    };
  }

  const wasmBytes = base64ToUint8Array(wasmB64).buffer as ArrayBuffer;

  // Decode stdin
  const stdinBytes = request.stdin ? base64ToUint8Array(request.stdin) : undefined;

  // Decode input files
  let inputFiles: Map<string, Uint8Array> | undefined;
  if (request.files) {
    inputFiles = new Map();
    for (const [path, b64] of Object.entries(request.files)) {
      inputFiles.set(path, base64ToUint8Array(b64));
    }
  }

  return runWasiModule(
    wasmBytes,
    request.args,
    stdinBytes,
    inputFiles,
    request.output_files,
  );
}
