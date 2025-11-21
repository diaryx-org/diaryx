#![cfg(feature = "cli")]

use clap::{Parser, Subcommand};
use diaryx_core::app::DiaryxApp;
use diaryx_core::config::Config;
use diaryx_core::date::parse_date;
use diaryx_core::editor::launch_editor;
use diaryx_core::fs::RealFileSystem;
use serde_yaml::Value;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "diaryx")]
#[command(about = "A tool to manage markdown files with YAML frontmatter", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new diary entry with default frontmatter
    Create {
        /// Path to the new entry file
        path: String,
    },

    /// Set a frontmatter property (adds or updates)
    Set {
        /// Path to the entry file
        path: String,

        /// Property key to set
        key: String,

        /// Property value (as YAML - e.g., "hello", "42", "[1,2,3]", "{a: 1}")
        value: String,
    },

    /// Get a frontmatter property value
    Get {
        /// Path to the entry file
        path: String,

        /// Property key to get
        key: String,
    },

    /// Remove a frontmatter property
    Remove {
        /// Path to the entry file
        path: String,

        /// Property key to remove
        key: String,
    },

    /// List all frontmatter properties
    List {
        /// Path to the entry file
        path: String,
    },

    /// Initialize diaryx configuration
    Init {
        /// Base directory for diary entries (default: ~/diaryx)
        #[arg(short, long)]
        base_dir: Option<PathBuf>,
    },

    /// Open today's entry in your editor
    Today,

    /// Open yesterday's entry in your editor
    Yesterday,

    /// Open an entry for a specific date
    Open {
        /// Date to open (e.g., "2024-01-15", "today", "yesterday")
        date: String,
    },

    /// Show current configuration
    Config,
}

pub fn run_cli() {
    let cli = Cli::parse();

    // Setup dependencies
    let fs = RealFileSystem;
    let app = DiaryxApp::new(fs);

    // Execute commands
    match cli.command {
        Commands::Init { base_dir } => {
            let dir = base_dir.unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("diaryx")
            });

            match Config::init(dir.clone()) {
                Ok(_) => {
                    println!("✓ Initialized diaryx configuration");
                    println!("  Base directory: {}", dir.display());
                    if let Some(config_path) = Config::config_path() {
                        println!("  Config file: {}", config_path.display());
                    }
                }
                Err(e) => eprintln!("✗ Error initializing config: {}", e),
            }
        }

        Commands::Today => {
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("✗ Error loading config: {}", e);
                    eprintln!("  Run 'diaryx init' first");
                    return;
                }
            };

            match parse_date("today") {
                Ok(date) => {
                    match app.ensure_dated_entry(&date, &config) {
                        Ok(path) => {
                            println!("Opening: {}", path.display());
                            if let Err(e) = launch_editor(&path, &config) {
                                eprintln!("✗ Error launching editor: {}", e);
                            }
                        }
                        Err(e) => eprintln!("✗ Error creating entry: {}", e),
                    }
                }
                Err(e) => eprintln!("✗ Error parsing date: {}", e),
            }
        }

        Commands::Yesterday => {
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("✗ Error loading config: {}", e);
                    eprintln!("  Run 'diaryx init' first");
                    return;
                }
            };

            match parse_date("yesterday") {
                Ok(date) => {
                    match app.ensure_dated_entry(&date, &config) {
                        Ok(path) => {
                            println!("Opening: {}", path.display());
                            if let Err(e) = launch_editor(&path, &config) {
                                eprintln!("✗ Error launching editor: {}", e);
                            }
                        }
                        Err(e) => eprintln!("✗ Error creating entry: {}", e),
                    }
                }
                Err(e) => eprintln!("✗ Error parsing date: {}", e),
            }
        }

        Commands::Open { date } => {
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("✗ Error loading config: {}", e);
                    eprintln!("  Run 'diaryx init' first");
                    return;
                }
            };

            match parse_date(&date) {
                Ok(parsed_date) => {
                    match app.ensure_dated_entry(&parsed_date, &config) {
                        Ok(path) => {
                            println!("Opening: {}", path.display());
                            if let Err(e) = launch_editor(&path, &config) {
                                eprintln!("✗ Error launching editor: {}", e);
                            }
                        }
                        Err(e) => eprintln!("✗ Error creating entry: {}", e),
                    }
                }
                Err(e) => eprintln!("✗ {}", e),
            }
        }

        Commands::Config => {
            match Config::load() {
                Ok(config) => {
                    println!("Current configuration:");
                    println!("  Base directory: {}", config.base_dir.display());
                    println!("  Editor: {}", config.editor.as_deref().unwrap_or("$EDITOR"));
                    println!("  Default template: {}", config.default_template.as_deref().unwrap_or("none"));
                    if let Some(config_path) = Config::config_path() {
                        println!("\nConfig file: {}", config_path.display());
                    }
                }
                Err(e) => {
                    eprintln!("✗ Error loading config: {}", e);
                    eprintln!("  Run 'diaryx init' to create a config file");
                }
            }
        }
        Commands::Create { path } => {
            match app.create_entry(&path) {
                Ok(_) => println!("✓ Created entry: {}", path),
                Err(e) => eprintln!("✗ Error creating entry: {}", e),
            }
        }

        Commands::Set { path, key, value } => {
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("✗ Error loading config: {}", e);
                    eprintln!("  Run 'diaryx init' first");
                    return;
                }
            };

            // Resolve path (supports "today", "yesterday", or literal paths)
            let resolved_path = app.resolve_path(&path, &config);
            let path_str = resolved_path.to_string_lossy();

            // Parse the value as YAML
            match serde_yaml::from_str::<Value>(&value) {
                Ok(yaml_value) => {
                    match app.set_frontmatter_property(&path_str, &key, yaml_value) {
                        Ok(_) => println!("✓ Set '{}' in {}", key, path),
                        Err(e) => eprintln!("✗ Error setting property: {}", e),
                    }
                }
                Err(e) => eprintln!("✗ Invalid YAML value: {}", e),
            }
        }

        Commands::Get { path, key } => {
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("✗ Error loading config: {}", e);
                    eprintln!("  Run 'diaryx init' first");
                    return;
                }
            };

            // Resolve path (supports "today", "yesterday", or literal paths)
            let resolved_path = app.resolve_path(&path, &config);
            let path_str = resolved_path.to_string_lossy();

            match app.get_frontmatter_property(&path_str, &key) {
                Ok(Some(value)) => {
                    // Format output based on value type
                    match &value {
                        Value::Sequence(items) => {
                            // Print each array item on its own line
                            for item in items {
                                match item {
                                    Value::String(s) => println!("{}", s),
                                    _ => println!("{}", serde_yaml::to_string(item).unwrap_or_default().trim()),
                                }
                            }
                        }
                        Value::String(s) => {
                            // Print strings directly without quotes
                            println!("{}", s);
                        }
                        _ => {
                            // For other types, use YAML formatting
                            println!("{}", serde_yaml::to_string(&value).unwrap_or_default().trim());
                        }
                    }
                }
                Ok(None) => {
                    eprintln!("Property '{}' not found in {}", key, path);
                }
                Err(e) => eprintln!("✗ Error getting property: {}", e),
            }
        }

        Commands::Remove { path, key } => {
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("✗ Error loading config: {}", e);
                    eprintln!("  Run 'diaryx init' first");
                    return;
                }
            };

            // Resolve path (supports "today", "yesterday", or literal paths)
            let resolved_path = app.resolve_path(&path, &config);
            let path_str = resolved_path.to_string_lossy();

            match app.remove_frontmatter_property(&path_str, &key) {
                Ok(_) => println!("✓ Removed '{}' from {}", key, path),
                Err(e) => eprintln!("✗ Error removing property: {}", e),
            }
        }

        Commands::List { path } => {
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("✗ Error loading config: {}", e);
                    eprintln!("  Run 'diaryx init' first");
                    return;
                }
            };

            // Resolve path (supports "today", "yesterday", or literal paths)
            let resolved_path = app.resolve_path(&path, &config);
            let path_str = resolved_path.to_string_lossy();

            match app.get_all_frontmatter(&path_str) {
                Ok(frontmatter) => {
                    if frontmatter.is_empty() {
                        println!("No frontmatter properties in {}", path);
                    } else {
                        println!("Frontmatter in {}:", path);
                        for (key, value) in frontmatter {
                            println!("  {}: {}", key, serde_yaml::to_string(&value).unwrap_or_default().trim());
                        }
                    }
                }
                Err(e) => eprintln!("✗ Error listing frontmatter: {}", e),
            }
        }
    }
}
