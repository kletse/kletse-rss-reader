// Onboarding (first run) and Settings: Miniflux is required, Readeck optional. Saving checks both servers before
// anything is written. Shown in place of the article; Esc cancels (the core decides), except on first run.
import SwiftUI

struct SettingsView: View {
    let model: Model

    @State private var minifluxUrl = ""
    @State private var minifluxApiKey = ""
    @State private var readeckUrl = ""
    @State private var readeckToken = ""
    @FocusState private var focus: Field?

    private enum Field { case minifluxUrl, minifluxApiKey }

    var body: some View {
        let settings = model.state?.settings
        let firstRun = model.state?.overlay == "onboarding"
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Text(firstRun ? "Welcome to Kletse RSS" : "Settings").font(.largeTitle.bold())
                    Text(firstRun
                        ? "Connect your Miniflux server to start reading. Readeck is optional: add it to save articles for later with b."
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
            Section {
                TextField("Readeck URL", text: $readeckUrl, prompt: Text("https://readeck.example.com"))
                SecureField("API token", text: $readeckToken)
            } header: {
                Text("Readeck")
            } footer: {
                Text("Optional: leave empty to hide the bookmark button. Profile → API Tokens (needs “Bookmarks: write”).")
            }
            if let error = settings?.error, !error.isEmpty {
                Section {
                    Text(error).foregroundStyle(.red)
                }
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
    }

    private func fill(_ values: SettingsValues?) {
        guard let values else { return }
        minifluxUrl = values.minifluxUrl
        minifluxApiKey = values.minifluxApiKey
        readeckUrl = values.readeckUrl
        readeckToken = values.readeckToken
        focus = minifluxUrl.isEmpty ? .minifluxUrl : .minifluxApiKey
    }

    private func submit() {
        model.submitSettings(minifluxUrl: minifluxUrl, minifluxApiKey: minifluxApiKey, readeckUrl: readeckUrl,
                             readeckToken: readeckToken)
    }
}
