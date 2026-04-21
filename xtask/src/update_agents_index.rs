use crate::util::{diaryx_app, workspace_root};
use chrono::Utc;
use diaryx_core::fs::SyncToAsyncFs;
use diaryx_core::workspace::Workspace;
use diaryx_native::RealFileSystem;
use std::fs;

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let agents = root.join("AGENTS.md");
    let readme = root.join("README.md");
    let agents_str = agents
        .to_str()
        .ok_or_else(|| format!("non-UTF8 path: {}", agents.display()))?;

    // Build the workspace tree the same way `diaryx workspace info --depth 0
    // --properties title,description,path` does (delimiter defaults to " - ").
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    let properties = vec![
        "title".to_string(),
        "description".to_string(),
        "path".to_string(),
    ];
    let tree = futures_lite::future::block_on(ws.workspace_info_with_properties(
        &readme,
        None,
        &properties,
        " - ",
    ))
    .map_err(|e| format!("workspace info: {e}"))?;

    let original =
        fs::read_to_string(&agents).map_err(|e| format!("read {}: {e}", agents.display()))?;
    let updated = splice_workspace_index(&original, &tree)?;

    if updated == original {
        return Ok(());
    }

    fs::write(&agents, &updated).map_err(|e| format!("write {}: {e}", agents.display()))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    // Mirror `diaryx property set`: parse the value through serde_yaml_ng so it
    // round-trips as whatever scalar type YAML infers (string here).
    let yaml_now: diaryx_core::YamlValue =
        serde_yaml_ng::from_str(&now).map_err(|e| format!("parse timestamp as YAML: {e}"))?;
    diaryx_app()
        .set_frontmatter_property(agents_str, "updated", yaml_now)
        .map_err(|e| format!("set AGENTS.md updated: {e}"))?;

    println!("Updated AGENTS.md workspace index");
    Ok(())
}

fn splice_workspace_index(text: &str, tree: &str) -> Result<String, String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let open_idx = lines
        .iter()
        .position(|l| *l == "```workspace-index")
        .ok_or_else(|| "no ```workspace-index fence in AGENTS.md".to_string())?;
    let close_rel = lines[open_idx + 1..]
        .iter()
        .position(|l| *l == "```")
        .ok_or_else(|| "no closing ``` fence after workspace-index".to_string())?;
    let close_idx = open_idx + 1 + close_rel;

    let mut out = String::with_capacity(text.len());
    for line in &lines[..=open_idx] {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(tree.trim_end_matches('\n'));
    out.push('\n');
    for (i, line) in lines[close_idx..].iter().enumerate() {
        out.push_str(line);
        if i + 1 < lines.len() - close_idx {
            out.push('\n');
        }
    }
    Ok(out)
}
