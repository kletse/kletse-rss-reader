// The feeds picker, in a sheet: "All unread", categories, and their feeds with unread counts.
// In history mode the same, with read counts. The cursor and the rows come from the core; a click picks a row.
import SwiftUI

struct PickerView: View {
    let model: Model

    var body: some View {
        let picker = model.state?.picker
        let history = picker?.history == true
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text(history ? "History" : "Feeds").font(.headline)
                Spacer()
                Button { model.press("search") } label: { Image(systemName: "magnifyingglass") }
                    .help("Search all feeds (/)")
                if !history {
                    Button(picker?.newestFirst == true ? "Newest First" : "Oldest First") { model.press("order") }
                        .help("Order of unread items (s)")
                }
                Button { model.press("help") } label: { Image(systemName: "keyboard") }
                    .help("Keys (?)")
                Button(history ? "Unread" : "History") { model.press("mode") }
                    .help(history ? "Unread items (h)" : "Read items, newest first (h)")
                Button { model.press("settings") } label: { Image(systemName: "gearshape") }
                    .help("Settings (,)")
            }
            .buttonStyle(.borderless)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
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
        }
        .frame(width: 380)
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
                            Text("\(row.count)")
                                .monospacedDigit()
                                .foregroundStyle(current ? AnyShapeStyle(.white.opacity(0.85)) : AnyShapeStyle(.secondary))
                        }
                        .foregroundStyle(current ? AnyShapeStyle(.white) : AnyShapeStyle(.primary))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(current ? Color.accentColor : .clear, in: RoundedRectangle(cornerRadius: 6))
                        .contentShape(Rectangle())
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
            return picker?.history == true ? "Nothing read yet. Press h for unread items."
                : "Nothing unread. Press r to refresh, or h for history."
        default: return "Loading…"
        }
    }
}

/// The search box (`/`): Enter searches every feed, read and unread; Esc goes back (the core takes it).
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
        .frame(width: 440)
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
        .frame(width: 560)
    }
}
