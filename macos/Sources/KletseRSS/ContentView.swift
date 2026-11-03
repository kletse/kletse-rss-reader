// The window: the toolbar, the reader (an article, a message or the settings form), and what goes over it:
// the feeds picker (a popover), the keys (a sheet), the confirm dialog (an alert) and toasts.
import SwiftUI

struct ContentView: View {
    @State private var model = Model.shared
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        let state = model.state
        Group {
            if state?.screen == "settings" {
                SettingsView(model: model)
            } else {
                ReaderView(model: model)
            }
        }
        .overlay(alignment: .bottom) { ToastView(model: model) }
        // The title is the article, for Mission Control and the Window menu; the toolbar leaves it out, as the
        // article's own heading is right below. Where you are sits in the middle, as in the Qt top bar.
        .navigationTitle(state?.windowTitle ?? "Kletse RSS")
        .toolbar(removing: .title)
        .toolbar { if state?.screen != "settings" { toolbar(state) } }
        .sheet(isPresented: binding("help", dismiss: model.closeHelp)) { HelpView(model: model) }
        .alert(
            state?.confirmText ?? "",
            isPresented: binding("confirm", dismiss: {}),
            actions: {
                Button("Mark Read") { model.key("y") }
                Button("Cancel", role: .cancel) { model.key("n") }
            },
            message: { Text("Enter or y to confirm, Esc or n to cancel.") }
        )
        // Links in articles open through the core, which marks things read as it should.
        .environment(\.openURL, OpenURLAction { url in
            model.openLink(url.absoluteString)
            return .handled
        })
        .onChange(of: colorScheme) { model.themeChanged() }
    }

    @ToolbarContentBuilder
    private func toolbar(_ state: AppState?) -> some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            let label = state?.selectionLabel ?? ""
            Button { model.press("picker") } label: {
                // Reading read items: the button says so with its icon, the middle of the bar with a word.
                Label(label.isEmpty ? "Feeds" : label,
                      systemImage: state?.history == true ? "clock.arrow.circlepath" : "list.bullet")
                    .labelStyle(.titleAndIcon)
            }
            .help("Feeds (p)")
            .popover(isPresented: binding("picker", dismiss: model.pickerBackdrop), arrowEdge: .bottom) {
                PickerView(model: model)
            }
        }
        if let position = state?.position, !position.isEmpty {
            ToolbarItem(placement: .principal) {
                Text(state?.history == true ? "History · \(position)" : position)
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

    /// Shown while the core has `overlay` up; a dismissal by the system (a click outside the popover) asks it.
    private func binding(_ overlay: String, dismiss: @escaping () -> Void) -> Binding<Bool> {
        Binding(
            get: { model.state?.overlay == overlay },
            set: { shown in if !shown && model.state?.overlay == overlay { dismiss() } }
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
                .overlay(Capsule().strokeBorder(toast.error ? Color.red : Color.accentColor.opacity(0.6)))
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
