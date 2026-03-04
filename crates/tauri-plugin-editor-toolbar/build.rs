const COMMANDS: &[&str] = &[];

fn main() {
    // Ensure swift-rs uses iOS 15+ deployment target even outside Xcode builds
    if std::env::var("IPHONEOS_DEPLOYMENT_TARGET").is_err() {
        std::env::set_var("IPHONEOS_DEPLOYMENT_TARGET", "15.0");
    }
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
