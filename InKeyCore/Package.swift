// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "InKeyCore",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .library(name: "InKeyCore", targets: ["InKeyCore"])
    ],
    targets: [
        .target(name: "InKeyCore"),
        .testTarget(name: "InKeyCoreTests", dependencies: ["InKeyCore"])
    ]
)
