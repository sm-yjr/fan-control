// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "FanControl",
    platforms: [.macOS(.v14)],
    targets: [
        .target(
            name: "SparkleRuntime",
            path: "Sources/SparkleRuntime",
            publicHeadersPath: "include",
            linkerSettings: [
                .linkedFramework("Foundation"),
            ]
        ),
        .executableTarget(
            name: "FanControl",
            dependencies: ["SparkleRuntime"],
            path: "Sources/FanControl",
            linkerSettings: [
                .linkedFramework("IOKit"),
            ]
        ),
    ]
)
