// The window: the toolbar, the reader (an article or a message; on first run, onboarding), and what goes over it: the
// feeds picker, the keys and the search box (`Floating`), the confirm dialog (an alert), the image viewer and toasts.
// The floating ones aren't sheets: a sheet disables the window's red button, ignores clicks beside it and holds up a
// quit from the Dock.
import SwiftUI

struct ContentView: View {
    @State private var model = Model.shared
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.openSettings) private var openSettings

    /// Something floats over the reader.
    private var floating: Bool {
        model.state?.pickerShown == true || ["help", "search"].contains(model.state?.overlay)
    }

    var body: some View {
        let state = model.state
        Group {
            if state?.screen == "settings" {
                OnboardingView(model: model)
            } else {
                ReaderView(model: model)
            }
        }
        .allowsHitTesting(!floating)
        .overlay {
            if state?.pickerShown == true {
                Floating(dismiss: model.pickerBackdrop) {
                    ViewThatFits(in: .vertical) {
                        PickerView(model: model)
                        ScrollView { PickerView(model: model) }
                    }
                }
            } else if state?.overlay == "help" {
                Floating(dismiss: model.closeHelp) {
                    ViewThatFits(in: .vertical) {
                        HelpView(model: model)
                        ScrollView { HelpView(model: model) }
                    }
                }
            } else if state?.overlay == "search" {
                Floating(dismiss: model.cancelSearch) { SearchView(model: model) }
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
        .navigationTitle(state?.windowTitle ?? "Onward")
        .toolbar(removing: .title)
        .toolbar { if state?.screen != "settings" { toolbar(state) } }
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
                // The logo, except reading read items, search results or the starred list: the button says so with
                // its icon, the middle of the bar with a word.
                Label {
                    Text(label.isEmpty ? "Feeds" : label)
                } icon: {
                    if state?.history == true {
                        Image(systemName: "clock.arrow.circlepath")
                    } else if state?.searching == true {
                        Image(systemName: "magnifyingglass")
                    } else if state?.starredList == true {
                        Image(systemName: "star")
                    } else {
                        LogoMark(width: 22)
                    }
                }
                .labelStyle(.titleAndIcon)
            }
            .help("Feeds (p)")
            .disabled(floating && state?.pickerShown != true)
        }
        // Always there, also with nothing in it: without a middle item the buttons on the right slide over to the left.
        ToolbarItem(placement: .principal) {
            let position = state?.position ?? ""
            HStack(spacing: 6) {
                // The article on screen is starred (*).
                if state?.screen == "article", state?.article?.starred == true {
                    Image(systemName: "star.fill")
                        .foregroundStyle(.tint)
                        .help("Starred (*)")
                }
                Text(position.isEmpty ? " " : state?.history == true ? "History · \(position)"
                    : state?.searching == true ? "Search · \(position)"
                    : state?.starredList == true ? "Starred · \(position)" : position)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                if state?.samples == true {
                    Text("Sample articles").foregroundStyle(.tint)
                }
            }
        }
        ToolbarItemGroup(placement: .primaryAction) {
            // The samples aren't your feeds, and this is the way out of them.
            if state?.samples == true {
                Button("Connect") { model.press("settings") }
                    .help("Connect your server (,)")
                    .disabled(floating)
            }
            if state?.readeck == true {
                Button { model.press("readeck") } label: { Label("Readeck", systemImage: "bookmark") }
                    .help("Save to Readeck (b)")
                    .disabled(floating)
            }
            ControlGroup {
                Button { model.press("prev") } label: { Label("Previous", systemImage: "chevron.up") }
                    .help("Previous (k)")
                Button { model.press("next") } label: { Label("Next", systemImage: "chevron.down") }
                    .help("Next, mark read (j)")
            }
            .disabled(floating)
        }
    }
}

/// A panel over the dimmed reader, in the middle of the window. A click beside it is `dismiss`.
private struct Floating<Content: View>: View {
    let dismiss: () -> Void
    @ViewBuilder let content: Content

    var body: some View {
        ZStack {
            // The dimming goes under the toolbar, but clicks there are left to the window, which is dragged from it.
            Color.black.opacity(0.3)
                .ignoresSafeArea()
                .allowsHitTesting(false)
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture(perform: dismiss)
            content
                .background(Brand.panel)
                .clipShape(RoundedRectangle(cornerRadius: 14))
                .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(.separator))
                .shadow(radius: 24, y: 8)
                .padding(12)
        }
    }
}

/// The confirm dialog. Asked from the picker (Shift+A on a row), it comes up with the picker still there.
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
