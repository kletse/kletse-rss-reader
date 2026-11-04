// Onward for macOS: the reader's window, its toolbar, the menus and the Settings window. Everything it shows comes from the Rust core
// (src/ffi.rs) through `Model`; keys and clicks go back to it.
import AppKit
import SwiftUI

@main
struct OnwardApp: App {
    @NSApplicationDelegateAdaptor private var delegate: AppDelegate

    init() {
        AppDefaults.moveFromOldName()
    }

    var body: some Scene {
        Window("Onward", id: "main") {
            ContentView()
                .frame(minWidth: 360, minHeight: 300)
                .modifier(BrandStyle())
        }
        .defaultSize(width: 900, height: 1000)
        // Also when it was closed at the last quit; there is only this one window.
        .defaultLaunchBehavior(.presented)
        .commands {
            CommandGroup(replacing: .newItem) {}
            CommandGroup(replacing: .appSettings) {
                Button("Settings…") { Model.shared.press("settings") }
                    .keyboardShortcut(",")
                    .disabled(Model.shared.state?.overlay == "onboarding")
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
            CommandGroup(replacing: .appTermination) {
                Button("Quit Onward") { Model.shared.quit() }
                    .keyboardShortcut("q")
            }
            CommandGroup(replacing: .help) {
                Button("Keyboard Shortcuts (?)") { Model.shared.press("help") }
            }
        }

        Settings {
            SettingsWindow(model: Model.shared)
                .modifier(BrandStyle())
        }
        .windowResizability(.contentSize)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        MainActor.assumeIsolated {
            // For screenshots: the system's own setting can't be overridden from outside.
            switch ProcessInfo.processInfo.environment["ONWARD_APPEARANCE"] {
            case "light": NSApp.appearance = NSAppearance(named: .aqua)
            case "dark": NSApp.appearance = NSAppearance(named: .darkAqua)
            default: break
            }
            AppIcon.applyAtLaunch()
            Model.shared.start()
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}
