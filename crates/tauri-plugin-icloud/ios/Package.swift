// swift-tools-version:5.7
import PackageDescription

let package = Package(
    name: "tauri-plugin-icloud",
    platforms: [
        .iOS(.v15),
    ],
    products: [
        .library(name: "tauri-plugin-icloud", type: .static, targets: ["tauri-plugin-icloud"]),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api"),
    ],
    targets: [
        .target(
            name: "tauri-plugin-icloud",
            dependencies: [
                .byName(name: "Tauri"),
            ],
            path: "Sources"
        ),
    ]
)
