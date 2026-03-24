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

    // Bindings can be at top level or inside an env. Since they're the same
    // across environments (only bucket names / remote flags differ), check
    // top level first, then fall back to the first env that has them.
    let d1_binding = find_binding(&config, "d1_databases")
        .expect("missing d1_databases[0].binding in top level or any env");
    let r2_binding = find_binding(&config, "r2_buckets")
        .expect("missing r2_buckets[0].binding in top level or any env");
    let kv_binding = find_binding(&config, "kv_namespaces")
        .expect("missing kv_namespaces[0].binding in top level or any env");

    fn find_binding(config: &serde_json::Value, key: &str) -> Option<String> {
        // Try top level first
        if let Some(s) = config[key][0]["binding"].as_str() {
            return Some(s.to_string());
        }
        // Fall back to first env that has it
        if let Some(envs) = config["env"].as_object() {
            for (_name, env_config) in envs {
                if let Some(s) = env_config[key][0]["binding"].as_str() {
                    return Some(s.to_string());
                }
            }
        }
        None
    }

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
