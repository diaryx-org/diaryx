#![cfg(feature = "cli")]

use std::path::Path;
use std::process::Command;
use crate::config::Config;

/// Launch an editor to open a file
pub fn launch_editor(path: &Path, config: &Config) -> Result<(), EditorError> {
    let editor = determine_editor(config)?;

    let status = Command::new(&editor)
        .arg(path)
        .status()
        .map_err(|e| EditorError::LaunchFailed(editor.clone(), e))?;

    if !status.success() {
        return Err(EditorError::EditorExited(status.code().unwrap_or(-1)));
    }

    Ok(())
}

/// Determine which editor to use
fn determine_editor(config: &Config) -> Result<String, EditorError> {
    // 1. Check config file
    if let Some(ref editor) = config.editor {
        return Ok(editor.clone());
    }

    // 2. Check $EDITOR environment variable
    if let Ok(editor) = std::env::var("EDITOR") {
        return Ok(editor);
    }

    // 3. Check $VISUAL environment variable
    if let Ok(visual) = std::env::var("VISUAL") {
        return Ok(visual);
    }

    // 4. Platform-specific defaults
    #[cfg(target_os = "windows")]
    {
        return Ok("notepad.exe".to_string());
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Try common editors
        for editor in &["vim", "vi", "nano", "emacs"] {
            if which(editor) {
                return Ok(editor.to_string());
            }
        }
    }

    Err(EditorError::NoEditorFound)
}

/// Check if a command exists in PATH
#[cfg(not(target_os = "windows"))]
fn which(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[derive(Debug)]
pub enum EditorError {
    NoEditorFound,
    LaunchFailed(String, std::io::Error),
    EditorExited(i32),
}

impl std::fmt::Display for EditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditorError::NoEditorFound => {
                write!(f, "No editor found. Set $EDITOR, $VISUAL, or configure editor in config file")
            }
            EditorError::LaunchFailed(editor, e) => {
                write!(f, "Failed to launch editor '{}': {}", editor, e)
            }
            EditorError::EditorExited(code) => {
                write!(f, "Editor exited with code {}", code)
            }
        }
    }
}

impl std::error::Error for EditorError {}
