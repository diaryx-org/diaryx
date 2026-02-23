// swift-tools-version:5.7
import PackageDescription

let package = Package(
    name: "tauri-plugin-editor-toolbar",
    platforms: [
        .iOS(.v15),
    ],
    products: [
        .library(
            name: "tauri-plugin-editor-toolbar",
            type: .static,
            targets: ["tauri-plugin-editor-toolbar"]
        ),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api"),
    ],
    targets: [
        .target(
            name: "tauri-plugin-editor-toolbar",
            dependencies: [
                .byName(name: "Tauri"),
            ],
            path: "Sources"
        ),
    ]
)
