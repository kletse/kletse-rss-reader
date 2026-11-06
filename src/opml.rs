//! OPML files: telling one from anything else before it goes to a server, reading and writing them on this machine,
//! and making one from a server's list where it has no export of its own. The servers read the file themselves: Onward
//! only needs to know that it's OPML, and which feeds it names.

use crate::error::Error;
use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

/// Far more than any feed list: 10,000 feeds come to about 2 MB. It stops a wrong file from going over the network.
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;

/// A file that passed `check`: what to send, and the addresses of the feeds in it, each once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedList {
    pub bytes: Vec<u8>,
    pub feeds: Vec<String>,
}

/// Why an export didn't end up in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportFailed {
    /// A file is there already. It's the path, as it'll be shown.
    Exists(String),
    /// Anything else, as a message.
    Other(String),
}

impl From<Error> for ExportFailed {
    fn from(e: Error) -> Self {
        Self::Other(e.message())
    }
}

/// One feed of a list made by `build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub title: String,
    pub url: String,
    pub site: String,
    /// Its folder, or nothing.
    pub category: String,
}

fn line_at(bytes: &[u8], position: u64) -> usize {
    let end = usize::try_from(position).unwrap_or(bytes.len()).min(bytes.len());
    bytes[..end].iter().filter(|&&b| b == b'\n').count() + 1
}

/// What's wrong with a file whose first element isn't `opml`, in words for someone who picked the wrong file.
fn not_opml(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "rss" | "feed" | "rdf:rdf" => {
            "This is a single feed, not a list of feeds. An OPML file comes from your old reader's export.".into()
        }
        "html" => "This is a web page, not an OPML file.".into(),
        _ => format!("This isn't an OPML file: it starts with <{name}>."),
    }
}

fn feed_address(element: &BytesStart<'_>) -> Result<Option<String>, String> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|e| e.to_string())?;
        if attribute.key.0.eq_ignore_ascii_case("xmlUrl") {
            let value = attribute.normalized_value(XmlVersion::Explicit1_0).map_err(|e| e.to_string())?;
            let value = value.trim();
            return Ok((!value.is_empty()).then(|| value.to_string()));
        }
    }
    Ok(None)
}

/// The addresses of the feeds in an OPML document, each once, in order. The error is the message to show.
pub fn feeds_in(bytes: &[u8]) -> Result<Vec<String>, String> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err("This file is empty.".into());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err("This file is in UTF-16, which Onward can't check. Save it as UTF-8 and try again.".into());
    }
    if std::str::from_utf8(bytes).is_err() {
        return Err("This file isn't UTF-8, which Onward can't check. Save it as UTF-8 and try again.".into());
    }
    let mut reader = Reader::from_reader(bytes);
    let mut feeds: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let (mut depth, mut root, mut body) = (0_usize, false, false);
    loop {
        let event = reader.read_event().map_err(|e| {
            let line = line_at(bytes, reader.error_position());
            format!("This file is broken near line {line}, so it isn't OPML a server could read: {e}.")
        })?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.name();
                if !root {
                    if name.0 != "opml" {
                        return Err(not_opml(name.0));
                    }
                    root = true;
                }
                body |= name.0 == "body";
                if name.0.eq_ignore_ascii_case("outline")
                    && let Some(url) = feed_address(&element).map_err(|e| format!("This OPML file is broken: {e}."))?
                    && seen.insert(url.clone())
                {
                    feeds.push(url);
                }
                depth += usize::from(!empty);
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }
    if !root {
        return Err("This isn't an OPML file: there's no XML in it.".into());
    }
    if depth > 0 {
        return Err("This file stops halfway, so it isn't OPML a server could read.".into());
    }
    if !body {
        return Err("This OPML file has no <body>, so a server would find nothing in it.".into());
    }
    Ok(feeds)
}

/// What the user typed as a path: outer spaces and quotes go, as a terminal may add them to a dragged file, and a
/// leading `~` is the home directory. Windows hand over real paths, which this leaves alone.
pub fn expand(typed: &str) -> PathBuf {
    let typed = typed.trim();
    let unquoted = ['\'', '"'].iter().find_map(|&q| typed.strip_prefix(q)?.strip_suffix(q)).unwrap_or(typed);
    let home = dirs::home_dir();
    match (unquoted.strip_prefix('~'), home) {
        (Some(""), Some(home)) => home,
        (Some(rest), Some(home)) if rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(unquoted),
    }
}

/// Reads and checks a file to import. Every way it can fail says what to do about it, because nothing has reached a
/// server yet. Call it off the window's thread: a path may be on a slow disk.
pub fn read_file(path: &Path) -> Result<FeedList, Error> {
    let shown = path.display();
    let refused = |message: String| Error::Refused(message);
    let meta = fs::metadata(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => refused(format!("There's no file at {shown}.")),
        ErrorKind::PermissionDenied => refused(format!("Onward isn't allowed to read {shown}.")),
        _ => Error::Io { context: format!("Could not read {shown}"), source: e },
    })?;
    if meta.is_dir() {
        return Err(refused(format!("{shown} is a folder. Name the OPML file in it.")));
    }
    // Not a pipe or a device, which might never end.
    if !meta.is_file() {
        return Err(refused(format!("{shown} isn't a regular file.")));
    }
    if meta.len() > MAX_BYTES {
        let megabytes = MAX_BYTES / (1024 * 1024);
        return Err(refused(format!("{shown} is larger than {megabytes} MB, which is too big for a list of feeds.")));
    }
    let mut bytes = Vec::new();
    let read = fs::File::open(path).and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes));
    read.map_err(|e| match e.kind() {
        ErrorKind::PermissionDenied => refused(format!("Onward isn't allowed to read {shown}.")),
        _ => Error::Io { context: format!("Could not read {shown}"), source: e },
    })?;
    let feeds = feeds_in(&bytes).map_err(refused)?;
    if feeds.is_empty() {
        return Err(refused("This OPML file lists no feeds.".into()));
    }
    Ok(FeedList { bytes, feeds })
}

/// Writes an export. A file that's there stays unless `replace` says it was asked about, and one that's half written
/// never takes its place: it goes through a temporary file next to it. Like that file it's readable by you alone, as
/// a feed's address can carry a private token. Call it off the window's thread.
pub fn write_file(path: &Path, text: &[u8], replace: bool) -> Result<(), ExportFailed> {
    let shown = path.display().to_string();
    if path.is_dir() {
        return Err(ExportFailed::Other(format!("{shown} is a folder. Name a file in it.")));
    }
    if !replace && path.symlink_metadata().is_ok() {
        return Err(ExportFailed::Exists(shown));
    }
    let folder = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if !folder.is_dir() {
        return Err(ExportFailed::Other(format!("There's no folder {}.", folder.display())));
    }
    let failed = |e: std::io::Error| ExportFailed::Other(format!("Could not write {shown}: {e}"));
    let mut temp = tempfile::NamedTempFile::new_in(folder).map_err(failed)?;
    temp.write_all(text).and_then(|()| temp.flush()).map_err(failed)?;
    if replace {
        // Through a link to where it points, rather than over the link.
        let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        temp.persist(target).map_err(|e| failed(e.error))?;
    } else {
        temp.persist_noclobber(path).map_err(|e| match e.error.kind() {
            ErrorKind::AlreadyExists => ExportFailed::Exists(shown.clone()),
            _ => failed(e.error),
        })?;
    }
    Ok(())
}

fn outline(feed: &Listed, indent: &str) -> String {
    let (title, url, site) = (escape(feed.title.as_str()), escape(feed.url.as_str()), escape(feed.site.as_str()));
    format!("{indent}<outline text=\"{title}\" title=\"{title}\" type=\"rss\" xmlUrl=\"{url}\" htmlUrl=\"{site}\"/>\n")
}

/// An OPML document of these feeds, with each folder once, in the order they come.
pub fn build(feeds: &[Listed]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"2.0\">\n  <head>\n    <title>Onward</title>\n  \
         </head>\n  <body>\n",
    );
    let mut folders: Vec<&str> = Vec::new();
    for feed in feeds.iter().filter(|f| !f.category.is_empty()) {
        if !folders.contains(&feed.category.as_str()) {
            folders.push(&feed.category);
        }
    }
    for folder in folders {
        out.push_str(&format!("    <outline text=\"{}\">\n", escape(folder)));
        for feed in feeds.iter().filter(|f| f.category == folder) {
            out.push_str(&outline(feed, "      "));
        }
        out.push_str("    </outline>\n");
    }
    for feed in feeds.iter().filter(|f| f.category.is_empty()) {
        out.push_str(&outline(feed, "    "));
    }
    out.push_str("  </body>\n</opml>\n");
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// As Miniflux 2.3.5 exported it.
    const MINIFLUX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0" xmlns:miniflux="https://miniflux.app/opml">
    <head>
        <title>Miniflux</title>
        <dateCreated>Wed, 04 Nov 2026 22:44:31 UTC</dateCreated>
    </head>
    <body>
        <outline text="All">
            <outline title="Delta Feed" text="Delta Feed" xmlUrl="http://onward-spike-feeds/delta.xml" htmlUrl="http://onward-spike-feeds/delta/" type="rss"></outline>
        </outline>
        <outline text="News">
            <outline title="Alpha Feed" text="Alpha Feed" xmlUrl="http://onward-spike-feeds/alpha.xml" htmlUrl="http://onward-spike-feeds/alpha/" type="rss"></outline>
        </outline>
    </body>
</opml>
"#;
    /// As FreshRSS 1.30.4 exported an empty account: one empty folder.
    const FRESHRSS_EMPTY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml xmlns:frss="https://freshrss.org/opml" version="2.0">
  <head>
    <title>FreshRSS</title>
    <dateCreated>Wed, 04 Nov 2026 22:45:53 +0000</dateCreated>
  </head>
  <body>
    <outline text="Uncategorized"/>
  </body>
</opml>
"#;

    #[test]
    fn what_the_servers_export_is_read_for_its_feeds() {
        let feeds = feeds_in(MINIFLUX.as_bytes()).unwrap();
        assert_eq!(feeds, ["http://onward-spike-feeds/delta.xml", "http://onward-spike-feeds/alpha.xml"]);
        assert_eq!(feeds_in(FRESHRSS_EMPTY.as_bytes()).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_feed_listed_twice_counts_once_and_entities_are_read() {
        let doc = r#"<opml version="1.0"><body>
            <outline xmlurl="https://a.example/?x=1&amp;y=2"/>
            <outline xmlUrl="https://a.example/?x=1&amp;y=2"/>
        </body></opml>"#;
        assert_eq!(feeds_in(doc.as_bytes()).unwrap(), ["https://a.example/?x=1&y=2"]);
    }

    #[test]
    fn each_wrong_file_says_what_it_is() {
        let says = |doc: &[u8], words: &str| {
            let message = feeds_in(doc).unwrap_err();
            assert!(message.contains(words), "{message}");
        };
        says(b"", "empty");
        says(b"  \n", "empty");
        says(b"[]", "no XML");
        says(b"just some text", "no XML");
        says(b"<?xml version=\"1.0\"?><rss version=\"2.0\"><channel></channel></rss>", "single feed");
        says(b"<feed xmlns=\"http://www.w3.org/2005/Atom\"></feed>", "single feed");
        says(b"<!doctype html><html><body></body></html>", "web page");
        says(b"<notes></notes>", "<notes>");
        says(b"<opml version=\"2.0\"><head></head></opml>", "no <body>");
        says(b"<opml><body><outline xmlUrl=\"a\"></body></opml>", "line 1");
        says(b"<opml>\n<body>\n<outline>", "stops halfway");
        says(&[0xFF, 0xFE, b'<', 0], "UTF-16");
        says(b"<opml><body><outline text=\"caf\xE9\"/></body></opml>", "isn't UTF-8");
    }

    #[test]
    fn a_home_path_is_expanded_and_quotes_go() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand("~"), home);
        assert_eq!(expand("~/feeds.opml"), home.join("feeds.opml"));
        assert_eq!(expand("  '~/a b.opml'\n"), home.join("a b.opml"));
        assert_eq!(expand("\"/tmp/a b.opml\""), PathBuf::from("/tmp/a b.opml"));
        assert_eq!(expand("/tmp/~x"), PathBuf::from("/tmp/~x"));
        assert_eq!(expand("~someone/feeds.opml"), PathBuf::from("~someone/feeds.opml"), "only your own home");
    }

    fn message(error: Error) -> String {
        error.message()
    }

    #[test]
    fn reading_says_what_is_wrong_with_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nothing.opml");
        let said = message(read_file(&missing).unwrap_err());
        assert_eq!(said, format!("There's no file at {}.", missing.display()));
        assert!(message(read_file(dir.path()).unwrap_err()).contains("is a folder"));

        let empty = dir.path().join("empty.opml");
        fs::write(&empty, FRESHRSS_EMPTY).unwrap();
        assert_eq!(message(read_file(&empty).unwrap_err()), "This OPML file lists no feeds.");

        let good = dir.path().join("feeds.opml");
        fs::write(&good, MINIFLUX).unwrap();
        let list = read_file(&good).unwrap();
        assert_eq!(list.bytes, MINIFLUX.as_bytes());
        assert_eq!(list.feeds.len(), 2);
    }

    #[test]
    fn a_file_over_the_cap_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.opml");
        fs::File::create(&big).unwrap().set_len(MAX_BYTES + 1).unwrap();
        assert!(message(read_file(&big).unwrap_err()).contains("larger than 10 MB"));
    }

    #[test]
    fn writing_leaves_what_is_there_unless_told_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feeds.opml");
        write_file(&path, b"new", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");

        assert_eq!(write_file(&path, b"newer", false), Err(ExportFailed::Exists(path.display().to_string())));
        assert_eq!(fs::read(&path).unwrap(), b"new", "untouched");

        write_file(&path, b"newer", true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"newer");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "no temporary file is left behind");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_counts_as_there_and_is_replaced_through() {
        let dir = tempfile::tempdir().unwrap();
        let (real, link) = (dir.path().join("real.opml"), dir.path().join("link.opml"));
        fs::write(&real, "old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(matches!(write_file(&link, b"new", false), Err(ExportFailed::Exists(_))));
        write_file(&link, b"new", true).unwrap();
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"new");
    }

    #[test]
    fn writing_says_what_is_wrong_with_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let in_nothing = dir.path().join("no/such/feeds.opml");
        let Err(ExportFailed::Other(said)) = write_file(&in_nothing, b"x", false) else { panic!() };
        assert!(said.starts_with("There's no folder"), "{said}");
        let Err(ExportFailed::Other(said)) = write_file(dir.path(), b"x", false) else { panic!() };
        assert!(said.contains("is a folder"), "{said}");
    }

    #[test]
    fn a_built_list_reads_back_with_awkward_names_and_its_folders() {
        let feed = |title: &str, url: &str, category: &str| Listed {
            title: title.into(),
            url: url.into(),
            site: "https://example.com/?a=1&b=2".into(),
            category: category.into(),
        };
        let doc = build(&[
            feed("R&D \"notes\" <1>", "https://a.example/feed?x=1&y=2", "News & views"),
            feed("Loose", "https://b.example/feed", ""),
            feed("Second", "https://c.example/feed", "News & views"),
        ]);
        assert_eq!(
            feeds_in(doc.as_bytes()).unwrap(),
            ["https://a.example/feed?x=1&y=2", "https://c.example/feed", "https://b.example/feed"]
        );
        assert_eq!(doc.matches("<outline text=\"News &amp; views\">").count(), 1);
    }
}
