// A row of the list Connect opens on, and its tile: `icon` from the table in src/providers.rs, in the system's colours. It's
// a letter and not a logo, as neither project's terms clearly allow a logo here (see AGENTS.md), and it mustn't look like
// one: `ServiceIcon` is all that changes if that ever does.
import SwiftUI

struct ServiceIcon: View {
    let icon: String
    let current: Bool

    var body: some View {
        Text(verbatim: icon)
            .font(.title3.weight(.semibold))
            .foregroundStyle(current ? Color.accentColor : Color.primary)
            .frame(width: 32, height: 32)
            .background(.quaternary, in: RoundedRectangle(cornerRadius: 7))
    }
}

/// `current` is where the arrow keys are.
struct ServiceRow: View {
    let row: ChoiceRowInfo
    let current: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                if !row.icon.isEmpty { ServiceIcon(icon: row.icon, current: current) }
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: row.label).fontWeight(current ? .semibold : .regular)
                    if !row.line.isEmpty {
                        Text(verbatim: row.line).font(.callout).foregroundStyle(.secondary)
                    }
                }
                Spacer(minLength: 0)
                if current { Image(systemName: "chevron.right").foregroundStyle(.tertiary) }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
