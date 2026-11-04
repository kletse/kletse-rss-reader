// The window: the toolbar, the reader (an article or a message; on first run, onboarding), and what goes over it:
// the feeds picker, the keys and the search box (sheets), the confirm dialog (an alert), the image viewer and
// toasts.
import SwiftUI

struct ContentView: View {
    @State private var model = Model.shared
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        let state = model.state
        Group {
            if state?.screen == "settings" {
                OnboardingView(model: model)
            } else {
                ReaderView(model: model)
            }
        }
        .overlay {
            if state?.overlay == "image", let viewer = state?.viewer {
                LightboxView(model: model, viewer: viewer)
            }
        }
        .overlay(alignment: .bottom) { ToastView(model: model) }
        // The title is the article, for Mission Control and the Window menu; the toolbar leaves it out, as the
        // article's own heading is right below. Where you are sits in the middle, as in the Qt top bar.
        .navigationTitle(state?.windowTitle ?? "Kletse RSS")
        .toolbar(removing: .title)
        .toolbar { if state?.screen != "settings" { toolbar(state) } }
        // One sheet for both: SwiftUI shows one at a time, and the core goes from the picker to the keys directly.
        .sheet(item: sheet) { sheet in
            switch sheet {
            case .picker: PickerView(model: model)
            case .help: HelpView(model: model)
            case .search: SearchView(model: model)
            }
        }
        .modifier(ConfirmAlert(model: model, overPicker: false))
        // Links in articles open through the core, which marks things read as it should.
        .environment(\.openURL, OpenURLAction { url in
            model.openLink(url.absoluteString)
            return .handled
        })
        .onChange(of: colorScheme) { model.themeChanged() }
        // Also brings it to the front when it's open already.
        .onChange(of: model.settingsRequests) { openSettings() }
    }

    @ToolbarContentBuilder
    private func toolbar(_ state: AppState?) -> some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            let label = state?.selectionLabel ?? ""
            Button { model.press("picker") } label: {
                // Reading read items or search results: the button says so with its icon, the middle of the bar
                // with a word.
                Label(label.isEmpty ? "Feeds" : label,
                      systemImage: state?.history == true ? "clock.arrow.circlepath"
                          : state?.searching == true ? "magnifyingglass" : "list.bullet")
                    .labelStyle(.titleAndIcon)
            }
            .help("Feeds (p)")
        }
        if let position = state?.position, !position.isEmpty {
            ToolbarItem(placement: .principal) {
                Text(state?.history == true ? "History · \(position)"
                    : state?.searching == true ? "Search · \(position)" : position)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
        }
        ToolbarItemGroup(placement: .primaryAction) {
            if state?.readeck == true {
                Button { model.press("readeck") } label: { Label("Readeck", systemImage: "bookmark") }
                    .help("Save to Readeck (b)")
            }
            ControlGroup {
                Button { model.press("prev") } label: { Label("Previous", systemImage: "chevron.up") }
                    .help("Previous (k)")
                Button { model.press("next") } label: { Label("Next", systemImage: "chevron.down") }
                    .help("Next, mark read (j)")
            }
        }
    }

    private enum Sheet: String, Identifiable {
        case picker, help, search
        var id: String { rawValue }
    }

    /// The sheet for the core's overlay; a dismissal by the system asks the core, which may keep the picker up
    /// (with nothing chosen yet, there is nothing to go back to).
    private var sheet: Binding<Sheet?> {
        Binding(
            get: { model.state.flatMap { $0.pickerShown ? .picker : Sheet(rawValue: $0.overlay) } },
            set: { shown in
                guard shown == nil else { return }
                switch model.state?.overlay {
                case "picker": model.pickerBackdrop()
                case "help": model.closeHelp()
                case "search": model.cancelSearch()
                default: break
                }
            }
        )
    }
}

/// The confirm dialog. Asked from the picker (Shift+A on a row), it comes up over the picker's sheet, which stays:
/// a window can't show an alert of its own while a sheet is up.
struct ConfirmAlert: ViewModifier {
    let model: Model
    let overPicker: Bool

    func body(content: Content) -> some View {
        content.alert(
            model.state?.confirmText ?? "",
            isPresented: Binding(
                get: { model.state?.overlay == "confirm" && model.state?.pickerShown == overPicker },
                set: { _ in }
            ),
            actions: {
                Button("Mark Read") { model.key("y") }
                Button("Cancel", role: .cancel) { model.key("n") }
            },
            message: { Text("Enter or y to confirm, Esc or n to cancel.") }
        )
    }
}

struct ToastView: View {
    let model: Model

    var body: some View {
        if let toast = model.state?.toast {
            Text(toast.message)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(.regularMaterial, in: Capsule())
                .overlay(Capsule().strokeBorder(toast.error ? Color.red : Brand.accent.opacity(0.6)))
                .padding(16)
                .transition(.opacity)
                // Errors stay up longer, to be read.
                .task(id: toast.serial) {
                    try? await Task.sleep(for: toast.error ? .seconds(6) : .milliseconds(2200))
                    model.dismissToast(toast.serial)
                }
        }
    }
}
