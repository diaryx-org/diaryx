use crate::util::workspace_root;
use std::env;
use std::fs;
use std::io::Read;

const REGISTRIES: &[&str] = &[
    "plugins/registry.md",
    "bundles/registry.md",
    "themes/registry.md",
    "typographies/registry.md",
    "templates/registry.md",
    "starter-workspaces/registry.md",
];

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let cdn = env::var("CDN_ORIGIN").unwrap_or_else(|_| "https://app.diaryx.org".to_string());
    let dist = root.join("apps/web/marketplace-dist");

    println!(
        "Syncing marketplace registries from {cdn}/cdn → {}",
        dist.display()
    );

    for reg in REGISTRIES {
        let url = format!("{cdn}/cdn/{reg}");
        let dest = dist.join(reg);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }

        // Padded label like the original script (40-char left-align).
        let label = format!("{reg:<40}");
        print!("  {label} ");

        match fetch(&url) {
            Ok(bytes) => {
                fs::write(&dest, &bytes).map_err(|e| format!("write {}: {e}", dest.display()))?;
                println!("OK");
            }
            Err(err) => {
                println!("{err}");
                // Match the old script: remove any partial file if the fetch
                // failed so stale content doesn't linger.
                let _ = fs::remove_file(&dest);
            }
        }
    }

    println!("Done. Run 'bun run dev' from apps/web to serve locally.");
    Ok(())
}

/// Fetches a URL. On non-200 or transport errors, returns a short `SKIP (…)`
/// label matching the old script's output.
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    match ureq::get(url).call() {
        Ok(response) => {
            let status = response.status();
            if status.as_u16() != 200 {
                return Err(format!("SKIP ({})", status.as_u16()));
            }
            let mut body = Vec::new();
            response
                .into_body()
                .into_reader()
                .read_to_end(&mut body)
                .map_err(|e| format!("FAIL (read: {e})"))?;
            Ok(body)
        }
        Err(e) => Err(format!("SKIP ({e})")),
    }
}
