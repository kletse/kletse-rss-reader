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
    /// The server details, kept from the reply that had them: the Settings window may open after it.
    private(set) var settingsValues: SettingsValues?
    /// Bumped each time the core asks for the Settings window.
    private(set) var settingsRequests = 0
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
        TitleBar.install()
        for name in [NSColor.systemColorsDidChangeNotification, NSApplication.didChangeScreenParametersNotification] {
            NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { Model.shared.themeChanged() }
            }
        }
        Snapshot.startIfAsked(model: self)
    }

    /// The system appearance or accent colour changed, or the choice of colours in Settings.
    func themeChanged() {
        var command = theme()
        command["cmd"] = "theme"
        send(command)
    }

    private func theme() -> [String: Any] {
        let dark = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        return Brand.original ? ["dark": dark, "accent": NSColor.controlAccentColor.hex] : ["dark": dark]
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

    /// A button: "next", "prev", "picker", "readeck", "unread", "history", "refresh", "settings", "help",
    /// "search", "order", "import", "export".
    func press(_ button: String) { send(["cmd": "press", "button": button]) }
    func chooseRow(_ row: Int) { send(["cmd": "choose_row", "row": row]) }
    func pickerBackdrop() { send(["cmd": "picker_backdrop"]) }
    func closeHelp() { send(["cmd": "close_help"]) }
    func openLink(_ url: String) { send(["cmd": "open_link", "url": url]) }
    func chooseOnboarding(_ row: Int) { send(["cmd": "choose_onboarding", "row": row]) }
    func addressEdited(_ text: String) { send(["cmd": "address_edited", "text": text]) }
    func findServer() { send(["cmd": "find_server"]) }
    func cancelSettings() { send(["cmd": "cancel_settings"]) }
    func search(_ query: String) { send(["cmd": "search", "query": query]) }
    func cancelSearch() { send(["cmd": "cancel_search"]) }
    func viewImage(_ src: String) { send(["cmd": "view_image", "src": src]) }
    func closeViewer() { send(["cmd": "close_viewer"]) }
    func transferAction() { send(["cmd": "transfer_action"]) }
    func cancelTransfer() { send(["cmd": "cancel_transfer"]) }
    /// `replace`: a save panel has asked about a file that's there already.
    func feedFile(_ path: String, replace: Bool = false) {
        send(["cmd": "feed_file", "path": path, "replace": replace])
    }

    /// The system's open or save panel. It's not a sheet, for the same reasons as `Floating`, and cancelling it leaves
    /// the panel behind it asking.
    private func chooseFile(saving: Bool) {
        let panel: NSSavePanel
        if saving {
            panel = NSSavePanel()
            panel.nameFieldStringValue = state?.transfer?.suggested ?? ""
        } else {
            let opening = NSOpenPanel()
            opening.canChooseDirectories = false
            opening.allowsMultipleSelection = false
            panel = opening
        }
        Keyboard.filePanel = panel
        Task { @MainActor in
            let response = await panel.begin()
            Keyboard.filePanel = nil
            if response == .OK, let url = panel.url { feedFile(url.path, replace: saving) }
        }
    }

    func dismissToast(_ serial: Int) { send(["cmd": "dismiss_toast", "serial": serial]) }

    func submitSettings(_ values: SettingsValues) {
        send(["cmd": "submit_settings", "server": values.server, "fields": values.fields])
    }

    /// AppKit won't quit while a sheet is up, as a confirm alert is, so they go first.
    func quit() {
        for window in NSApp.windows.reversed() {
            if let sheet = window.attachedSheet { window.endSheet(sheet) }
        }
        DispatchQueue.main.async { NSApp.terminate(nil) }
    }

    @discardableResult
    private func send(_ command: [String: Any]) -> Reply? {
        guard let reply = core.send(command) else { return nil }
        let formClosed = state?.settings != nil && reply.state.settings == nil
        state = reply.state
        // Here and not in the window's view: SwiftUI holds a hidden window's changes back until it opens again.
        if formClosed { Keyboard.settingsWindow?.close() }
        if let blocks = reply.state.article?.blocks {
            self.blocks = blocks
        }
        if let values = reply.state.settings?.values {
            settingsValues = values
        }
        RemoteImage.prefetch(reply.state.prefetchImages)
        for event in reply.events {
            switch event {
            case "scroll_up", "scroll_down":
                scrollUp = event == "scroll_up"
                scrollSerial += 1
            case "show_settings":
                settingsRequests += 1
            case "choose_import_file", "choose_export_file":
                chooseFile(saving: event == "choose_export_file")
            case "quit":
                quit()
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
