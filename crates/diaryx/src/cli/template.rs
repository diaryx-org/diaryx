//! Template command handlers — routes to the templating plugin via CLI plugin context.

use crate::cli::args::TemplateCommands;

/// Handle template subcommands.
/// Returns true on success, false on error.
pub fn handle_template_command(command: TemplateCommands) -> bool {
    #[cfg(not(feature = "plugins"))]
    {
        let _ = command;
        eprintln!("Template commands require the 'plugins' feature.");
        false
    }

    #[cfg(feature = "plugins")]
    plugin_impl::handle_template_command_impl(command)
}

#[cfg(feature = "plugins")]
mod plugin_impl {
    use std::io::{self, Write};
    use std::path::Path;

    use diaryx_core::config::Config;
    use diaryx_native::NativeConfigExt;
    use serde_json::Value as JsonValue;

    use crate::cli::args::TemplateCommands;
    use crate::cli::plugin_loader::CliPluginContext;
    use crate::editor::launch_editor;

    const PLUGIN_ID: &str = "diaryx.templating";

    pub fn handle_template_command_impl(command: TemplateCommands) -> bool {
        let config = Config::load().ok();
        let workspace_root = config
            .as_ref()
            .map(|c| c.default_workspace.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));

        let ctx = match CliPluginContext::load(&workspace_root, PLUGIN_ID) {
            Ok(ctx) => ctx,
            Err(e) => {
                eprintln!("Failed to load templating plugin: {}", e);
                eprintln!("Is the diaryx.templating plugin installed?");
                return false;
            }
        };

        match command {
            TemplateCommands::List { paths } => handle_list(&ctx, paths),
            TemplateCommands::Show { name } => handle_show(&ctx, &name),
            TemplateCommands::New { name, from, edit } => {
                handle_new(&ctx, &name, from.as_deref(), edit, config.as_ref())
            }
            TemplateCommands::Edit { name } => handle_edit(&ctx, &name, config.as_ref()),
            TemplateCommands::Delete { name, yes } => handle_delete(&ctx, &name, yes),
            TemplateCommands::Path => handle_path(&ctx),
            TemplateCommands::Variables => handle_variables(&ctx),
        }
    }

    fn handle_list(ctx: &CliPluginContext, show_paths: bool) -> bool {
        match ctx.cmd("ListTemplates", serde_json::json!({})) {
            Ok(data) => {
                let templates = data.as_array().cloned().unwrap_or_default();
                if templates.is_empty() {
                    println!("No templates found.");
                    return true;
                }

                println!("Available templates:\n");
                for tmpl in &templates {
                    let name = tmpl.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let source = tmpl.get("source").and_then(|v| v.as_str()).unwrap_or("?");
                    println!("  {} [{}]", name, source);
                    if show_paths {
                        if let Ok(info) =
                            ctx.cmd("GetTemplatePath", serde_json::json!({ "name": name }))
                        {
                            let path_str = info
                                .get("path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("(built-in)");
                            println!("    {}", path_str);
                        }
                    }
                }
                println!();
                println!("Use 'diaryx template show <name>' to view a template's contents.");
                true
            }
            Err(e) => {
                eprintln!("Failed to list templates: {}", e);
                false
            }
        }
    }

    fn handle_show(ctx: &CliPluginContext, name: &str) -> bool {
        match ctx.cmd("GetTemplate", serde_json::json!({ "name": name })) {
            Ok(data) => {
                let content = data.as_str().unwrap_or("");
                println!("Template: {}\n", name);
                println!("{}", content);
                true
            }
            Err(e) => {
                eprintln!("Template not found: {}", e);
                eprintln!("  Use 'diaryx template list' to see available templates.");
                false
            }
        }
    }

    fn handle_new(
        ctx: &CliPluginContext,
        name: &str,
        from: Option<&str>,
        edit: bool,
        config: Option<&Config>,
    ) -> bool {
        let content = if let Some(source_name) = from {
            match ctx.cmd("GetTemplate", serde_json::json!({ "name": source_name })) {
                Ok(data) => data.as_str().unwrap_or("").to_string(),
                Err(e) => {
                    eprintln!("Source template not found: {}", e);
                    return false;
                }
            }
        } else {
            default_template_content()
        };

        match ctx.cmd(
            "SaveTemplate",
            serde_json::json!({ "name": name, "content": content }),
        ) {
            Ok(_) => {
                println!("Created template: {}", name);
                if edit {
                    if let Ok(info) =
                        ctx.cmd("GetTemplatePath", serde_json::json!({ "name": name }))
                    {
                        if let Some(path_str) = info.get("path").and_then(|v| v.as_str()) {
                            if let Some(cfg) = config {
                                println!("Opening in editor...");
                                if let Err(e) = launch_editor(Path::new(path_str), cfg) {
                                    eprintln!("Error launching editor: {}", e);
                                    return false;
                                }
                            } else {
                                eprintln!("No config found, cannot open editor.");
                            }
                        }
                    }
                } else {
                    println!("  Use 'diaryx template edit {}' to customize it.", name);
                }
                true
            }
            Err(e) => {
                eprintln!("Error creating template: {}", e);
                false
            }
        }
    }

    fn handle_edit(ctx: &CliPluginContext, name: &str, config: Option<&Config>) -> bool {
        match ctx.cmd("GetTemplatePath", serde_json::json!({ "name": name })) {
            Ok(info) => {
                let source = info
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if source == "builtin" {
                    eprintln!("Cannot edit built-in template '{}' directly.", name);
                    eprintln!(
                        "  Use 'diaryx template new {} --from {}' to create an editable copy.",
                        name, name
                    );
                    return false;
                }
                if let Some(path_str) = info.get("path").and_then(|v| v.as_str()) {
                    if let Some(cfg) = config {
                        println!("Opening: {}", path_str);
                        if let Err(e) = launch_editor(Path::new(path_str), cfg) {
                            eprintln!("Error launching editor: {}", e);
                            return false;
                        }
                        true
                    } else {
                        eprintln!("No config found, cannot determine editor.");
                        eprintln!("  Template file: {}", path_str);
                        false
                    }
                } else {
                    eprintln!("Template path not found.");
                    false
                }
            }
            Err(e) => {
                eprintln!("Template not found: {}", e);
                eprintln!("  Use 'diaryx template list' to see available templates.");
                false
            }
        }
    }

    fn handle_delete(ctx: &CliPluginContext, name: &str, yes: bool) -> bool {
        let info = match ctx.cmd("GetTemplatePath", serde_json::json!({ "name": name })) {
            Ok(info) => info,
            Err(e) => {
                eprintln!("Template not found: {}", e);
                eprintln!("  Use 'diaryx template list' to see available templates.");
                return false;
            }
        };

        let source = info
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        if source == "builtin" {
            eprintln!("Cannot delete built-in template '{}'.", name);
            return false;
        }

        let path_str = info.get("path").and_then(|v| v.as_str()).unwrap_or(name);

        if !yes {
            print!("Delete template '{}' at {}? [y/N] ", name, path_str);
            io::stdout().flush().unwrap();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                eprintln!("Failed to read input");
                return false;
            }
            let input = input.trim().to_lowercase();
            if input != "y" && input != "yes" {
                println!("Cancelled.");
                return true;
            }
        }

        match ctx.cmd("DeleteTemplate", serde_json::json!({ "name": name })) {
            Ok(_) => {
                println!("Deleted template: {}", name);
                true
            }
            Err(e) => {
                eprintln!("Error deleting template: {}", e);
                false
            }
        }
    }

    fn handle_path(ctx: &CliPluginContext) -> bool {
        match ctx.cmd("GetTemplatePaths", serde_json::json!({})) {
            Ok(data) => {
                println!("Template directories:\n");
                if let Some(dir) = data.get("workspace_templates_dir").and_then(|v| v.as_str()) {
                    let exists = Path::new(dir).exists();
                    let status = if exists { "+" } else { "o" };
                    println!("  {} Workspace: {}", status, dir);
                }
                println!("  + Built-in:  (compiled into plugin)");
                println!();
                println!("Legend: + = exists, o = does not exist");
                true
            }
            Err(e) => {
                eprintln!("Failed to get template paths: {}", e);
                false
            }
        }
    }

    fn handle_variables(ctx: &CliPluginContext) -> bool {
        match ctx.cmd("GetTemplateVariables", serde_json::json!({})) {
            Ok(data) => {
                println!("Available template variables:\n");
                if let JsonValue::Array(vars) = &data {
                    for var in vars {
                        if let JsonValue::Array(pair) = var {
                            let name = pair.first().and_then(|v| v.as_str()).unwrap_or("?");
                            let desc = pair.get(1).and_then(|v| v.as_str()).unwrap_or("");
                            println!("  {{{{{}}}}}  ", name);
                            println!("      {}", desc);
                            println!();
                        }
                    }
                }
                println!("Custom format examples:");
                println!("  {{{{date:%B %d, %Y}}}}     -> \"January 15, 2024\"");
                println!("  {{{{time:%H:%M:%S}}}}      -> \"14:30:45\"");
                println!("  {{{{datetime:%A, %B %d}}}} -> \"Monday, January 15\"");
                println!();
                println!("Format codes follow strftime conventions.");
                true
            }
            Err(e) => {
                eprintln!("Failed to get template variables: {}", e);
                false
            }
        }
    }

    fn default_template_content() -> String {
        r#"---
title: "{{title}}"
created: {{timestamp}}
---

# {{title}}

"#
        .to_string()
    }
}
