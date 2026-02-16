// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "AIAssistClient",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: [
        .library(
            name: "AIAssistClientLib",
            targets: ["AIAssistClientLib"]
        ),
    ],
    targets: [
        .target(
            name: "AIAssistClientLib",
            path: "Sources/AIAssistClientLib"
        ),
        .testTarget(
            name: "AIAssistClientTests",
            dependencies: ["AIAssistClientLib"],
            path: "Tests/AIAssistClientTests"
        ),
    ]
)
