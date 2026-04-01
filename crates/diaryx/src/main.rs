#![doc = include_str!(concat!(env!("OUT_DIR"), "/README.md"))]

/// CLI module - command-line interface for diaryx
mod cli;

/// Editor module - opening editors in the command-line
mod editor;

fn main() {
    // Suppress broken-pipe panics when piping to `head`, `jq`, etc.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        if msg.contains("Broken pipe") {
            std::process::exit(0);
        }
        default_hook(info);
    }));

    cli::run_cli();
}
