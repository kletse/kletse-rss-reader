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
    /// "none", "picker", "help", "confirm", "settings" or "onboarding".
    let overlay: String
    let endTitle: String
    let windowTitle: String
    let selectionLabel: String
    let history: Bool
    let position: String
    let readeck: Bool
    let article: ArticleInfo?
    let picker: PickerInfo
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
}

struct PickerRow: Decodable {
    let label: String
    let count: Int
    let depth: Int
}

struct ToastInfo: Decodable {
    let message: String
    let error: Bool
    let serial: Int
}

struct SettingsInfo: Decodable {
    let problem: String
    let error: String
    let checking: Bool
    /// Bumped when `values` comes along: the form fills in from them once.
    let serial: Int
    let values: SettingsValues?
}

struct SettingsValues: Decodable {
    let minifluxUrl: String
    let minifluxApiKey: String
    let readeckUrl: String
    let readeckToken: String
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
