// swift-tools-version: 6.0
import PackageDescription

// Local development links against the debug Rust static library.
// For release / CI, run `scripts/build-xcframework.sh` and switch to:
//
//   .binaryTarget(
//       name: "CSEPMLSFFI",
//       path: "../build/SEPMLSFFI.xcframework"
//   )
//
// and remove the unsafeFlags linker settings below.

let package = Package(
    name: "SwiftMLS",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .library(
            name: "SwiftMLS",
            targets: ["SwiftMLS"]
        )
    ],
    targets: [
        .target(
            name: "CSEPMLSFFI",
            path: "Sources/CSEPMLSFFI",
            publicHeadersPath: "include"
        ),
        .target(
            name: "SwiftMLS",
            dependencies: ["CSEPMLSFFI"],
            path: "Sources/SwiftMLS",
            linkerSettings: [
                .unsafeFlags([
                    "-L", "../target/debug",
                    "-lsep_xxxx_circuits"
                ])
            ]
        ),
        .testTarget(
            name: "SwiftMLSTests",
            dependencies: ["SwiftMLS"],
            path: "Tests/SwiftMLSTests"
        )
    ]
)
