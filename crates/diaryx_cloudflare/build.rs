//! Parse wrangler.jsonc at compile time and emit binding name constants.

use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let wrangler_path = Path::new(&manifest_dir).join("wrangler.jsonc");

    println!("cargo:rerun-if-changed=wrangler.jsonc");

    let raw = fs::read_to_string(&wrangler_path).expect("failed to read wrangler.jsonc");

    // Strip single-line comments (// ...) to turn JSONC into valid JSON.
    let json_str: String = raw
        .lines()
        .map(|line| {
            // Naive but sufficient for wrangler config: strip everything after
            // a `//` that isn't inside a quoted string. Since our values never
            // contain `//`, a simple "outside quotes" check works.
            let mut in_string = false;
            let mut prev = ' ';
            for (i, ch) in line.char_indices() {
                if ch == '"' && prev != '\\' {
                    in_string = !in_string;
                }
                if !in_string && ch == '/' && prev == '/' {
                    return &line[..i - 1];
                }
                prev = ch;
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");

    let config: serde_json::Value =
        serde_json::from_str(&json_str).expect("failed to parse wrangler.jsonc as JSON");

    let d1_binding = config["d1_databases"][0]["binding"]
        .as_str()
        .expect("missing d1_databases[0].binding");
    let r2_binding = config["r2_buckets"][0]["binding"]
        .as_str()
        .expect("missing r2_buckets[0].binding");
    let kv_binding = config["kv_namespaces"][0]["binding"]
        .as_str()
        .expect("missing kv_namespaces[0].binding");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("wrangler_bindings.rs");

    fs::write(
        &dest,
        format!(
            r#"pub const D1_BINDING: &str = "{d1_binding}";
pub const R2_BINDING: &str = "{r2_binding}";
pub const KV_BINDING: &str = "{kv_binding}";
"#
        ),
    )
    .expect("failed to write wrangler_bindings.rs");
}
