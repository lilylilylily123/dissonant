// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "DissonantCore",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .library(name: "DissonantCore", targets: ["DissonantCore"])
    ],
    targets: [
        .target(name: "DissonantCore"),
        .testTarget(name: "DissonantCoreTests", dependencies: ["DissonantCore"])
    ]
)
