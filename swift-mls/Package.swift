// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SwiftMLS",
    platforms: [
        .iOS(.v15),
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
