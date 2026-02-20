// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Kronroe",
    platforms: [
        .iOS(.v15)
    ],
    products: [
        .library(
            name: "Kronroe",
            targets: ["Kronroe"]
        )
    ],
    targets: [
        .binaryTarget(
            name: "KronroeFFI",
            path: "KronroeFFI.xcframework"
        ),
        .target(
            name: "Kronroe",
            dependencies: ["KronroeFFI"],
            path: "Sources/Kronroe"
        )
    ]
)
