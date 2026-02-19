// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "Diaryx",
    platforms: [
        .macOS(.v14),
        .iOS(.v17),
    ],
    products: [
        .library(
            name: "DiaryxLib",
            targets: ["DiaryxLib"]
        ),
    ],
    targets: [
        .binaryTarget(
            name: "diaryx_apple",
            path: "diaryx_apple.xcframework"
        ),
        .target(
            name: "DiaryxLib",
            dependencies: ["diaryx_apple"],
            path: "Diaryx",
            resources: [
                .copy("../editor-bundle/dist"),
            ]
        ),
    ]
)
