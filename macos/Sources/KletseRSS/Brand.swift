// The logo's colours, for light and dark mode, and the logo itself. The Rust palette has the same ones for the article
// (src/theme/macos.rs). `logo` is the mark's own orange; `accent` is deeper in light mode, where that one is too faint
// for text. Settings can switch to the original look instead: the system's window colours and accent.
import AppKit
import SwiftUI

enum Brand {
    static let key = "colours"
    static let onward = "onward"
    static let originalLook = "original"

    static var original: Bool { UserDefaults.standard.string(forKey: key) == originalLook }

    static let background = dynamic(light: 0xF7F4EC, dark: 0x1E130D)
    private static let orange = dynamic(light: 0xBF3F0C, dark: 0xFF8A3D)
    private static let onOrange = dynamic(light: 0xFFFFFF, dark: 0x1E130D)
    private static let logoOrange = dynamic(light: 0xFF6A2B, dark: 0xFF8A3D)

    static var accent: Color { original ? .accentColor : orange }
    /// What floats over the reader: the feeds picker, the keys, the search box.
    static var panel: Color { original ? Color(nsColor: .windowBackgroundColor) : background }
    /// Text on `accent`.
    static var onAccent: Color { original ? .white : onOrange }
    static var logo: Color { original ? .accentColor : logoOrange }

    private static func dynamic(light: Int, dark: Int) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            return rgb(isDark ? dark : light)
        })
    }

    private static func rgb(_ value: Int) -> NSColor {
        let parts = [value >> 16, value >> 8, value].map { CGFloat($0 & 0xFF) / 255 }
        return NSColor(srgbRed: parts[0], green: parts[1], blue: parts[2], alpha: 1)
    }
}

/// The chosen colours on a window: the logo's background and accent, or the system's own for the original look.
struct BrandStyle: ViewModifier {
    @AppStorage(Brand.key) private var colours = Brand.onward

    // One chain for both, not an `if`: two branches would be two views, and switching would tear the window's content
    // down, which for Settings means closing the form.
    func body(content: Content) -> some View {
        let original = colours == Brand.originalLook
        content
            // Forms paint their own grey over anything behind them.
            .scrollContentBackground(original ? .automatic : .hidden)
            .background(original ? Color.clear : Brand.background)
            .toolbarBackground(original ? AnyShapeStyle(.bar) : AnyShapeStyle(Brand.background), for: .windowToolbar)
            .tint(Brand.accent)
    }
}

/// Two dots and an O: the articles to come and the one you're reading. Drawn, not an image, so it takes any colour.
/// The sizes match packaging/logo, cropped to the mark.
struct LogoMark: View {
    var width: CGFloat = 96

    var body: some View {
        Canvas { context, size in
            let scale = size.width / 360
            func circle(_ x: CGFloat, _ radius: CGFloat) -> Path {
                let center = CGPoint(x: (x - 72) * scale, y: size.height / 2)
                let r = radius * scale
                return Path(ellipseIn: CGRect(x: center.x - r, y: center.y - r, width: r * 2, height: r * 2))
            }
            context.fill(circle(92, 20), with: .color(Brand.logo.opacity(0.5)))
            context.fill(circle(164, 32), with: .color(Brand.logo.opacity(0.75)))
            context.stroke(circle(330, 76), with: .color(Brand.logo), lineWidth: 52 * scale)
        }
        .frame(width: width, height: width * 204 / 360)
        .accessibilityHidden(true)
    }
}

/// The Dock icon. By default it's the bundle's, which macOS switches with the appearance; the orange one is a choice
/// in Settings, kept on the bundle so the Dock shows it while the app isn't running too.
enum AppIcon {
    static let key = "appIcon"
    static let orange = "orange"
    static let automatic = "automatic"

    /// At launch: the running app's icon is ours to set. Installing a new build drops the one kept on the bundle, which
    /// is the `Icon\r` file, so a chosen icon goes back on.
    @MainActor
    static func applyAtLaunch() {
        let image = chosen()
        NSApp.applicationIconImage = image
        let bundle = Bundle.main.bundlePath
        if image != nil, !FileManager.default.fileExists(atPath: bundle + "/Icon\r") {
            NSWorkspace.shared.setIcon(image, forFile: bundle)
            NSWorkspace.shared.noteFileSystemChanged(bundle)
        }
    }

    @MainActor
    static func change() {
        let image = chosen()
        NSApp.applicationIconImage = image
        NSWorkspace.shared.setIcon(image, forFile: Bundle.main.bundlePath)
        // The Dock and Finder keep their own copy until told.
        NSWorkspace.shared.noteFileSystemChanged(Bundle.main.bundlePath)
    }

    private static func chosen() -> NSImage? {
        guard UserDefaults.standard.string(forKey: key) == orange,
              let path = Bundle.main.path(forResource: "icon-orange", ofType: "png") else { return nil }
        return NSImage(contentsOfFile: path)
    }
}
