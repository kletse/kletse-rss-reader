// The server details, in two places. `SettingsWindow` is the app's Settings window, beside the reader; `OnboardingView`
// is the same fields in the main window on first run, when there is nothing to read until they're filled in.
// A feed server is required (Miniflux, or a Google Reader API server such as FreshRSS), Readeck optional. Saving
// checks both servers before anything is written, which is why these have a Save button where most Mac settings
// apply as you go.
import AppKit
import SwiftUI

/// The field to start in: the chosen server's first empty one, or its last.
private enum Field {
    case minifluxUrl, minifluxApiKey, greaderUrl, greaderUsername, greaderPassword

    static func first(_ values: SettingsValues) -> Field {
        let fields: [(Field, String)] = values.server == "greader"
            ? [(.greaderUrl, values.greaderUrl), (.greaderUsername, values.greaderUsername),
               (.greaderPassword, values.greaderPassword)]
            : [(.minifluxUrl, values.minifluxUrl), (.minifluxApiKey, values.minifluxApiKey)]
        return (fields.first { $0.1.isEmpty } ?? fields[fields.count - 1]).0
    }
}

/// Applies at once, unlike the servers: there's nothing to check.
private struct AppearancePane: View {
    @AppStorage(Brand.key) private var colours = Brand.onward
    @AppStorage(AppIcon.key) private var icon = AppIcon.automatic

    var body: some View {
        Form {
            Picker("Colours", selection: $colours) {
                Text("Onward").tag(Brand.onward)
                Text("Original, with the system accent colour").tag(Brand.originalLook)
            }
            .pickerStyle(.radioGroup)
            Picker("App icon", selection: $icon) {
                Text("Light or dark, as the system").tag(AppIcon.automatic)
                Text("Orange").tag(AppIcon.orange)
            }
            .pickerStyle(.radioGroup)
        }
        .formStyle(.grouped)
        .scrollDisabled(true)
        .fixedSize(horizontal: false, vertical: true)
        .onChange(of: colours) { Model.shared.themeChanged() }
        .onChange(of: icon) { AppIcon.change() }
    }
}

struct SettingsWindow: View {
    let model: Model

    @State private var values = SettingsValues.empty
    /// As loaded, for Revert and to tell whether there is anything to save.
    @State private var saved = SettingsValues.empty
    @FocusState private var focus: Field?

    var body: some View {
        let settings = model.state?.settings
        TabView {
            Tab("Feed Server", systemImage: "antenna.radiowaves.left.and.right") {
                pane(settings) { FeedServerSections(values: $values, focus: $focus) }
            }
            Tab("Readeck", systemImage: "bookmark") {
                pane(settings) { ReadeckSection(values: $values) }
            }
            Tab("Appearance", systemImage: "paintpalette") {
                AppearancePane()
            }
        }
        .frame(width: 480)
        .background(WindowNote())
        // The form starts from the saved values once (the serial says when they are in); after that it keeps what
        // is typed, also when a check fails.
        .onChange(of: settings?.serial, initial: true) {
            guard settings != nil, let loaded = model.settingsValues else { return }
            values = loaded
            saved = loaded
            focus = .first(loaded)
        }
        // What's left unsaved when the window closes is dropped. A save closes it from `Model`.
        .onDisappear { model.cancelSettings() }
    }

    private func pane(_ settings: SettingsInfo?, @ViewBuilder content: () -> some View) -> some View {
        let checking = settings?.checking == true
        return VStack(spacing: 0) {
            Form {
                MessageSection(settings?.problem, systemImage: "exclamationmark.triangle.fill")
                content()
                MessageSection(settings?.error, systemImage: "xmark.octagon.fill")
            }
            .formStyle(.grouped)
            .scrollDisabled(true)
            .fixedSize(horizontal: false, vertical: true)
            HStack {
                if checking {
                    ProgressView().controlSize(.small)
                    Text("Checking…").foregroundStyle(.secondary)
                }
                Spacer()
                Button("Revert") { values = saved }
                    .disabled(values == saved || checking)
                Button("Save") { model.submitSettings(values) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(checking || (values == saved && settings?.problem.isEmpty != false))
            }
            .padding(.horizontal, 20)
            .padding(.bottom, 20)
        }
    }
}

/// Tells `Keyboard` which window is this one: its keys are its own, not the reader's.
private struct WindowNote: NSViewRepresentable {
    final class Note: NSView {
        override func viewDidMoveToWindow() {
            if let window { Keyboard.settingsWindow = window }
        }
    }

    func makeNSView(context: Context) -> Note { Note() }
    func updateNSView(_ view: Note, context: Context) {}
}

/// First run. It can't be cancelled.
struct OnboardingView: View {
    let model: Model

    @State private var values = SettingsValues.empty
    @FocusState private var focus: Field?

    var body: some View {
        let settings = model.state?.settings
        let checking = settings?.checking == true
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    LogoMark(width: 72).padding(.bottom, 6)
                    Text("Welcome to Kletse RSS").font(.largeTitle.bold())
                    Text("Connect your Miniflux or Google Reader API server to start reading. Readeck is optional: add it to save articles for later with b.")
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            }
            // What was wrong with the config file, above the fields it is about.
            MessageSection(settings?.problem, systemImage: "exclamationmark.triangle.fill")
            FeedServerSections(values: $values, focus: $focus)
            ReadeckSection(values: $values)
            MessageSection(settings?.error, systemImage: "xmark.octagon.fill")
            HStack {
                Spacer()
                Button(checking ? "Checking…" : "Connect") { model.submitSettings(values) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(checking)
            }
        }
        .formStyle(.grouped)
        .frame(maxWidth: 600)
        .frame(maxWidth: .infinity)
        .onChange(of: settings?.serial, initial: true) {
            guard settings != nil, let loaded = model.settingsValues else { return }
            values = loaded
            focus = .first(loaded)
        }
    }
}

private struct FeedServerSections: View {
    @Binding var values: SettingsValues
    var focus: FocusState<Field?>.Binding

    var body: some View {
        Section {
            Picker("Server type", selection: $values.server) {
                Text("Miniflux").tag("miniflux")
                Text("Google Reader API").tag("greader")
            }
            .onChange(of: values.server) { focus.wrappedValue = .first(values) }
            if values.server == "greader" {
                TextField("Server URL", text: $values.greaderUrl,
                          prompt: Text(verbatim: "https://freshrss.example.com/api/greader.php"))
                    .focused(focus, equals: .greaderUrl)
                TextField("Username", text: $values.greaderUsername)
                    .focused(focus, equals: .greaderUsername)
                SecureField("Password", text: $values.greaderPassword)
                    .focused(focus, equals: .greaderPassword)
            } else {
                TextField("Server URL", text: $values.minifluxUrl,
                          prompt: Text(verbatim: "https://miniflux.example.com"))
                    .focused(focus, equals: .minifluxUrl)
                SecureField("API key", text: $values.minifluxApiKey)
                    .focused(focus, equals: .minifluxApiKey)
            }
        } footer: {
            Group {
                if values.server == "greader" {
                    Text(verbatim: "FreshRSS: https://your-freshrss/api/greader.php. Miniflux: the server URL, with the Google Reader integration enabled.")
                } else {
                    Text("In Miniflux: Settings → API Keys → Create a new API key.")
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct ReadeckSection: View {
    @Binding var values: SettingsValues

    var body: some View {
        Section {
            TextField("Readeck URL", text: $values.readeckUrl,
                      prompt: Text(verbatim: "https://readeck.example.com"))
            SecureField("API token", text: $values.readeckToken)
        } footer: {
            Text("Optional: leave empty to hide the bookmark button. In Readeck: Profile → API Tokens (needs “Bookmarks: write”).")
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// Nothing when there is nothing to say.
private struct MessageSection: View {
    let message: String
    let systemImage: String

    init(_ message: String?, systemImage: String) {
        self.message = message ?? ""
        self.systemImage = systemImage
    }

    var body: some View {
        if !message.isEmpty {
            Section {
                Label(message, systemImage: systemImage)
                    .symbolRenderingMode(.multicolor)
            }
        }
    }
}
