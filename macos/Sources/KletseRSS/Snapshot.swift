// KLETSE_RSS_SNAPSHOT=/path.png: once the feeds are in, press KLETSE_RSS_SNAPSHOT_KEYS (default "Enter", which
// opens the first row; a space is written "Space"), one per 400 ms, give images 3 s, save the window to the
// PNG and quit. A popover or sheet that is open goes to a second file, "<path>-2.png". As the Qt version does.
import AppKit

@MainActor
enum Snapshot {
    static func startIfAsked(model: Model) {
        let env = ProcessInfo.processInfo.environment
        guard let path = env["KLETSE_RSS_SNAPSHOT"], !path.isEmpty else { return }
        let keys = (env["KLETSE_RSS_SNAPSHOT_KEYS"] ?? "Enter").split(separator: " ").map(String.init)
        Task { @MainActor in
            while model.state?.picker.state != "ready" {
                try? await Task.sleep(for: .milliseconds(100))
            }
            for key in keys {
                try? await Task.sleep(for: .milliseconds(400))
                model.key(key == "Space" ? " " : key)
            }
            try? await Task.sleep(for: .seconds(3))
            save(to: path)
            // Not `terminate`: an open popover or sheet can hold that up.
            exit(0)
        }
    }

    private static func save(to path: String) {
        // An alert's glass backdrop is a window of its own, and draws nothing into a bitmap.
        let windows = NSApp.windows.filter { $0.isVisible && $0.frame.width > 1 && !$0.className.contains("Glass") }
            .sorted { $0.frame.width * $0.frame.height > $1.frame.width * $1.frame.height }
        for (i, window) in windows.prefix(2).enumerated() {
            // The frame view: the content and the title bar with its toolbar.
            guard let view = window.contentView?.superview ?? window.contentView,
                  let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds)
            else { continue }
            view.cacheDisplay(in: view.bounds, to: rep)
            let file = i == 0 ? path : path.replacingOccurrences(of: ".png", with: "-2.png")
            try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: file))
        }
    }
}
