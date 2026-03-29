// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "a11y-helper",
    platforms: [.macOS(.v12)],
    targets: [
        .target(
            name: "A11yHelperLib",
            path: "Sources/A11yHelperLib"
        ),
        .executableTarget(
            name: "a11y-helper",
            dependencies: ["A11yHelperLib"],
            path: "Sources/A11yHelperCLI"
        ),
        .executableTarget(
            name: "a11y-helper-tests",
            dependencies: ["A11yHelperLib"],
            path: "Sources/A11yHelperTests"
        ),
    ]
)
