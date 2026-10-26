// swift-tools-version: 6.0
import PackageDescription

// The Swift binding lives in bindings/swift; Package.swift is here because SwiftPM wants it at the
// root of the repository. It links the shared library `sortition` (cargo build --release); tell the
// linker where it is: swift build -Xlinker -L<dir>   (and -Xlinker -rpath -Xlinker <dir> to run).
let package = Package(
    name: "Sortition",
    products: [
        .library(name: "Sortition", targets: ["Sortition"]),
    ],
    targets: [
        // The C header of the core, with `link "sortition"`.
        .systemLibrary(name: "CSortition", path: "bindings/swift/Sources/CSortition"),
        .target(name: "Sortition", dependencies: ["CSortition"], path: "bindings/swift/Sources/Sortition"),
        .testTarget(name: "SortitionTests", dependencies: ["Sortition"], path: "bindings/swift/Tests/SortitionTests"),
    ]
)
