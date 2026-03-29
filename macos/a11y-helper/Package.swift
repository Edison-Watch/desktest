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
        // NOTE: This is an executableTarget, not a testTarget, because XCTest/Testing
        // frameworks are unavailable without Xcode (Command Line Tools only).
        // Run tests with: swift run a11y-helper-tests (NOT swift test)
        .executableTarget(
            name: "a11y-helper-tests",
            dependencies: ["A11yHelperLib"],
            path: "Sources/A11yHelperTests"
        ),
    ]
)
