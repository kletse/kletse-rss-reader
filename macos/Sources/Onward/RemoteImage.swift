// An article image: loaded in the background, at its natural size but never wider than the column. A click opens
// the image viewer (LightboxView), where o opens it, or the link it's in, in the browser.
import AppKit
import SwiftUI

struct RemoteImage: View {
    let src: String
    let alt: String

    @State private var image: NSImage?
    @State private var failed = false

    var body: some View {
        Group {
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: image.size.width)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
                    .pointerStyle(.link)
                    .onTapGesture { Model.shared.viewImage(src) }
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
        image = await Self.image(src)
        failed = image == nil
    }
}

extension RemoteImage {
    /// The image, from the cache or loaded into it. On the main actor, as the views that show it: `NSImage`
    /// isn't `Sendable`.
    @MainActor static func image(_ src: String) async -> NSImage? {
        if let cached = ImageCache.shared.object(forKey: src as NSString) {
            return cached
        }
        guard let data = await data(src), let loaded = NSImage(data: data), loaded.size.width > 0 else { return nil }
        ImageCache.shared.setObject(loaded, forKey: src as NSString)
        return loaded
    }

    /// Loads images into the cache ahead of time: the next article's, while this one is read.
    @MainActor static func prefetch(_ srcs: [String]) {
        for src in srcs where ImageCache.shared.object(forKey: src as NSString) == nil && !loading.contains(src) {
            loading.insert(src)
            Task {
                _ = await image(src)
                loading.remove(src)
            }
        }
    }

    /// Images being loaded ahead, so each is asked for once.
    @MainActor private static var loading = Set<String>()

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
