// The image viewer (`v`, or a click on an image): one of the article's images over the whole window, as large as
// fits but at most twice its own size. ← / → go between them and o opens one in the browser (the core takes the
// keys); a click or Esc closes it.
import AppKit
import SwiftUI

struct LightboxView: View {
    let model: Model
    let viewer: ViewerInfo

    @State private var image: NSImage?
    @State private var failed = false

    var body: some View {
        ZStack {
            Rectangle().fill(.black.opacity(0.88))
            VStack(spacing: 10) {
                Group {
                    if let image {
                        Image(nsImage: image)
                            .resizable()
                            .aspectRatio(contentMode: .fit)
                            .frame(maxWidth: image.size.width * 2, maxHeight: image.size.height * 2)
                    } else if failed {
                        Text("The image didn't load").foregroundStyle(.white.opacity(0.7))
                    } else {
                        ProgressView().controlSize(.large)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                if !viewer.alt.isEmpty {
                    Text(viewer.alt)
                        .foregroundStyle(.white)
                        .multilineTextAlignment(.center)
                        .lineLimit(3)
                }
                Text(hint)
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.6))
            }
            .padding(24)
        }
        .contentShape(Rectangle())
        .onTapGesture { model.closeViewer() }
        .pointerStyle(.link)
        .task(id: viewer.src) {
            image = nil
            failed = false
            image = await RemoteImage.image(viewer.src)
            failed = image == nil
        }
    }

    private var hint: String {
        var parts: [String] = []
        if viewer.count > 1 { parts += ["\(viewer.index + 1) / \(viewer.count)", "← / → between images"] }
        parts += ["o opens it in the browser", "Esc closes"]
        return parts.joined(separator: " · ")
    }
}
