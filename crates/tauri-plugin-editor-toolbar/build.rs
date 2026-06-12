const COMMANDS: &[&str] = &[];

fn main() {
    // Ensure swift-rs uses an iOS 16+ deployment target even outside Xcode
    // builds. Below 16.0 the Swift compiler force-links back-deployment compat
    // libs (swiftCompatibility56 / swiftCompatibilityConcurrency) that Xcode 26
    // no longer ships, breaking the link. Keep in sync with the iOS
    // `minimumSystemVersion` in apps/tauri/src-tauri/tauri.conf.json.
    if std::env::var("IPHONEOS_DEPLOYMENT_TARGET").is_err() {
        std::env::set_var("IPHONEOS_DEPLOYMENT_TARGET", "16.0");
    }
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
