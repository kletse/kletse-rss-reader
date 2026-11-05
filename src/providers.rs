//! What the windows draw first run and the server form from: the kinds of server, their fields and help, the screens
//! around them. It's data, here once, so that a new row or field means one edit rather than one in the core, the Qt
//! bridge, the C interface, QML, Swift and the terminal. What it all does is in app.rs.
//!
//! A hosted service gets a row only once it has passed against a real account.

use crate::config::Server;
use crate::feeds::Capabilities;
use serde::Serialize;

/// A value in the settings form, as the windows name it. `SettingsValues` has one slot for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKey {
    /// The box first run starts from, whatever the server turns out to be.
    Address,
    MinifluxUrl,
    MinifluxApiKey,
    /// Asked for once, to make a key with. It never reaches the config.
    MinifluxUsername,
    MinifluxPassword,
    GreaderUrl,
    GreaderUsername,
    GreaderPassword,
    ReadeckUrl,
    ReadeckToken,
}

impl FieldKey {
    pub const ALL: [FieldKey; 10] = [
        FieldKey::Address,
        FieldKey::MinifluxUrl,
        FieldKey::MinifluxApiKey,
        FieldKey::MinifluxUsername,
        FieldKey::MinifluxPassword,
        FieldKey::GreaderUrl,
        FieldKey::GreaderUsername,
        FieldKey::GreaderPassword,
        FieldKey::ReadeckUrl,
        FieldKey::ReadeckToken,
    ];

    /// As it goes to the windows and comes back: the same as its JSON.
    pub fn name(self) -> &'static str {
        match self {
            FieldKey::Address => "address",
            FieldKey::MinifluxUrl => "miniflux_url",
            FieldKey::MinifluxApiKey => "miniflux_api_key",
            FieldKey::MinifluxUsername => "miniflux_username",
            FieldKey::MinifluxPassword => "miniflux_password",
            FieldKey::GreaderUrl => "greader_url",
            FieldKey::GreaderUsername => "greader_username",
            FieldKey::GreaderPassword => "greader_password",
            FieldKey::ReadeckUrl => "readeck_url",
            FieldKey::ReadeckToken => "readeck_token",
        }
    }
}

/// One text field of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Field {
    pub key: FieldKey,
    pub label: &'static str,
    /// Gone once something is typed, so nothing that matters goes only here: that's for `help`.
    pub placeholder: &'static str,
    pub secret: bool,
    /// Stays under the field. May be empty.
    pub help: &'static str,
}

/// A page on the web. The windows give it a button, and the terminal prints `url` in full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Link {
    pub label: &'static str,
    pub url: &'static str,
}

/// A kind of server the form offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Provider {
    /// The name of its protocol, while there's one row for each: see `Server::name`.
    pub id: &'static str,
    /// As the server choice offers it.
    pub label: &'static str,
    /// Where the help nearby names the servers: the Mac's picker, section titles.
    pub short_label: &'static str,
    /// The address of a hosted service, which then needs no address field: see `HOSTED`.
    pub fixed_url: Option<&'static str>,
    pub protocol: Server,
    /// All of it, for filling in by hand.
    pub fields: &'static [Field],
    /// What first run asks for, once it knows the address and that it's this kind of server.
    pub asks: &'static [Field],
    pub links: &'static [Link],
    /// What it can do: what it can't, the keys only say so and the windows leave out their hints.
    pub capabilities: Capabilities,
}

const fn field(key: FieldKey, label: &'static str, placeholder: &'static str, help: &'static str) -> Field {
    Field { key, label, placeholder, secret: false, help }
}

const fn secret(key: FieldKey, label: &'static str, placeholder: &'static str, help: &'static str) -> Field {
    Field { key, label, placeholder, secret: true, help }
}

/// In the order of `Server::ALL`, which the check below holds to.
pub const PROVIDERS: [Provider; Server::ALL.len()] = [
    Provider {
        id: "miniflux",
        label: "Miniflux",
        short_label: "Miniflux",
        fixed_url: None,
        protocol: Server::Miniflux,
        fields: &[
            field(FieldKey::MinifluxUrl, "Miniflux URL", "https://miniflux.example.com", ""),
            secret(FieldKey::MinifluxApiKey, "API key", "", "In Miniflux: Settings → API Keys → Create a new API key."),
        ],
        asks: &[
            field(FieldKey::MinifluxUsername, "Username", "", ""),
            secret(
                FieldKey::MinifluxPassword,
                "Password",
                "",
                "Used once, to get Onward a key of its own on your server. It isn't kept.",
            ),
        ],
        links: &[Link { label: "About API keys", url: "https://miniflux.app/docs/api.html#authentication" }],
        capabilities: Capabilities::EVERYTHING,
    },
    Provider {
        id: "greader",
        label: "Google Reader API (FreshRSS, …)",
        short_label: "Google Reader API",
        fixed_url: None,
        protocol: Server::GReader,
        fields: &[
            field(
                FieldKey::GreaderUrl,
                "Server URL",
                "https://freshrss.example.com/api/greader.php",
                "FreshRSS: https://your-freshrss/api/greader.php. Miniflux: the server URL, with the Google Reader \
                 integration enabled.",
            ),
            field(FieldKey::GreaderUsername, "Username", "", ""),
            secret(
                FieldKey::GreaderPassword,
                "Password",
                "",
                "FreshRSS: the API password from your profile, not your login password. Miniflux: the one under \
                 Settings → Integrations.",
            ),
        ],
        asks: &[
            field(FieldKey::GreaderUsername, "Username", "", ""),
            secret(
                FieldKey::GreaderPassword,
                "API password",
                "",
                "This isn't your login password. In FreshRSS, sign in, then go to Settings → Profile → External access \
                 via API. If you never set one, or aren't sure of it, set one there: a new one can take a couple of \
                 seconds to work.",
            ),
        ],
        links: &[Link {
            label: "Turn on FreshRSS's API",
            url: "https://freshrss.github.io/FreshRSS/en/users/06_Mobile_access.html",
        }],
        // The protocol has no call for the first two. Importing is an extension of FreshRSS's, and exporting works
        // from any server's list: see `Config::capabilities`.
        capabilities: Capabilities {
            fetch_full: false,
            refresh: false,
            search: None,
            import_feeds: false,
            export_feeds: true,
            samples: false,
        },
    },
];

const _: () = {
    let mut at = 0;
    while at < PROVIDERS.len() {
        assert!(PROVIDERS[at].protocol.place() == at, "PROVIDERS goes in the order of Server::ALL");
        at += 1;
    }
};

pub fn provider(server: Server) -> &'static Provider {
    &PROVIDERS[server.place()]
}

/// Hosted services, offered under the address box: one row here, with its `fixed_url`, is all a new one takes. A row
/// goes in only once it has passed against a real account, and none has.
pub const HOSTED: &[Provider] = &[];

/// The box first run opens with. What comes after it depends on what's there.
pub const ADDRESS: Field = field(
    FieldKey::Address,
    "Server address",
    "rss.example.com",
    "The one you open in your browser. A short name, a port, a path or a page copied from the address bar will do.",
);

/// Readeck isn't a feed server, and it's not on the way to a first article: it's in Settings only.
pub const READECK: [Field; 2] = [
    field(FieldKey::ReadeckUrl, "Readeck URL", "https://readeck.example.com", ""),
    secret(FieldKey::ReadeckToken, "API token", "", "In Readeck: Profile → API Tokens (needs “Bookmarks: write”)."),
];

/// The three things the first screen offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Samples,
    Connect,
    NoServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WelcomeRow {
    #[serde(skip)]
    pub choice: Choice,
    pub label: &'static str,
    /// Offered, not pushed: drawn with less weight than the others.
    pub quiet: bool,
}

pub const WELCOME_TITLE: &str = "Welcome to Onward";
pub const WELCOME_TEXT: &str = "One unread article at a time, oldest first. Onward reads your feeds from a server. \
                                See how it feels first, if you like: nothing here needs an account.";
pub const WELCOME: [WelcomeRow; 3] = [
    WelcomeRow { choice: Choice::Samples, label: "Try it with sample articles", quiet: false },
    WelcomeRow { choice: Choice::Connect, label: "Connect", quiet: false },
    WelcomeRow { choice: Choice::NoServer, label: "I don't have a server", quiet: true },
];

pub const CONNECT_TITLE: &str = "Connect your server";
pub const CONNECT_TEXT: &str = "Onward looks at the address and works out what your server needs.";
pub const BY_HAND_TEXT: &str = "Onward checks these with your server before it saves anything.";

// What Connect says about what it found. A Miniflux user sees "API key" only on the steps for accounts that need one.
pub const MINIFLUX_FOUND: &str = "This is Miniflux. Sign in once and Onward gets a key of its own from your server.";
pub const MINIFLUX_PASTE: &str =
    "Make an API key on Miniflux's page: it asks you to sign in first if you need to. Then paste the key here.";
pub const MINIFLUX_NO_PASSWORDS: &str = "This Miniflux doesn't sign in with passwords, so Onward needs an API key. \
                                         Make one on its page, then paste it here.";
pub const MINIFLUX_NO_MINTING: &str = "This Miniflux can't make a key for Onward: that came with version 2.2.9. Make \
                                       one on its page, then paste it here.";
pub const FRESHRSS_FOUND: &str = "This is FreshRSS. It needs your username and its API password.";
pub const PLAIN_HTTP: &str = "This uses plain http://, unencrypted. That's normal at home or on a tailnet, and Onward \
                              does it only on addresses like those.";
pub const MINT_REFUSED: &str = "Miniflux didn't accept that. Either the username or password is wrong, or this \
                                account doesn't sign in with a password (Google, OpenID Connect, a passkey), or a \
                                proxy in front of Miniflux removes the login. For the last two, use the button below.";
pub const MINT_REDIRECTED: &str = "Miniflux redirected the sign-in, so Onward stopped rather than send your password \
                                   on. Use an address that reaches the server directly.";
pub const BY_HAND_ROW: &str = "Enter the details yourself";
pub const TO_ADDRESS_ROW: &str = "Find my server by its address";
pub const NO_PASSWORD_ROW: &str = "I don't sign in with a password";
pub const USE_PASSWORD_ROW: &str = "Use my username and password";
pub const KEY_PAGE_ROW: &str = "Open Miniflux's API key page";
pub const OPEN_FRESHRSS_ROW: &str = "Open FreshRSS";

/// An unset API password and a wrong one look the same from here, so this covers both.
pub fn freshrss_refused(web: &str) -> String {
    format!(
        "FreshRSS turned down that username and API password. If you've never set one, or aren't sure of it: sign in \
         at {web}, then go to Settings → Profile → External access via API and set one. If you've only just set it, wait a \
         couple of seconds and try again."
    )
}

pub const DOCS_MOBILE: Link =
    Link { label: "FreshRSS's API", url: "https://freshrss.github.io/FreshRSS/en/users/06_Mobile_access.html" };

/// A way to get a server, on the no-server screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Offer {
    pub name: &'static str,
    pub price: &'static str,
    pub text: &'static str,
    pub link: Link,
}

/// The no-server screen. What changes between releases belongs on `more`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NoServer {
    pub title: &'static str,
    pub text: &'static str,
    pub offers: [Offer; 2],
    pub note: &'static str,
    pub more: Link,
}

pub const NO_SERVER: NoServer = NoServer {
    title: "No server yet?",
    text: "Onward has no feeds of its own: a server fetches them and remembers what you've read. Here are two ways to \
           get one. Neither is free, and Onward has no deal with any of them.",
    offers: [
        Offer {
            name: "Hosted Miniflux",
            price: "$15 a year, 15-day free trial",
            text: "The least to set up. But unread articles are removed 60 days after being added, and read ones after \
                   30. That conflicts with working through a backlog: stop for two months and the oldest articles, \
                   the ones Onward would show you next, are gone.",
            link: Link { label: "Hosted Miniflux", url: "https://miniflux.app/hosting.html" },
        },
        Offer {
            name: "Miniflux on PikaPods",
            price: "From about $1.80 a month",
            text: "The same Miniflux in a small hosted server of your own. How long articles stay is up to you.",
            link: Link { label: "PikaPods", url: "https://www.pikapods.com/apps" },
        },
    ],
    note: "Either way, Connect comes next. If you're leaving another reader, export an OPML file from it, and Onward \
           imports that once you're connected. Otherwise you add feeds on the server's web page.",
    more: Link { label: "More on choosing", url: "https://onward.kletse.dev/no-server/" },
};

// Importing and exporting a feed list, the same on all three.
pub const IMPORT_TITLE: &str = "Import feeds";
pub const IMPORT_TEXT: &str = "An OPML file is the list of feeds any reader can export. Onward checks the file and gives \
                               it to your server, which subscribes to what's in it. Feeds you have already stay as \
                               they are.";
pub const IMPORT_ACTION: &str = "Choose a file…";
pub const IMPORT_WORKING: &str = "Your server is going through the list. A long one can take minutes, as a server may \
                                  fetch every feed before it answers. You can close this: the server carries on, and \
                                  Onward says when it's done.";
pub const EXPORT_TITLE: &str = "Export feeds";
pub const EXPORT_TEXT: &str = "Saves the feeds on your server to an OPML file, which any reader can import.";
pub const EXPORT_ACTION: &str = "Choose where to save…";
pub const EXPORT_WORKING: &str = "Asking your server for its list…";
/// What an export is called until you call it something else.
pub const EXPORT_NAME: &str = "onward-feeds.opml";
/// The terminal's field, where a window has its file dialog.
pub const PATH_LABEL: &str = "Path to the file";
pub const PATH_HELP: &str = "~ is your home folder. Over SSH, this is a file on that machine.";

/// All the words of first run, for the windows that take them in one piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FirstRun {
    pub welcome_title: &'static str,
    pub welcome_text: &'static str,
    pub welcome: [WelcomeRow; 3],
    pub connect_title: &'static str,
    pub connect_text: &'static str,
    pub by_hand_text: &'static str,
    pub address: Field,
    /// Settings' way to the address box.
    pub find_server: &'static str,
    pub no_server: NoServer,
}

pub const FIRST_RUN: FirstRun = FirstRun {
    welcome_title: WELCOME_TITLE,
    welcome_text: WELCOME_TEXT,
    welcome: WELCOME,
    connect_title: CONNECT_TITLE,
    connect_text: CONNECT_TEXT,
    by_hand_text: BY_HAND_TEXT,
    address: ADDRESS,
    find_server: TO_ADDRESS_ROW,
    no_server: NO_SERVER,
};

impl NoServer {
    /// What the arrows move through on this screen, top to bottom.
    pub fn links(&self) -> [Link; 3] {
        [self.offers[0].link, self.offers[1].link, self.more]
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn every_field_is_on_some_form_and_named_as_its_json() {
        let shown = std::iter::once(&ADDRESS)
            .chain(PROVIDERS.iter().flat_map(|p| p.fields.iter().chain(p.asks)))
            .chain(&READECK);
        let keys: Vec<FieldKey> = shown.map(|f| f.key).collect();
        for key in FieldKey::ALL {
            assert!(keys.contains(&key), "{key:?} is on no form");
            assert_eq!(serde_json::to_value(key).unwrap(), key.name());
        }
    }

    /// Spike S3 hasn't run: a hosted row needs a real account first.
    #[test]
    fn no_hosted_service_has_passed_yet() {
        assert!(HOSTED.is_empty());
    }

    #[test]
    fn what_first_run_asks_a_miniflux_user_never_says_api_key() {
        let words = provider(Server::Miniflux).asks.iter().flat_map(|f| [f.label, f.help, f.placeholder]);
        for text in words.chain([ADDRESS.label, ADDRESS.help, CONNECT_TITLE, CONNECT_TEXT]) {
            assert!(!text.to_lowercase().contains("api key"), "{text}");
        }
    }

    #[test]
    fn a_row_says_which_kind_it_is() {
        for server in Server::ALL {
            assert_eq!(provider(server).protocol, server);
            assert_eq!(provider(server).id, server.name());
        }
    }

    #[test]
    fn the_no_server_screen_warns_before_anyone_pays() {
        let [hosted, pikapods] = NO_SERVER.offers;
        assert!(hosted.price.contains("$15") && hosted.price.contains("15-day"));
        assert!(hosted.text.contains("60 days") && hosted.text.contains("after 30") && hosted.text.contains("backlog"));
        assert!(pikapods.price.contains("$1.80"));
        assert!(NO_SERVER.text.contains("Neither is free") && NO_SERVER.text.contains("no deal"));
    }
}
