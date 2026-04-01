// swift-tools-version: 6.0
import PackageDescription

// Requires the XCFramework to be built first:
//   ./scripts/build-xcframework.sh
//
// For local macOS-only development with the debug Rust library, replace
// the binaryTarget with:
//
//   .target(
//       name: "CSEPMLSFFI",
//       path: "Sources/CSEPMLSFFI",
//       publicHeadersPath: "include"
//   ),
//
// and add linkerSettings to the SwiftMLS target:
//
//   linkerSettings: [
//       .unsafeFlags(["-L", "../target/debug", "-lsep_xxxx_circuits"])
//   ]

let package = Package(
    name: "SwiftMLS",
    platforms: [
        .macOS(.v13),
        .iOS(.v17)
    ],
    products: [
        .library(
            name: "SwiftMLS",
            targets: ["SwiftMLS"]
        )
    ],
    targets: [
        .binaryTarget(
            name: "CSEPMLSFFI",
            path: "../build/SEPMLSFFI.xcframework"
        ),
        .target(
            name: "SwiftMLS",
            dependencies: ["CSEPMLSFFI"],
            path: "Sources/SwiftMLS"
        ),
        .testTarget(
            name: "SwiftMLSTests",
            dependencies: ["SwiftMLS"],
            path: "Tests/SwiftMLSTests"
        )
    ]
)
