// Every key goes to the core first, named as the web's `KeyboardEvent.key` names it (src/keys.rs has the
// keymaps). What the core doesn't use carries on as usual: ⌘Q, ⌘C, typing in a field. The Settings window keeps its
// keys to itself, or j on a popup button would move the reader along behind it.
import AppKit

@MainActor
enum Keyboard {
    private static var monitor: Any?
    static weak var settingsWindow: NSWindow?

    static func install(model: Model) {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            let used = MainActor.assumeIsolated { handle(event, model: model) }
            return used ? nil : event
        }
    }

    private static func handle(_ event: NSEvent, model: Model) -> Bool {
        if let settingsWindow, event.window === settingsWindow { return false }
        guard let name = name(of: event) else { return false }
        let flags = event.modifierFlags
        return model.key(
            name,
            shift: flags.contains(.shift),
            command: flags.contains(.command),
            option: flags.contains(.option),
            control: flags.contains(.control),
            typing: typing(in: event.window),
            held: event.isARepeat
        )
    }

    /// A text field has the keyboard: its field editor is the first responder. Selectable article text, once
    /// clicked, gets one too, but that one isn't editable.
    private static func typing(in window: NSWindow?) -> Bool {
        guard let editor = window?.firstResponder as? NSTextView else { return false }
        return editor.isFieldEditor && editor.isEditable
    }

    static func name(of event: NSEvent) -> String? {
        switch event.keyCode {
        case 125: return "ArrowDown"
        case 126: return "ArrowUp"
        case 123: return "ArrowLeft"
        case 124: return "ArrowRight"
        case 53: return "Escape"
        case 36, 76: return "Enter"
        case 48: return "Tab"
        case 49: return " "
        default: break
        }
        // Shift still applies ("A", "?"); ⌘ and the rest don't ("=" for ⌘=).
        guard let chars = event.charactersIgnoringModifiers, chars.count == 1,
              let scalar = chars.unicodeScalars.first,
              scalar.value >= 32, !(0xF700...0xF8FF).contains(scalar.value) // function keys
        else { return nil }
        return chars
    }
}
