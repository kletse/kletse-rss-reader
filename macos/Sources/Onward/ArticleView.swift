// The article: its header (feed, author, date, read status, title, site), then the converted body. The column
// scales with the font size, as in CSS: `--article-size * --width-factor`.
import SwiftUI

struct ArticleView: View {
    let model: Model
    let article: ArticleInfo
    @State private var titleHovered = false

    var body: some View {
        let size = CGFloat(model.state?.fontSize ?? 18)
        let measure = size * CGFloat(model.state?.widthFactor ?? 40)
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline) {
                Text(meta)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Spacer(minLength: 12)
                Text(article.status.uppercased())
                    .font(.caption.weight(.medium))
                    .tracking(0.9)
                    .foregroundStyle(article.status == "unread" ? AnyShapeStyle(.tint) : AnyShapeStyle(.tertiary))
            }
            Text(article.title)
                .font(.system(size: size * 1.66, weight: .bold))
                .tracking(-0.015 * size * 1.66)
                .foregroundStyle(titleHovered ? AnyShapeStyle(.tint) : AnyShapeStyle(.primary))
                .padding(.top, 12)
                .padding(.bottom, 4)
                .fixedSize(horizontal: false, vertical: true)
                .onHover { titleHovered = $0 }
                .pointerStyle(.link)
                .onTapGesture { model.openLink(article.url) }
            Text(host(article.url))
                .font(.callout)
                .foregroundStyle(.tertiary)
                .lineLimit(1)
                .padding(.bottom, 32)
            BlocksView(blocks: model.blocks, size: size, gap: size * 1.1)
        }
        .frame(maxWidth: measure, alignment: .leading)
    }

    private var meta: String {
        var parts = [article.feedTitle, article.author, published(article.publishedAt)]
        if article.readingTime > 0 { parts.append("\(article.readingTime) min") }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    private func host(_ url: String) -> String {
        guard let host = URL(string: url)?.host() else { return url }
        return host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
    }

    private func published(_ iso: String) -> String {
        let parse = ISO8601DateFormatter()
        parse.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        guard let date = parse.date(from: iso) ?? ISO8601DateFormatter().date(from: iso) else { return "" }
        return date.formatted(.dateTime.day().month(.abbreviated).year().hour().minute())
    }
}
