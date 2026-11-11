// swift-tools-version:5.9
// Native push, local notifications and alarms for the Agentic OS iOS app.
// dx 0.7.10 builds this package (found through `#[manganis::ffi("src/ios/push")]` in src/native/ios.rs)
// into Frameworks/DioxusSwiftPlugins.framework; Rust loads it and calls the @objc methods.
import PackageDescription

let package = Package(
    name: "AosPushPlugin",
    platforms: [
        .iOS(.v16),
        .macOS(.v13),
    ],
    products: [
        .library(
            name: "AosPushPlugin",
            type: .static,
            targets: ["AosPushPlugin"]
        )
    ],
    dependencies: [],
    targets: [
        .target(
            name: "AosPushPlugin",
            path: "Sources",
            linkerSettings: [
                .linkedFramework("Foundation"),
                .linkedFramework("UIKit", .when(platforms: [.iOS])),
                .linkedFramework("UserNotifications"),
            ]
        )
    ]
)
