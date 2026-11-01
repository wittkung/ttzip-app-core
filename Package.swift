// swift-tools-version: 6.0
// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import PackageDescription

let coreSwiftSettings: [SwiftSetting] = [
    .enableUpcomingFeature("StrictConcurrency")
]

let package = Package(
    name: "TTZipAppCore",
    platforms: [
        .macOS(.v14),
        .iOS(.v17)
    ],
    products: [
        .library(
            name: "TTZipAppCore",
            type: .dynamic,
            targets: ["TTZipAppCore"]
        ),
        .library(
            name: "CTTZipAppCoreBridge",
            targets: ["CTTZipAppCoreBridge"]
        )
    ],
    dependencies: [],
    targets: [
        .binaryTarget(
            name: "TTZipAppCoreVendor",
            path: "Frameworks/TTZipAppCoreVendor.xcframework"
        ),
        .target(
            name: "CTTZipAppCoreBridge",
            dependencies: ["TTZipAppCoreVendor"],
            path: "Sources/CTTZipAppCoreBridge",
            publicHeadersPath: "include",
            cSettings: [
                .headerSearchPath("include")
            ],
            linkerSettings: [
                .linkedFramework("IOKit", .when(platforms: [.macOS])),
                .linkedFramework("CoreFoundation", .when(platforms: [.macOS, .iOS])),
                .linkedFramework("Security", .when(platforms: [.macOS, .iOS])),
                .linkedLibrary("iconv", .when(platforms: [.macOS, .iOS])),
                .linkedLibrary("c++", .when(platforms: [.macOS, .iOS])),
                .linkedLibrary("pthread", .when(platforms: [.macOS, .iOS])),
                .linkedLibrary("archive", .when(platforms: [.macOS, .iOS])),
                .linkedLibrary("bz2", .when(platforms: [.macOS, .iOS])),
                .linkedLibrary("lzma", .when(platforms: [.macOS, .iOS])),
                .linkedLibrary("compression", .when(platforms: [.macOS, .iOS]))
            ]
        ),
        .target(
            name: "TTZipAppCore",
            dependencies: [
                "CTTZipAppCoreBridge",
                "TTZipAppCoreVendor"
            ],
            path: "Sources/TTZipAppCore",
            swiftSettings: coreSwiftSettings
        ),
        .testTarget(
            name: "TTZipAppCoreTests",
            dependencies: [
                "TTZipAppCore"
            ],
            path: "Tests/TTZipAppCoreTests",
            swiftSettings: coreSwiftSettings
        )
    ],
    swiftLanguageModes: [.v6]
)
