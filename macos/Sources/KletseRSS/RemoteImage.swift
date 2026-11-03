// An article image: loaded in the background, at its natural size but never wider than the column, clickable
// (its link, or the image itself).
import AppKit
import SwiftUI

struct RemoteImage: View {
    let src: String
    let alt: String
    let href: String?

    @State private var image: NSImage?
    @State private var failed = false
    @Environment(\.openURL) private var openURL

    var body: some View {
        Group {
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: image.size.width)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
                    .pointerStyle(.link)
                    .onTapGesture { if let url = URL(string: href ?? src) { openURL(url) } }
            } else if failed {
                Text("image didn't load: \(alt.isEmpty ? src : alt)")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            } else {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.primary.opacity(0.05))
                    .frame(height: 160)
            }
        }
        .frame(maxWidth: .infinity)
        .task(id: src) { await load() }
    }

    private func load() async {
        image = nil
        failed = false
        if let cached = ImageCache.shared.object(forKey: src as NSString) {
            image = cached
            return
        }
        guard let data = await Self.data(src), let loaded = NSImage(data: data), loaded.size.width > 0 else {
            failed = true
            return
        }
        ImageCache.shared.setObject(loaded, forKey: src as NSString)
        image = loaded
    }
}

extension RemoteImage {
    /// The image's bytes. `data:` URLs are decoded here: feeds write them with raw spaces and quotes, which
    /// `URL` turns down.
    static func data(_ src: String) async -> Data? {
        if src.hasPrefix("data:") {
            guard let comma = src.firstIndex(of: ",") else { return nil }
            let header = src[..<comma].lowercased()
            let body = String(src[src.index(after: comma)...])
            if header.hasSuffix(";base64") {
                return Data(base64Encoded: body, options: .ignoreUnknownCharacters)
            }
            return (body.removingPercentEncoding ?? body).data(using: .utf8)
        }
        guard let url = URL(string: src) ?? src.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed)
            .flatMap(URL.init(string:))
        else { return nil }
        return try? await URLSession.shared.data(from: url).0
    }
}

enum ImageCache {
    nonisolated(unsafe) static let shared: NSCache<NSString, NSImage> = {
        let cache = NSCache<NSString, NSImage>()
        cache.countLimit = 200
        return cache
    }()
}
