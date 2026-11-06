// The feeds panel: every feed and category, for changing them. The core says what's on screen (`manage::ManageView`):
// the rows, what can be done on the one the cursor's on, and the field on the stages that have one. Keys reach the core
// through `Keyboard`, so the buttons have no shortcuts of their own (see AGENTS.md on menu items); the field's keys
// stay the field's, as the core leaves them alone while it's `typing`.
import SwiftUI

struct ManagePanel: View {
    let model: Model
    let manage: ManageInfo

    @State private var hovered: Int?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(manage.title).font(.headline)
            if manage.stage == "loading", manage.error.isEmpty {
                Text("Loading…").foregroundStyle(.secondary)
            }
            if !manage.text.isEmpty {
                Text(manage.text)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let field = manage.field {
                // Its own identity per field, so that each starts from what the core gives it.
                ManageTextField(model: model, field: field)
                    .id("\(manage.title) \(field.label)")
            }
            if !manage.error.isEmpty {
                Text(manage.error)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !manage.rows.isEmpty {
                rows
            }
            if !manage.notice.isEmpty, manage.stage == "list" || manage.stage == "working" {
                Text(manage.notice)
                    .foregroundStyle(manage.worrying ? AnyShapeStyle(.red) : AnyShapeStyle(.secondary))
                    .fixedSize(horizontal: false, vertical: true)
            }
            buttons
        }
        .padding(20)
        .frame(idealWidth: 440, maxWidth: 440)
    }

    private var rows: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(manage.rows.enumerated()), id: \.offset) { index, row in
                        HStack {
                            Text(row.label)
                                .fontWeight(row.depth == 0 ? .semibold : .regular)
                                .lineLimit(1)
                                .padding(.leading, row.depth == 1 ? 18 : 0)
                            Spacer()
                            Text(row.note).foregroundStyle(.secondary).monospacedDigit()
                        }
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(fill(index))
                        .contentShape(Rectangle())
                        .onHover { hovered = $0 ? index : (hovered == index ? nil : hovered) }
                        .onTapGesture { model.manageRow(index) }
                        .id(index)
                    }
                }
            }
            .frame(maxHeight: 360)
            .onChange(of: manage.cursor) { proxy.scrollTo(manage.cursor) }
        }
    }

    private func fill(_ row: Int) -> Color {
        if row == manage.cursor { return Brand.accent.opacity(0.3) }
        return hovered == row ? Color.primary.opacity(0.08) : .clear
    }

    private var buttons: some View {
        HStack {
            if manage.stage == "working" {
                ProgressView().controlSize(.small)
            }
            if manage.stage == "list" {
                ForEach(manage.keys, id: \.button) { key in
                    Button(key.label) { model.press(key.button) }
                        .help("\(key.label) (\(key.key))")
                }
            }
            Spacer()
            Button(manage.stage == "field" || manage.stage == "choose" ? "Back" : "Close") { model.cancelManage() }
        }
    }
}

/// Where what's typed lives until it's sent.
private struct ManageTextField: View {
    let model: Model
    let field: ManageFieldInfo

    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            TextField(field.label, text: $text)
                .textFieldStyle(.roundedBorder)
                .focused($focused)
                .onSubmit { model.submitManage(text) }
            HStack {
                Spacer()
                Button(field.action) { model.submitManage(text) }
                    .buttonStyle(.borderedProminent)
            }
        }
        .onAppear {
            text = field.value
            focused = true
        }
    }
}
