// The server details, in two places. `SettingsWindow` is the app's Settings window, beside the reader; `OnboardingView`
// is first run in the main window: what it offers, and Connect, which starts from an address and shows what the core
// says that server needs. The fields, their help and all of first run's words come from the table in src/providers.rs.
// A feed server is required (Miniflux, or a Google Reader API server such as FreshRSS), Readeck optional. Saving
// checks both servers before anything is written, which is why these have a Save button where most Mac settings
// apply as you go.
import AppKit
import SwiftUI

/// The field to start in: the chosen server's first empty one, or its last.
private func firstField(_ values: SettingsValues, _ providers: [ProviderInfo]) -> String? {
    guard let fields = providers.first(where: { $0.id == values.server })?.fields else { return nil }
    return (fields.first { values[field: $0.key].isEmpty } ?? fields.last)?.key
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
    @FocusState private var focus: String?

    var body: some View {
        let settings = model.state?.settings
        TabView {
            Tab("Feed Server", systemImage: "antenna.radiowaves.left.and.right") {
                pane(settings) {
                    Section {
                        Button(settings?.findServer ?? "") { model.findServer() }
                            .disabled(settings?.checking == true)
                    }
                    FeedServerSections(model: model, providers: settings?.providers ?? [], values: $values, focus: $focus)
                }
            }
            Tab("Readeck", systemImage: "bookmark") {
                pane(settings) {
                    ReadeckSection(fields: settings?.readeckFields ?? [], values: $values, focus: $focus)
                }
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
            focus = firstField(loaded, settings?.providers ?? [])
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

/// First run: what the first screen offers, the server form, and what to do with no server. The core keeps the step, a
/// click goes there and Esc steps back. What's typed stays here while the step changes.
struct OnboardingView: View {
    let model: Model

    @State private var values = SettingsValues.empty
    @FocusState private var focus: String?

    var body: some View {
        let settings = model.state?.settings
        let onboarding = model.state?.onboarding
        Form {
            if let onboarding {
                switch onboarding.step {
                case "connect":
                    connect(onboarding.words, onboarding.connect, settings)
                case "no_server":
                    noServer(onboarding.words.noServer, cursor: onboarding.cursor, settings)
                default:
                    welcome(onboarding.words, cursor: onboarding.cursor, settings)
                }
            }
        }
        .formStyle(.grouped)
        .frame(maxWidth: 600)
        .frame(maxWidth: .infinity)
        .onChange(of: settings?.serial, initial: true) {
            guard settings != nil, let loaded = model.settingsValues else { return }
            values = loaded
            focusFields()
        }
        .onChange(of: onboarding?.step) { focusFields() }
        .onChange(of: onboarding?.connect.stage) { focusFields() }
        .onChange(of: onboarding?.connect.fields.map(\.key)) { focusFields() }
        // Typing asks nothing of a server: it only makes what was found, or is on its way, out of date.
        .onChange(of: values[field: "address"]) { _, address in model.addressEdited(address) }
    }

    /// What Connect has just put on screen. Anywhere else no field may keep the keyboard.
    private func focusFields() {
        guard let connect = model.state?.onboarding?.connect, model.state?.onboarding?.step == "connect" else {
            focus = nil
            return
        }
        if connect.stage == "by_hand" {
            focus = firstField(values, model.state?.settings?.providers ?? [])
        } else {
            let asked = connect.fields.first { values[field: $0.key].isEmpty } ?? connect.fields.last
            focus = asked?.key ?? "address"
        }
    }

    private func heading(_ title: String, _ text: String, logo: Bool = false) -> some View {
        Section {
            VStack(alignment: .leading, spacing: 6) {
                if logo { LogoMark(width: 72).padding(.bottom, 6) }
                Text(title).font(.largeTitle.bold())
                Text(text).foregroundStyle(.secondary)
            }
            .padding(.vertical, 4)
        }
    }

    @ViewBuilder
    private func welcome(_ words: FirstRunWords, cursor: Int, _ settings: SettingsInfo?) -> some View {
        heading(words.welcomeTitle, words.welcomeText, logo: true)
        // What was wrong with the config file, on whichever step you're at.
        MessageSection(settings?.problem, systemImage: "exclamationmark.triangle.fill")
        Section {
            ForEach(Array(words.welcome.enumerated()), id: \.offset) { index, row in
                RowButton(label: row.label, quiet: row.quiet, current: index == cursor) {
                    model.chooseOnboarding(index)
                }
            }
        }
    }

    @ViewBuilder
    private func noServer(_ words: NoServerInfo, cursor: Int, _ settings: SettingsInfo?) -> some View {
        heading(words.title, words.text)
        MessageSection(settings?.problem, systemImage: "exclamationmark.triangle.fill")
        ForEach(Array(words.offers.enumerated()), id: \.offset) { index, offer in
            Section(offer.name) {
                Text(offer.price).bold()
                Text(offer.text)
                RowButton(label: offer.link.label, quiet: false, current: index == cursor) {
                    model.chooseOnboarding(index)
                }
            }
        }
        Section {
            Text(words.note).foregroundStyle(.secondary)
            RowButton(label: words.more.label, quiet: false, current: cursor == words.offers.count) {
                model.chooseOnboarding(words.offers.count)
            }
            ShownLink(model: model)
            HStack {
                Spacer()
                Button("Back") { model.key("Escape") }
            }
        }
    }

    @ViewBuilder
    private func connect(_ words: FirstRunWords, _ view: ConnectInfo, _ settings: SettingsInfo?) -> some View {
        let checking = settings?.checking == true
        let byHand = view.stage == "by_hand"
        heading(words.connectTitle, byHand ? words.byHandText : words.connectText)
        MessageSection(settings?.problem, systemImage: "exclamationmark.triangle.fill")
        if byHand {
            FeedServerSections(model: model, providers: settings?.providers ?? [], values: $values, focus: $focus)
        } else {
            Section {
                FieldRow(field: words.address, values: $values, focus: $focus)
                    .disabled(checking)
                if !view.text.isEmpty { Text(verbatim: view.text) }
                ForEach(view.fields) {
                    FieldRow(field: $0, values: $values, focus: $focus)
                        .disabled(checking)
                }
            } footer: {
                VStack(alignment: .leading, spacing: 6) {
                    if !view.note.isEmpty { Text(verbatim: view.note) }
                    ForEach(([words.address] + view.fields).filter { !$0.help.isEmpty }) { Text(verbatim: $0.help) }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        MessageSection(settings?.error, systemImage: "xmark.octagon.fill")
        Section {
            ForEach(Array(view.rows.enumerated()), id: \.offset) { index, row in
                Button(row.label) { model.chooseOnboarding(index) }
                    .help(row.url ?? "")
                    .disabled(checking)
            }
            ShownLink(model: model)
        }
        HStack {
            if view.probing {
                ProgressView().controlSize(.small)
            }
            Spacer()
            Button("Back") { model.key("Escape") }
                .disabled(checking)
            Button(checking ? "Checking…" : view.probing ? "Looking…" : view.action) { model.submitSettings(values) }
                .keyboardShortcut(.defaultAction)
                .disabled(checking || view.probing)
        }
    }
}

/// A row of a first-run step. `current` is where the arrow keys are.
private struct RowButton: View {
    let label: String
    let quiet: Bool
    let current: Bool
    let action: () -> Void

    var body: some View {
        if quiet && !current {
            Button(action: action) { Text(label).foregroundStyle(.secondary) }
                .buttonStyle(.plain)
        } else if current {
            Button(action: action) { Text(label) }
                .buttonStyle(.borderedProminent)
        } else {
            Button(action: action) { Text(label) }
                .buttonStyle(.bordered)
        }
    }
}

/// The link just opened, as text to take by hand. `open` is reliable here, but a browser that's asked isn't one that
/// came up.
private struct ShownLink: View {
    let model: Model

    var body: some View {
        if let link = model.state?.shownLink {
            VStack(alignment: .leading, spacing: 2) {
                Text("If your browser didn't open, here's the address:").foregroundStyle(.secondary)
                Text(verbatim: link).textSelection(.enabled)
            }
        }
    }
}

private struct FieldRow: View {
    let field: FieldInfo
    @Binding var values: SettingsValues
    var focus: FocusState<String?>.Binding

    var body: some View {
        if field.secret {
            SecureField(field.label, text: $values[field: field.key])
                .focused(focus, equals: field.key)
        } else {
            TextField(field.label, text: $values[field: field.key],
                      prompt: field.placeholder.isEmpty ? nil : Text(verbatim: field.placeholder))
                .focused(focus, equals: field.key)
        }
    }
}

private struct FeedServerSections: View {
    let model: Model
    let providers: [ProviderInfo]
    @Binding var values: SettingsValues
    var focus: FocusState<String?>.Binding

    private var provider: ProviderInfo? { providers.first { $0.id == values.server } }

    var body: some View {
        Section {
            Picker("Server type", selection: $values.server) {
                ForEach(providers) { Text($0.shortLabel).tag($0.id) }
            }
            .onChange(of: values.server) { focus.wrappedValue = firstField(values, providers) }
            ForEach(provider?.fields ?? []) { FieldRow(field: $0, values: $values, focus: focus) }
        } footer: {
            VStack(alignment: .leading, spacing: 6) {
                ForEach((provider?.fields ?? []).filter { !$0.help.isEmpty }) { Text(verbatim: $0.help) }
                ForEach(provider?.links ?? []) { link in
                    Button(link.label) { model.openLink(link.url) }
                        .buttonStyle(.link)
                        .help(link.url)
                }
                ShownLink(model: model)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct ReadeckSection: View {
    let fields: [FieldInfo]
    @Binding var values: SettingsValues
    var focus: FocusState<String?>.Binding

    var body: some View {
        Section {
            ForEach(fields) { FieldRow(field: $0, values: $values, focus: focus) }
        } footer: {
            VStack(alignment: .leading, spacing: 6) {
                Text("Optional: leave empty to hide the bookmark button.")
                ForEach(fields.filter { !$0.help.isEmpty }) { Text(verbatim: $0.help) }
            }
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
