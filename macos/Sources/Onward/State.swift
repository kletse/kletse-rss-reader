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
    let capabilities: Capabilities
    /// Reading the sample articles, chosen on the first screen.
    let samples: Bool
    /// The last link opened from a form: a browser may not have opened.
    let shownLink: String?
    /// First run, while it's on.
    let onboarding: OnboardingInfo?
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
    /// How to answer it, what its button says, and whether what it does can be taken back.
    let confirmHint: String
    let confirmAction: String
    let confirmLasting: Bool
    /// Importing or exporting a feed list.
    let transfer: TransferInfo?
    /// The feeds panel, while it's open or a change from it is under way.
    let manage: ManageInfo?
    /// It's on screen: open, or behind the question before a loss.
    let manageShown: Bool
    /// Anything about the feeds can be changed here.
    let manages: Bool
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

/// What the server can do on request. The core also sends `search`, which nothing here reads.
struct Capabilities: Decodable {
    /// Fetches the whole article (f).
    let fetchFull: Bool
    /// Refreshes the feeds (r).
    let refresh: Bool
    /// Takes an OPML file (i).
    let importFeeds: Bool
    /// Gives one (e).
    let exportFeeds: Bool
}

/// The import and export panel (`app::TransferView`).
struct TransferInfo: Decodable {
    /// "asking", "working" or "done".
    let stage: String
    let title: String
    let text: String
    let error: String
    /// What the main button says, while asking.
    let action: String
    /// What to call the file, where one is being made.
    let suggested: String
}

/// The feeds panel (`manage::ManageView`).
struct ManageInfo: Decodable {
    /// "loading", "list", "field", "choose" or "working".
    let stage: String
    let title: String
    let text: String
    /// What the last change came to, and whether that needs reading.
    let notice: String
    let worrying: Bool
    let error: String
    let rows: [ManageRowInfo]
    let cursor: Int
    let keys: [ManageKeyInfo]
    let field: ManageFieldInfo?
}

struct ManageRowInfo: Decodable {
    let label: String
    /// Beside it: a feed's unread items, a category's feeds.
    let note: String
    /// 0 for a category, 1 for a feed.
    let depth: Int
}

/// Something that can be done here: the key, and what a button calls it and sends to `press`.
struct ManageKeyInfo: Decodable {
    let key: String
    let label: String
    let button: String
}

/// The text field, on the stages that have one. `value` is where it starts: what was typed, after a failure.
struct ManageFieldInfo: Decodable {
    let label: String
    let value: String
    let action: String
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
    /// What it says with no rows, and whether it then offers to import, or to add a feed.
    let emptyText: String
    let offersImport: Bool
    let offersAdd: Bool
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

/// Where first run is, and all its words (src/providers.rs).
struct OnboardingInfo: Decodable {
    /// "welcome", "connect" or "no_server".
    let step: String
    /// The row the arrows are on.
    let cursor: Int
    let words: FirstRunWords
    let connect: ConnectInfo
}

/// Connect as it stands: the core says what's on screen, and this keeps what's typed (`app::ConnectView`).
struct ConnectInfo: Decodable {
    /// "address" for the address box with `fields` under it, or "by_hand" for the whole form.
    let stage: String
    /// What was found.
    let text: String
    /// About the connection itself.
    let note: String
    let fields: [FieldInfo]
    let rows: [ConnectRowInfo]
    /// What the main button says.
    let action: String
    /// Looking at an address. The fields stay open.
    let probing: Bool
}

struct ConnectRowInfo: Decodable {
    let label: String
    /// What it opens, if anything.
    let url: String?
}

struct FirstRunWords: Decodable {
    let welcomeTitle: String
    let welcomeText: String
    let welcome: [WelcomeRow]
    let connectTitle: String
    let connectText: String
    let byHandText: String
    let address: FieldInfo
    let noServer: NoServerInfo
}

struct WelcomeRow: Decodable {
    let label: String
    /// Offered, not pushed.
    let quiet: Bool
}

struct NoServerInfo: Decodable {
    let title: String
    let text: String
    let offers: [OfferInfo]
    let note: String
    let more: LinkInfo
}

struct OfferInfo: Decodable {
    let name: String
    let price: String
    let text: String
    let link: LinkInfo
}

struct LinkInfo: Decodable, Identifiable {
    let label: String
    let url: String
    var id: String { url }
}

struct SettingsInfo: Decodable {
    /// What the form offers.
    let providers: [ProviderInfo]
    let readeckFields: [FieldInfo]
    /// What the button that leads to the address box says.
    let findServer: String
    let problem: String
    let error: String
    let checking: Bool
    /// Bumped when `values` comes along: the form fills in from them once, and again if the core takes something back.
    let serial: Int
    let values: SettingsValues?
}

/// A row of `PROVIDERS`. The core also sends its capabilities and such, which nothing here reads.
struct ProviderInfo: Decodable, Identifiable {
    let id: String
    let shortLabel: String
    let fields: [FieldInfo]
    let links: [LinkInfo]
}

struct FieldInfo: Decodable, Identifiable {
    let key: String
    let label: String
    let placeholder: String
    let secret: Bool
    /// May be empty.
    let help: String
    var id: String { key }
}

struct SettingsValues: Equatable {
    /// An `id` from `SettingsInfo.providers`. The other kinds' fields are kept, for switching back.
    var server: String
    /// Every field's value by its `key`, on screen or not.
    var fields: [String: String]

    static let empty = SettingsValues(server: "miniflux", fields: [:])

    subscript(field key: String) -> String {
        get { fields[key] ?? "" }
        set { fields[key] = newValue }
    }
}

/// The core sends the fields as a list: this decoder would rewrite the keys of an object.
extension SettingsValues: Decodable {
    private struct Pair: Decodable {
        let key: String
        let value: String
    }

    private enum Key: String, CodingKey {
        case server, fields
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        server = try c.decode(String.self, forKey: .server)
        let pairs = try c.decode([Pair].self, forKey: .fields)
        fields = Dictionary(uniqueKeysWithValues: pairs.map { ($0.key, $0.value) })
    }
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
