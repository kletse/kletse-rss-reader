// The converted article (src/content/native.rs), laid out top to bottom. Text is New York, headings the system
// font, code SF Mono, all at the article size. Quotes and list items hold blocks of their own.
import AppKit
import SwiftUI

struct BlocksView: View {
    let blocks: [Block]
    let size: CGFloat
    let gap: CGFloat
    var secondary = false

    var body: some View {
        VStack(alignment: .leading, spacing: gap) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                view(for: block)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var body1: TextLook { TextLook(size: size * 0.95, design: .serif) }

    @ViewBuilder
    private func view(for block: Block) -> some View {
        switch block {
        case .paragraph(let runs):
            RichText(runs: runs, look: body1, secondary: secondary)
        case .heading(let level, let runs):
            let scale: [CGFloat] = [1.5, 1.5, 1.3, 1.15, 1.05, 1, 1]
            RichText(runs: runs, look: TextLook(size: size * scale[min(level, 6)], design: .default, weight: .bold,
                                                lineSpacing: 0.15))
                .padding(.top, size * 0.7)
        case .image(let src, let alt, let href, let caption):
            VStack(spacing: 8) {
                RemoteImage(src: src, alt: alt, href: href)
                if let caption {
                    RichText(runs: caption, look: TextLook(size: size * 0.8, design: .serif), secondary: true)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: .infinity)
                }
            }
            .frame(maxWidth: .infinity)
        case .caption(let runs):
            RichText(runs: runs, look: TextLook(size: size * 0.8, design: .serif), secondary: true)
                .multilineTextAlignment(.center)
                .frame(maxWidth: .infinity)
        case .code(let runs, _):
            ScrollView(.horizontal) {
                Text(attributed(runs, TextLook(size: size * 0.95 * 0.8, design: .monospaced, lineSpacing: 0.25)))
                    .textSelection(.enabled)
                    .fixedSize()
                    .padding(.horizontal, 16)
                    .padding(.vertical, 14)
            }
            .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
        case .quote(let blocks):
            BlocksView(blocks: blocks, size: size, gap: gap * 0.6, secondary: true)
                .padding(.leading, size)
                .overlay(alignment: .leading) {
                    Rectangle().fill(.tint).frame(width: 3)
                }
        case .list(let ordered, let start, let items):
            VStack(alignment: .leading, spacing: gap * 0.4) {
                ForEach(Array(items.enumerated()), id: \.offset) { i, item in
                    HStack(alignment: .firstTextBaseline, spacing: size * 0.6) {
                        Text(ordered ? "\(start + i)." : "•")
                            .font(.system(size: size * 0.95, design: .serif))
                            .foregroundStyle(.secondary)
                            .frame(width: size * 1.4, alignment: .trailing)
                        BlocksView(blocks: item, size: size, gap: gap * 0.6, secondary: secondary)
                    }
                }
            }
        case .table(let rows):
            ScrollView(.horizontal) {
                Grid(alignment: .topLeading, horizontalSpacing: 16, verticalSpacing: 8) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { r, row in
                        if r > 0 { Divider() }
                        GridRow {
                            ForEach(Array(row.enumerated()), id: \.offset) { _, cell in
                                Text(attributed(cell.runs, TextLook(size: size * 0.85, design: .serif,
                                                                    weight: cell.header ? .semibold : .regular)))
                                    .textSelection(.enabled)
                                    .frame(maxWidth: 320, alignment: .leading)
                                    .fixedSize(horizontal: false, vertical: true)
                                    .gridCellColumns(max(cell.colspan, 1))
                            }
                        }
                    }
                }
                .padding(.vertical, 4)
            }
        case .rule:
            Divider().padding(.vertical, size * 0.5)
        case .embed(let href, let label):
            Button(label) { openLink(href) }
                .buttonStyle(.bordered)
                .pointerStyle(.link)
        case .unknown:
            EmptyView()
        }
    }

    @Environment(\.openURL) private var openURL

    private func openLink(_ href: String) {
        if let url = URL(string: href) { openURL(url) }
    }
}

/// How a run of text looks before its own style (bold, code, …) applies.
struct TextLook {
    var size: CGFloat
    var design: Font.Design
    var weight: Font.Weight = .regular
    /// Extra space between lines, as a fraction of the size.
    var lineSpacing: CGFloat = 0.55
}

/// One block of selectable text; links open through the window's `openURL`, which goes to the core.
struct RichText: View {
    let runs: [Run]
    let look: TextLook
    var secondary = false

    var body: some View {
        Text(attributed(runs, look))
            .lineSpacing(look.size * look.lineSpacing)
            .foregroundStyle(secondary ? .secondary : .primary)
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
    }
}

func attributed(_ runs: [Run], _ look: TextLook) -> AttributedString {
    var out = AttributedString()
    for run in runs {
        var piece = AttributedString(run.text)
        var size = look.size
        if run.code == true { size *= 0.85 }
        if run.small == true { size *= 0.85 }
        if run.script != nil { size *= 0.75 }
        let design: Font.Design = run.code == true ? .monospaced : look.design
        var font = Font.system(size: size, weight: run.bold == true ? .bold : look.weight, design: design)
        if run.italic == true { font = font.italic() }
        piece.font = font
        if let color = run.color.flatMap(Color.init(hex:)) { piece.foregroundColor = color }
        if let background = run.background.flatMap(Color.init(hex:)) { piece.backgroundColor = background }
        if run.underline == true { piece.underlineStyle = .single }
        if run.strike == true { piece.strikethroughStyle = .single }
        switch run.script {
        case "sup": piece.baselineOffset = size * 0.45
        case "sub": piece.baselineOffset = -size * 0.2
        default: break
        }
        if let link = run.link, let url = URL(string: link) { piece.link = url }
        out += piece
    }
    return out
}

extension Color {
    /// `#rrggbb`
    init?(hex: String) {
        let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard digits.count == 6, let v = UInt32(digits, radix: 16) else { return nil }
        self.init(.sRGB, red: Double(v >> 16 & 0xFF) / 255, green: Double(v >> 8 & 0xFF) / 255,
                  blue: Double(v & 0xFF) / 255)
    }
}
