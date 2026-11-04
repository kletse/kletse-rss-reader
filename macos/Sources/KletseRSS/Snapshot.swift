// KLETSE_RSS_SNAPSHOT=/path.png: once the feeds are in, press KLETSE_RSS_SNAPSHOT_KEYS (default "Enter", which
// opens the first row; a space is written "Space"), one per 400 ms, give images 3 s, save the window to the
// PNG and quit. As the Qt version does. The window is 1200×860 for it, the size of the README's screenshots, and
// a sheet that is open is drawn over it where it sits (the image grows to take in anything that hangs out of
// the window). Alerts don't draw into a bitmap.
import AppKit

@MainActor
enum Snapshot {
    static func startIfAsked(model: Model) {
        let env = ProcessInfo.processInfo.environment
        guard let path = env["KLETSE_RSS_SNAPSHOT"], !path.isEmpty else { return }
        let keys = (env["KLETSE_RSS_SNAPSHOT_KEYS"] ?? "Enter").split(separator: " ").map(String.init)
        Task { @MainActor in
            while model.state?.picker.state != "ready" && model.state?.overlay != "onboarding" || main() == nil {
                try? await Task.sleep(for: .milliseconds(100))
            }
            if let window = main() {
                let content = NSRect(x: 0, y: 0, width: 1200, height: 860)
                window.setFrame(window.frameRect(forContentRect: content), display: true)
                window.center()
            }
            for key in keys {
                try? await Task.sleep(for: .milliseconds(400))
                model.key(key == "Space" ? " " : key)
            }
            try? await Task.sleep(for: .seconds(3))
            save(to: path)
            // Not `terminate`: an open sheet can hold that up.
            exit(0)
        }
    }

    /// The app's window, not a sheet or alert.
    private static func main() -> NSWindow? {
        NSApp.windows.first {
            $0.isVisible && $0.styleMask.contains(.titled) && $0.sheetParent == nil && $0.frame.width >= 360
                && $0 !== Keyboard.settingsWindow
        }
    }

    private static func save(to path: String) {
        guard let main = main() else { return }
        // The window first, then what floats over it, each by its frame view (content, title bar, toolbar).
        // An alert's glass backdrop is a window of its own that draws nothing.
        let others = NSApp.windows.filter { $0 !== main && $0.isVisible && !$0.className.contains("Glass") }
        let canvas = others.reduce(main.frame) { $0.union($1.frame) }
        let scale = main.backingScaleFactor
        guard let out = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(canvas.width * scale),
            pixelsHigh: Int(canvas.height * scale), bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
            isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ) else { return }
        out.size = canvas.size
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: out)
        for window in [main] + others {
            guard let view = window.contentView?.superview ?? window.contentView,
                  let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds)
            else { continue }
            view.cacheDisplay(in: view.bounds, to: rep)
            let at = NSPoint(x: window.frame.minX - canvas.minX, y: window.frame.minY - canvas.minY)
            rep.draw(in: NSRect(origin: at, size: window.frame.size), from: .zero, operation: .sourceOver,
                     fraction: 1, respectFlipped: true, hints: nil)
        }
        NSGraphicsContext.restoreGraphicsState()
        try? out.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
    }
}
