// A double-click on the empty part of the title bar zooms the window, or does what System Settings says instead. The
// toolbar's background colour (`toolbarBackground` in BrandStyle) is a SwiftUI view over the title bar that takes
// the click, and it drags the window but doesn't zoom it, so this does that part itself.
import AppKit

@MainActor
enum TitleBar {
    private static var monitor: Any?

    static func install() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .leftMouseDown) { event in
            let used = MainActor.assumeIsolated { handle(event) }
            return used ? nil : event
        }
    }

    private static func handle(_ event: NSEvent) -> Bool {
        guard event.clickCount == 2, let window = event.window, window !== Keyboard.settingsWindow,
              window.styleMask.contains(.titled), event.locationInWindow.y > window.contentLayoutRect.maxY,
              let frame = window.contentView?.superview,
              let hit = frame.hitTest(frame.convert(event.locationInWindow, from: nil)),
              !onToolbarItem(hit)
        else { return false }
        let defaults = UserDefaults.standard
        switch defaults.string(forKey: "AppleActionOnDoubleClick") {
        case "None": break
        case "Minimize": window.performMiniaturize(nil)
        case nil where defaults.bool(forKey: "AppleMiniaturizeOnDoubleClick"): window.performMiniaturize(nil)
        // "Fill" has no API of its own; zooming fills the screen too.
        default: zoom(window)
        }
        return true
    }

    /// The frame before the last zoom, and the one it zoomed to.
    private static var lastZoom: (window: NSWindow, from: NSRect, to: NSRect)?

    /// Zooms, or goes back the second time. Not by `isZoomed`: a real double-click starts a drag with its first click,
    /// and the system may move the zoomed window a little, after which it no longer counts as zoomed and a zoom does
    /// nothing. So it remembers, and goes back while the window is about where it zoomed to.
    private static func zoom(_ window: NSWindow) {
        if let last = lastZoom, last.window === window, near(window.frame, last.to) {
            lastZoom = nil
            window.setFrame(last.from, display: true, animate: true)
            return
        }
        let from = window.frame
        window.performZoom(nil)
        lastZoom = window.frame == from ? nil : (window, from, window.frame)
    }

    private static func near(_ a: NSRect, _ b: NSRect) -> Bool {
        abs(a.minX - b.minX) < 24 && abs(a.minY - b.minY) < 24 && abs(a.maxX - b.maxX) < 24 && abs(a.maxY - b.maxY) < 24
    }

    /// A button or another item in the toolbar, which has the click to itself.
    private static func onToolbarItem(_ view: NSView) -> Bool {
        var view: NSView? = view
        while let current = view {
            if current is NSControl || String(describing: type(of: current)).contains("ToolbarItem") { return true }
            view = current.superview
        }
        return false
    }
}
