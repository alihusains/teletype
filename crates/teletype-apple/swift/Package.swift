// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "TeletypeApple",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "TeletypeApple", type: .static, targets: ["TeletypeApple"]),
    ],
    dependencies: [],
    targets: [
        .target(name: "TeletypeApple"),
    ]
)
