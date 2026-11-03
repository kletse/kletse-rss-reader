// swift-tools-version: 6.0
// The native macOS app: SwiftUI over the Rust core (src/ffi.rs), linked as a static library.
// Build it with packaging/macos/build-app.sh, which builds the library first and puts the app together.
import PackageDescription

let package = Package(
    name: "KletseRSS",
    platforms: [.macOS(.v15)],
    targets: [
        .systemLibrary(name: "CKletse", path: "Sources/CKletse"),
        .executableTarget(
            name: "KletseRSS",
            dependencies: ["CKletse"],
            path: "Sources/KletseRSS",
            linkerSettings: [.unsafeFlags(["-L", Context.packageDirectory + "/../target/release"])]
        ),
    ]
)
