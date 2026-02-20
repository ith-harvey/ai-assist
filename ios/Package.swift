// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "AIAssistClient",
    platforms: [
        .iOS(.v18),
        .macOS(.v15),
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
