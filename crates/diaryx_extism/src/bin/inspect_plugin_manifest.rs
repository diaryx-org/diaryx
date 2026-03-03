use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let wasm_path = match args.next() {
        Some(path) => PathBuf::from(path),
        None => {
            eprintln!("Usage: inspect_plugin_manifest <path-to-plugin.wasm>");
            std::process::exit(2);
        }
    };

    match diaryx_extism::inspect_plugin_wasm_manifest(&wasm_path) {
        Ok(manifest) => match serde_json::to_string_pretty(&manifest) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Failed to serialize manifest: {e}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Failed to inspect plugin manifest: {e}");
            std::process::exit(1);
        }
    }
}
