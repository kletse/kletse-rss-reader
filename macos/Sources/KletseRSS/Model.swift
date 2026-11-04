// The window's state: the last reply from the core, plus what only the window knows (scroll position).
// Views only read this and call its methods; every decision is the core's.
import AppKit
import Observation

@MainActor
@Observable
final class Model {
    static let shared = Model()

    private(set) var state: AppState?
    /// The article body, kept from the last reply that had it.
    private(set) var blocks: [Block] = []
    /// Bumped for each page scroll the core asks for.
    private(set) var scrollSerial = 0
    private(set) var scrollUp = false
    /// The article is scrolled to its end (Space then moves on).
    var atBottom = false

    @ObservationIgnored private lazy var core = Core()
    @ObservationIgnored private var started = false

    func start() {
        guard !started else { return }
        started = true
        var command = theme()
        command["cmd"] = "start"
        send(command)
        Keyboard.install(model: self)
        for name in [NSColor.systemColorsDidChangeNotification, NSApplication.didChangeScreenParametersNotification] {
            NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { Model.shared.themeChanged() }
            }
        }
        Snapshot.startIfAsked(model: self)
    }

    /// The system appearance or accent colour changed.
    func themeChanged() {
        var command = theme()
        command["cmd"] = "theme"
        send(command)
    }

    private func theme() -> [String: Any] {
        let dark = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        return ["dark": dark, "accent": NSColor.controlAccentColor.hex]
    }

    func pump() {
        send(["cmd": "pump"])
    }

    /// A key press, named as the web names keys. Returns whether the core used it.
    @discardableResult
    func key(_ name: String, shift: Bool = false, command: Bool = false, option: Bool = false, control: Bool = false,
             typing: Bool = false) -> Bool {
        send([
            "cmd": "key", "key": name, "shift": shift, "ctrl": command, "alt": option, "meta": control,
            "typing": typing, "at_bottom": atBottom,
        ])?.used ?? false
    }

    /// A button: "next", "prev", "picker", "readeck", "mode", "settings", "help", "search", "order".
    func press(_ button: String) { send(["cmd": "press", "button": button]) }
    func chooseRow(_ row: Int) { send(["cmd": "choose_row", "row": row]) }
    func pickerBackdrop() { send(["cmd": "picker_backdrop"]) }
    func closeHelp() { send(["cmd": "close_help"]) }
    func openLink(_ url: String) { send(["cmd": "open_link", "url": url]) }
    func cancelSettings() { send(["cmd": "cancel_settings"]) }
    /// A click on a tab of the settings form; the core switches, and the next state shows it.
    func settingsSection(_ name: String) { send(["cmd": "settings_section", "section": name]) }
    func search(_ query: String) { send(["cmd": "search", "query": query]) }
    func cancelSearch() { send(["cmd": "cancel_search"]) }
    func viewImage(_ src: String) { send(["cmd": "view_image", "src": src]) }
    func closeViewer() { send(["cmd": "close_viewer"]) }
    func dismissToast(_ serial: Int) { send(["cmd": "dismiss_toast", "serial": serial]) }

    func submitSettings(_ values: SettingsValues) {
        send([
            "cmd": "submit_settings", "server": values.server, "miniflux_url": values.minifluxUrl,
            "miniflux_api_key": values.minifluxApiKey, "greader_url": values.greaderUrl,
            "greader_username": values.greaderUsername, "greader_password": values.greaderPassword,
            "readeck_url": values.readeckUrl, "readeck_token": values.readeckToken,
        ])
    }

    @discardableResult
    private func send(_ command: [String: Any]) -> Reply? {
        guard let reply = core.send(command) else { return nil }
        state = reply.state
        if let blocks = reply.state.article?.blocks {
            self.blocks = blocks
        }
        RemoteImage.prefetch(reply.state.prefetchImages)
        for event in reply.events {
            switch event {
            case "scroll_up", "scroll_down":
                scrollUp = event == "scroll_up"
                scrollSerial += 1
            case "quit":
                NSApp.terminate(nil)
            default:
                break
            }
        }
        return reply
    }
}

extension NSColor {
    /// `#rrggbb` in sRGB.
    var hex: String {
        guard let c = usingColorSpace(.sRGB) else { return "#0a84ff" }
        let v = [c.redComponent, c.greenComponent, c.blueComponent].map { Int(($0 * 255).rounded()) }
        return String(format: "#%02x%02x%02x", v[0], v[1], v[2])
    }
}
