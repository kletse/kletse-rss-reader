// Kletse RSS for macOS: one window, its toolbar and the menus. Everything it shows comes from the Rust core
// (src/ffi.rs) through `Model`; keys and clicks go back to it.
import AppKit
import SwiftUI

@main
struct KletseApp: App {
    @NSApplicationDelegateAdaptor private var delegate: AppDelegate

    var body: some Scene {
        Window("Kletse RSS", id: "main") {
            ContentView()
                .frame(minWidth: 360, minHeight: 300)
        }
        .defaultSize(width: 900, height: 1000)
        // Also when it was closed at the last quit; there is only this one window.
        .defaultLaunchBehavior(.presented)
        .commands {
            CommandGroup(replacing: .newItem) {}
            CommandGroup(replacing: .appSettings) {
                Button("Settings…") { Model.shared.press("settings") }
                    .keyboardShortcut(",")
            }
            // Single keys can't be menu shortcuts: AppKit would take them from the settings fields too.
            CommandMenu("Article") {
                Button("Next (j)") { Model.shared.press("next") }
                Button("Skip, Keep Unread (n)") { Model.shared.key("n") }
                Button("Previous (k)") { Model.shared.press("prev") }
                Divider()
                Button("Toggle Read (m)") { Model.shared.key("m") }
                Button("Open Original (o)") { Model.shared.key("o") }
                Button("Fetch Full Article (f)") { Model.shared.key("f") }
                    .disabled(Model.shared.state?.fetches == false)
                Button("Save to Readeck (b)") { Model.shared.press("readeck") }
                Divider()
                Button("Feeds (p)") { Model.shared.press("picker") }
                Button("History (h)") { Model.shared.key("h") }
                Button("Mark Whole List Read (Shift+A)") { Model.shared.key("A") }
                Divider()
                Button("Search… (/)") { Model.shared.press("search") }
                Button("View Images (v)") { Model.shared.key("v") }
                Button("Oldest or Newest First (s)") { Model.shared.press("order") }
            }
            CommandGroup(replacing: .help) {
                Button("Keyboard Shortcuts (?)") { Model.shared.press("help") }
            }
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        MainActor.assumeIsolated { Model.shared.start() }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}
