/**
 * Recursively coerce BigInt values to plain numbers.
 *
 * The browser (wasm) backend deserializes YAML integers as JavaScript `BigInt`
 * (serde-wasm-bindgen maps Rust `i64`/`u64` → `BigInt`), whereas the native and
 * extism backends round-trip through JSON and yield plain `number`s. Registry
 * validators check `typeof value === "number"`, so an integer field such as
 * `artifact.size`, `file_count`, or `baseFontSize` would be rejected on the web
 * (every entry silently dropped). Normalizing parsed frontmatter through this
 * helper makes both backends behave identically.
 *
 * Registry integers (byte sizes, counts, font sizes, schema versions) are far
 * below `Number.MAX_SAFE_INTEGER`, so the conversion is lossless here.
 */
export function coerceBigIntsToNumbers<T>(value: T): T {
  if (typeof value === "bigint") {
    return Number(value) as unknown as T;
  }
  if (Array.isArray(value)) {
    return value.map((item) => coerceBigIntsToNumbers(item)) as unknown as T;
  }
  if (value !== null && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(value)) {
      out[key] = coerceBigIntsToNumbers(val);
    }
    return out as T;
  }
  return value;
}
