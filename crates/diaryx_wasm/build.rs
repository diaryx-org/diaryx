use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=README.md");

    let readme = fs::read_to_string("README.md").expect("Failed to read README.md");

    let body = extract_body(&readme);

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set by Cargo");
    fs::write(Path::new(&out_dir).join("README.md"), body).expect("Failed to write README.md");
}

fn extract_body(content: &str) -> &str {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return content;
    }

    let rest = &content[4..];
    if let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
        let body_start = end_idx + 5;
        if body_start < rest.len() {
            &rest[body_start..]
        } else {
            ""
        }
    } else {
        content
    }
}
