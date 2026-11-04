// Onboarding (first run) and Settings: a feed server is required (Miniflux, or a Google Reader API server such as
// FreshRSS), Readeck optional. Saving checks both servers before anything is written. Shown in place of the
// article; Esc cancels (the core decides), except on first run.
// One tab at a time (Server, Readeck): the core lists them and says which is open; a click asks it to switch, and
// Control+Tab reaches it as a key. A hidden tab's fields keep what is typed, and every field is sent.
import SwiftUI

struct SettingsView: View {
    let model: Model

    /// "miniflux" or "greader": which fields show. The other kind's are kept, for switching back.
    @State private var server = "miniflux"
    @State private var minifluxUrl = ""
    @State private var minifluxApiKey = ""
    @State private var greaderUrl = ""
    @State private var greaderUsername = ""
    @State private var greaderPassword = ""
    @State private var readeckUrl = ""
    @State private var readeckToken = ""
    @FocusState private var focus: Field?

    private enum Field {
        case minifluxUrl, minifluxApiKey, greaderUrl, greaderUsername, greaderPassword, readeckUrl, readeckToken
    }

    /// The tab on screen.
    private var section: String { model.state?.settings?.section ?? "server" }

    private func tab(_ name: String) -> SettingsTab? {
        model.state?.settings?.sections.first { $0.name == name }
    }

    var body: some View {
        let settings = model.state?.settings
        let firstRun = model.state?.overlay == "onboarding"
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Text(firstRun ? "Welcome to Kletse RSS" : "Settings").font(.largeTitle.bold())
                    Text(firstRun
                        ? "Connect your Miniflux or Google Reader API server to start reading. Readeck is optional: add it on its tab to save articles for later with b."
                        : "Changes are checked against your servers before they are saved.")
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            }
            // What was wrong with the config file, above the fields it is about.
            if let problem = settings?.problem, !problem.isEmpty {
                Section {
                    Label(problem, systemImage: "exclamationmark.triangle.fill")
                        .symbolRenderingMode(.multicolor)
                }
            }
            // The tabs, driven by the core's section.
            Section {
                Picker("Section", selection: Binding(get: { section }, set: { model.settingsSection($0) })) {
                    ForEach(settings?.sections ?? []) { Text($0.title).tag($0.name) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
            } footer: {
                Text(settings?.tabsHint ?? "")
            }
            // Under the tabs, whichever is open; a failed save opens the one it is about.
            if let error = settings?.error, !error.isEmpty {
                Section {
                    Text(error).foregroundStyle(.red)
                }
            }
            if section == "readeck" {
                readeckSection
            } else {
                serverSection
            }
            HStack {
                Spacer()
                if !firstRun {
                    Button("Cancel") { model.cancelSettings() }
                }
                Button(settings?.checking == true ? "Checking…" : (firstRun ? "Connect" : "Save"), action: submit)
                    .buttonStyle(.borderedProminent)
                    .disabled(settings?.checking == true)
            }
        }
        .formStyle(.grouped)
        .frame(maxWidth: 600)
        .frame(maxWidth: .infinity)
        .onSubmit(submit)
        // The form starts from the saved values once (the serial says when they are in); after that it keeps what
        // is typed, also when a check fails.
        .onChange(of: settings?.serial, initial: true) { fill(settings?.values) }
        .onChange(of: settings?.section) { focusFirst() }
    }

    private func header(_ name: String) -> Text {
        guard let tab = tab(name) else { return Text("") }
        return Text(verbatim: "\(tab.title) · \(tab.note)")
    }

    @ViewBuilder private var serverSection: some View {
        Section {
            Picker("Server", selection: $server) {
                Text("Miniflux").tag("miniflux")
                Text("Google Reader API (FreshRSS, …)").tag("greader")
            }
            .pickerStyle(.segmented)
            .onChange(of: server) { focusFirst() }
        } header: {
            header("server")
        }
        if server == "greader" {
            Section {
                TextField("Server URL", text: $greaderUrl,
                          prompt: Text("https://freshrss.example.com/api/greader.php"))
                    .focused($focus, equals: .greaderUrl)
                TextField("Username", text: $greaderUsername)
                    .focused($focus, equals: .greaderUsername)
                SecureField("Password", text: $greaderPassword)
                    .focused($focus, equals: .greaderPassword)
            } header: {
                Text("Google Reader API")
            } footer: {
                Text(verbatim: "Required. FreshRSS: https://your-freshrss/api/greader.php. Miniflux: the server URL, with the Google Reader integration enabled.")
            }
        } else {
            Section {
                TextField("Miniflux URL", text: $minifluxUrl, prompt: Text("https://miniflux.example.com"))
                    .focused($focus, equals: .minifluxUrl)
                SecureField("API key", text: $minifluxApiKey)
                    .focused($focus, equals: .minifluxApiKey)
            } header: {
                Text("Miniflux")
            } footer: {
                Text("Required. Settings → API Keys → Create a new API key.")
            }
        }
    }

    private var readeckSection: some View {
        Section {
            TextField("Readeck URL", text: $readeckUrl, prompt: Text("https://readeck.example.com"))
                .focused($focus, equals: .readeckUrl)
            SecureField("API token", text: $readeckToken)
                .focused($focus, equals: .readeckToken)
        } header: {
            header("readeck")
        } footer: {
            Text("Leave both empty to hide the bookmark button. Profile → API Tokens (needs “Bookmarks: write”).")
        }
    }

    private func fill(_ values: SettingsValues?) {
        guard let values else { return }
        server = values.server == "greader" ? "greader" : "miniflux"
        minifluxUrl = values.minifluxUrl
        minifluxApiKey = values.minifluxApiKey
        greaderUrl = values.greaderUrl
        greaderUsername = values.greaderUsername
        greaderPassword = values.greaderPassword
        readeckUrl = values.readeckUrl
        readeckToken = values.readeckToken
        focusFirst()
    }

    /// The tab's first empty field, or its last: on the Server tab, the chosen server's.
    private func focusFirst() {
        let fields: [(Field, String)] = section == "readeck"
            ? [(.readeckUrl, readeckUrl), (.readeckToken, readeckToken)]
            : server == "greader"
            ? [(.greaderUrl, greaderUrl), (.greaderUsername, greaderUsername), (.greaderPassword, greaderPassword)]
            : [(.minifluxUrl, minifluxUrl), (.minifluxApiKey, minifluxApiKey)]
        focus = (fields.first { $0.1.isEmpty } ?? fields[fields.count - 1]).0
    }

    private func submit() {
        model.submitSettings(SettingsValues(
            server: server, minifluxUrl: minifluxUrl, minifluxApiKey: minifluxApiKey, greaderUrl: greaderUrl,
            greaderUsername: greaderUsername, greaderPassword: greaderPassword, readeckUrl: readeckUrl,
            readeckToken: readeckToken))
    }
}
