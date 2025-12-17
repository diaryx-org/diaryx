#![doc = include_str!("../README.md")]

/// CLI module - command-line interface for diaryx
mod cli;

/// Editor module - opening editors in the command-line
mod editor;

fn main() {
    cli::run_cli();
}
