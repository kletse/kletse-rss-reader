// Everything under the toolbar but the settings: the article, "Loading…" or the end of the list, in one
// scrolling area. Scrolling is the system's own; Space pages through the core (`scroll_up`/`scroll_down`).
import SwiftUI

struct ReaderView: View {
    let model: Model
    @State private var position = ScrollPosition(edge: .top)
    /// Where the scroll is: offset, visible height, content height.
    @State private var scroll = (y: CGFloat(0), height: CGFloat(0), content: CGFloat(0))

    var body: some View {
        GeometryReader { outer in
            ScrollView {
                content
                    .padding(.horizontal, outer.size.width < 560 ? 16 : 24)
                    .padding(.top, outer.size.width < 560 ? 24 : 40)
                    // Room to scroll the last lines up to the middle, as the web version had.
                    .padding(.bottom, outer.size.height * 0.3)
                    .frame(maxWidth: .infinity)
            }
            .scrollPosition($position)
            .onScrollGeometryChange(for: [CGFloat].self) { g in
                [g.contentOffset.y, g.containerSize.height, g.contentSize.height]
            } action: { _, g in
                scroll = (g[0], g[1], g[2])
                model.atBottom = g[0] + g[1] >= g[2] - 4
            }
        }
        .onChange(of: model.scrollSerial) { page(up: model.scrollUp) }
        .onChange(of: model.state?.article?.serial) { position.scrollTo(edge: .top) }
        .onChange(of: model.state?.screen) { position.scrollTo(edge: .top) }
    }

    @ViewBuilder
    private var content: some View {
        switch model.state?.screen {
        case "article":
            if let article = model.state?.article {
                ArticleView(model: model, article: article)
            }
        case "loading":
            ProgressView("Loading…")
                .padding(.top, 120)
        case "end":
            ContentUnavailableView {
                Label(model.state?.endTitle ?? "", systemImage: "checkmark.circle")
            } description: {
                Text("Press j or Enter for the feeds, k to go back.")
            }
            .padding(.top, 80)
        default:
            EmptyView()
        }
    }

    /// A page, less a few lines to keep your place, as browsers scroll on Space.
    private func page(up: Bool) {
        let step = max(scroll.height - 80, scroll.height * 0.8)
        let maxY = max(0, scroll.content - scroll.height)
        let y = min(maxY, max(0, scroll.y + (up ? -step : step)))
        withAnimation(.easeOut(duration: 0.18)) { position.scrollTo(y: y) }
    }
}
