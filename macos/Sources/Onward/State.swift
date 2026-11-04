// What the core sends back (the `Reply` in src/ffi.rs), and the article blocks (src/content/native.rs).
import Foundation

struct Reply: Decodable {
    let used: Bool
    let events: [String]
    let state: AppState
}

struct AppState: Decodable {
    /// "settings", "loading", "article", "end" or "empty".
    let screen: String
    /// "none", "picker", "help", "confirm", "settings", "onboarding", "search" or "image".
    let overlay: String
    /// The picker is on screen: open, or behind the question Shift+A on a row asks.
    let pickerShown: Bool
    let endTitle: String
    /// Where j goes from the end of the list.
    let endHint: String
    let windowTitle: String
    let selectionLabel: String
    let history: Bool
    /// Reading search results.
    let searching: Bool
    /// Reading the starred items.
    let starredList: Bool
    let position: String
    let readeck: Bool
    /// The server refreshes feeds (r) and fetches whole articles (f): Miniflux, not a Google Reader server.
    let fetches: Bool
    let article: ArticleInfo?
    let picker: PickerInfo
    /// The search box starts from the last search.
    let searchQuery: String
    let viewer: ViewerInfo?
    /// The next article's images, to load while this one is read.
    let prefetchImages: [String]
    /// `[keys, action]` pairs.
    let help: [[String]]
    let confirmText: String
    let toast: ToastInfo?
    let settings: SettingsInfo?
    let fontSize: Int
    let widthFactor: Int
}

struct ArticleInfo: Decodable {
    /// Changes for a new article or new text for it: back to the top.
    let serial: Int
    let id: Int
    let title: String
    let url: String
    let feedTitle: String
    let author: String
    let publishedAt: String
    let readingTime: Int
    let status: String
    /// Starred on the server (*).
    let starred: Bool
    /// Only when converted again; the model keeps the last ones.
    let blocks: [Block]?
}

struct PickerInfo: Decodable {
    /// "loading", "error" or "ready".
    let state: String
    let error: String
    let rows: [PickerRow]
    let cursor: Int
    let history: Bool
    /// Unread items come newest first.
    let newestFirst: Bool
}

/// The image viewer: the image on screen, the link it's in, and where it is among the article's images.
struct ViewerInfo: Decodable {
    let src: String
    let alt: String
    let href: String?
    let index: Int
    let count: Int
}

struct PickerRow: Decodable {
    let label: String
    /// nil when the server can't count these (read items on a Google Reader API server).
    let count: Int?
    let depth: Int
}

struct ToastInfo: Decodable {
    let message: String
    let error: Bool
    let serial: Int
}

struct SettingsInfo: Decodable {
    /// What the form offers.
    let kinds: [ServerKind]
    let problem: String
    let error: String
    let checking: Bool
    /// Bumped when `values` comes along: the form fills in from them once.
    let serial: Int
    let values: SettingsValues?
}

struct ServerKind: Decodable, Identifiable {
    let name: String
    let label: String
    var id: String { name }
}

struct SettingsValues: Decodable, Equatable {
    /// A `name` from `SettingsInfo.kinds`. The other kinds' fields are kept, for switching back.
    var server: String
    var minifluxUrl: String
    var minifluxApiKey: String
    var greaderUrl: String
    var greaderUsername: String
    var greaderPassword: String
    var readeckUrl: String
    var readeckToken: String

    static let empty = SettingsValues(
        server: "miniflux", minifluxUrl: "", minifluxApiKey: "", greaderUrl: "", greaderUsername: "",
        greaderPassword: "", readeckUrl: "", readeckToken: "")
}

enum Block: Decodable {
    case paragraph([Run])
    case heading(level: Int, runs: [Run])
    case image(src: String, alt: String, href: String?, caption: [Run]?)
    case caption([Run])
    case code([Run], lang: String?)
    case quote([Block])
    case list(ordered: Bool, start: Int, items: [[Block]])
    case table([[Cell]])
    case rule
    case embed(href: String, label: String)
    case unknown

    private enum Key: String, CodingKey {
        case type, runs, level, src, alt, href, caption, lang, blocks, ordered, start, items, rows, label
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .type) {
        case "paragraph": self = .paragraph(try c.decode([Run].self, forKey: .runs))
        case "heading": self = .heading(level: try c.decode(Int.self, forKey: .level), runs: try c.decode([Run].self, forKey: .runs))
        case "image":
            self = .image(
                src: try c.decode(String.self, forKey: .src),
                alt: try c.decode(String.self, forKey: .alt),
                href: try c.decodeIfPresent(String.self, forKey: .href),
                caption: try c.decodeIfPresent([Run].self, forKey: .caption)
            )
        case "caption": self = .caption(try c.decode([Run].self, forKey: .runs))
        case "code": self = .code(try c.decode([Run].self, forKey: .runs), lang: try c.decodeIfPresent(String.self, forKey: .lang))
        case "quote": self = .quote(try c.decode([Block].self, forKey: .blocks))
        case "list":
            self = .list(
                ordered: try c.decode(Bool.self, forKey: .ordered),
                start: try c.decode(Int.self, forKey: .start),
                items: try c.decode([[Block]].self, forKey: .items)
            )
        case "table": self = .table(try c.decode([[Cell]].self, forKey: .rows))
        case "rule": self = .rule
        case "embed": self = .embed(href: try c.decode(String.self, forKey: .href), label: try c.decode(String.self, forKey: .label))
        default: self = .unknown
        }
    }
}

struct Cell: Decodable {
    let header: Bool
    let colspan: Int
    let runs: [Run]
}

/// Styled text. Only what is set comes along.
struct Run: Decodable {
    let text: String
    let bold: Bool?
    let italic: Bool?
    let underline: Bool?
    let strike: Bool?
    let code: Bool?
    let small: Bool?
    /// "sup" or "sub".
    let script: String?
    let link: String?
    /// `#rrggbb`
    let color: String?
    let background: String?
}
