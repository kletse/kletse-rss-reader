// The feeds picker, in a sheet: "All unread", categories, and their feeds with unread counts.
// In history mode the same, with read counts. The cursor and the rows come from the core; a click picks a row.
import SwiftUI

struct PickerView: View {
    let model: Model

    @State private var hovered: Int?

    var body: some View {
        let picker = model.state?.picker
        let history = picker?.history == true
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                modeSwitch(history)
                Spacer()
                if model.state?.capabilities.refresh != false {
                    Button { model.press("refresh") } label: { Image(systemName: "arrow.clockwise") }
                        .help("Refresh feeds (r)")
                }
                Button { model.press("search") } label: { Image(systemName: "magnifyingglass") }
                    .help("Search all feeds (/)")
            }
            .buttonStyle(.borderless)
            .padding(.horizontal, 14)
            .padding(.vertical, 6)
            Divider()
            if let picker, picker.state == "ready", !picker.rows.isEmpty {
                rows(picker)
            } else {
                Text(message(picker))
                    .foregroundStyle(picker?.state == "error" ? AnyShapeStyle(.red) : AnyShapeStyle(.secondary))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(14)
            }
            Divider()
            footer(picker, history: history)
        }
        .frame(idealWidth: 380, maxWidth: 380)
        .modifier(ConfirmAlert(model: model, overPicker: true))
    }

    /// The row keys are on, or the one under the mouse, more faintly: hovering doesn't move the keys' place.
    private func rowFill(_ row: Int, current: Bool) -> Color {
        if current { return Brand.accent }
        return hovered == row ? Color.primary.opacity(0.08) : .clear
    }

    /// Shows which list this is, and changes it.
    private func modeSwitch(_ history: Bool) -> some View {
        HStack(spacing: 16) {
            ModeTab(title: "Unread", selected: !history, help: "Unread items (h)") { model.press("unread") }
            ModeTab(title: "History", selected: history, help: "Read items, newest first (h)") { model.press("history") }
        }
    }

    private func footer(_ picker: PickerInfo?, history: Bool) -> some View {
        HStack(spacing: 8) {
            // History has only the one order, so this says it rather than offering a choice.
            if history {
                Label("Newest First", systemImage: "arrow.up.arrow.down")
                    .foregroundStyle(.secondary)
            } else {
                Button { model.press("order") } label: {
                    Label(picker?.newestFirst == true ? "Newest First" : "Oldest First",
                          systemImage: "arrow.up.arrow.down")
                }
                .help("Order of unread items (s)")
            }
            Spacer()
            Button { model.press("help") } label: { Image(systemName: "keyboard") }
                .help("Keys (?)")
            Button { model.press("settings") } label: { Image(systemName: "gearshape") }
                .help("Settings (,)")
        }
        .labelStyle(.titleAndIcon)
        .buttonStyle(.borderless)
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private func rows(_ picker: PickerInfo) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(Array(picker.rows.enumerated()), id: \.offset) { i, row in
                        let current = i == picker.cursor
                        HStack {
                            Text(row.label)
                                .fontWeight(row.depth == 0 ? .semibold : .regular)
                                .lineLimit(1)
                                .padding(.leading, row.depth == 1 ? 16 : 0)
                            Spacer(minLength: 12)
                            Text(row.count.map(String.init) ?? "")
                                .monospacedDigit()
                                .foregroundStyle(current ? AnyShapeStyle(Brand.onAccent.opacity(0.85)) : AnyShapeStyle(.secondary))
                        }
                        .foregroundStyle(current ? AnyShapeStyle(Brand.onAccent) : AnyShapeStyle(.primary))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(rowFill(i, current: current), in: RoundedRectangle(cornerRadius: 6))
                        .contentShape(Rectangle())
                        .onHover { inside in
                            if inside { hovered = i } else if hovered == i { hovered = nil }
                        }
                        .onTapGesture { model.chooseRow(i) }
                        .padding(.top, row.depth == 0 && i > 0 ? 6 : 0)
                        .id(i)
                    }
                }
                .padding(6)
            }
            .frame(maxHeight: 520)
            .fixedSize(horizontal: false, vertical: true)
            .onAppear { proxy.scrollTo(picker.cursor) }
            .onChange(of: picker.cursor) { proxy.scrollTo(picker.cursor) }
        }
    }

    private func message(_ picker: PickerInfo?) -> String {
        switch picker?.state {
        case "error": return picker?.error ?? ""
        case "ready":
            if picker?.history == true { return "Nothing read yet. Press h for unread items." }
            return model.state?.capabilities.refresh == false ? "Nothing unread. Press h for history."
                : "Nothing unread. Press r to refresh, or h for history."
        default: return "Loading…"
        }
    }
}

/// The search box (`/`): Enter searches every feed, read and unread; Esc goes back (the core takes it).
/// A word that lights up under the mouse, and when chosen stands on the line under the header in the accent colour.
/// Always the same weight, so the other tab doesn't shift.
private struct ModeTab: View {
    let title: String
    let selected: Bool
    let help: String
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            Text(title)
                .fontWeight(.semibold)
                .foregroundStyle(selected || hovered ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
                .overlay(alignment: .bottom) {
                    Rectangle()
                        .fill(selected ? Brand.accent : .clear)
                        .frame(height: 2)
                        .offset(y: 7)
                }
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help(help)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

struct SearchView: View {
    let model: Model
    @State private var query = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Search").font(.headline)
            TextField("Words to look for, in every feed", text: $query)
                .textFieldStyle(.roundedBorder)
                .focused($focused)
                .onSubmit { model.search(query) }
            HStack {
                Text("Read and unread items, best match first.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Cancel") { model.cancelSearch() }
                Button("Search") { model.search(query) }
                    .buttonStyle(.borderedProminent)
            }
        }
        .padding(20)
        .frame(idealWidth: 440, maxWidth: 440)
        .onAppear {
            query = model.state?.searchQuery ?? ""
            focused = true
        }
    }
}

/// The keys, from the same table the keymaps are checked against (src/keys.rs).
struct HelpView: View {
    let model: Model

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Keys").font(.title2.bold())
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 20, verticalSpacing: 6) {
                ForEach(Array((model.state?.help ?? []).enumerated()), id: \.offset) { _, line in
                    GridRow {
                        Text(line.first ?? "")
                            .font(.body.monospaced())
                            .foregroundStyle(.tint)
                        Text(line.last ?? "")
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            HStack {
                Spacer()
                Button("Done") { model.closeHelp() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(idealWidth: 560, maxWidth: 560)
    }
}
