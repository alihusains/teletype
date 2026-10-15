// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "TeletypeFluid",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "TeletypeFluid", type: .static, targets: ["TeletypeFluid"]),
    ],
    dependencies: [
        // Plan §3.1: upstream FluidInference repo (never the saurabhav88 fork),
        // pinned to an exact release tag instead of a branch.
        .package(url: "https://github.com/FluidInference/FluidAudio", exact: "0.17.7"),
    ],
    targets: [
        .target(
            name: "TeletypeFluid",
            dependencies: [.product(name: "FluidAudio", package: "FluidAudio")]
        ),
    ]
)
