//! src-tauri/src/main.rs

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    diaryx_tauri_lib::run();
}
