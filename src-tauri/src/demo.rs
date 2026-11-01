//! Demo mode (`KLETSE_RSS_DEMO=1`): built-in sample feeds and articles, no server needed.
//! Used for README screenshots and for working on the UI without touching real unread items.
//! All feeds, authors and articles here are made up.

use crate::miniflux::{Category, Entry, EntryFeed, EntryPage, EntryStatus, Feed, FeedCount, Selection};

struct DemoFeed {
    id: i64,
    title: &'static str,
    category: (i64, &'static str),
}

struct DemoEntry {
    feed: i64,
    title: &'static str,
    author: &'static str,
    published_at: &'static str,
    minutes: i64,
    content: &'static str,
}

const FEEDS: &[DemoFeed] = &[
    DemoFeed { id: 1, title: "Terminal Notes", category: (1, "Dev") },
    DemoFeed { id: 2, title: "The Crate Digest", category: (1, "Dev") },
    DemoFeed { id: 3, title: "Small Web Weekly", category: (2, "Blogs") },
    DemoFeed { id: 4, title: "Quiet Pixels", category: (2, "Blogs") },
    DemoFeed { id: 5, title: "Escapement Journal", category: (3, "Watches") },
];

/// Oldest first; the entry id is its 1-based position here.
const ENTRIES: &[DemoEntry] = &[
    DemoEntry {
        feed: 1,
        title: "Why I read my feeds one article at a time",
        author: "Mira Janssens",
        published_at: "2026-10-24T08:12:00+02:00",
        minutes: 4,
        content: ARTICLE_ONE_AT_A_TIME,
    },
    DemoEntry {
        feed: 5,
        title: "Inside a hand-wound movement, gear by gear",
        author: "Tomas Verhoeven",
        published_at: "2026-10-25T19:40:00+02:00",
        minutes: 6,
        content: ARTICLE_MOVEMENT,
    },
    DemoEntry {
        feed: 2,
        title: "Small Rust services that never wake you up at night",
        author: "Ada Lindqvist",
        published_at: "2026-10-26T10:05:00+01:00",
        minutes: 5,
        content: ARTICLE_RUST,
    },
    DemoEntry {
        feed: 3,
        title: "The case for a slower internet",
        author: "Noor El Amrani",
        published_at: "2026-10-27T14:30:00+01:00",
        minutes: 3,
        content: ARTICLE_SLOW_WEB,
    },
    DemoEntry {
        feed: 4,
        title: "Designing with only five colours",
        author: "Jonas Peeters",
        published_at: "2026-10-28T09:00:00+01:00",
        minutes: 4,
        content: ARTICLE_COLOURS,
    },
    DemoEntry {
        feed: 1,
        title: "A dotfiles setup you can explain in one breath",
        author: "Mira Janssens",
        published_at: "2026-10-29T21:15:00+01:00",
        minutes: 2,
        content: ARTICLE_DOTFILES,
    },
    DemoEntry {
        feed: 2,
        title: "Error messages are user interface",
        author: "Ada Lindqvist",
        published_at: "2026-10-30T07:45:00+01:00",
        minutes: 3,
        content: ARTICLE_ERRORS,
    },
    DemoEntry {
        feed: 5,
        title: "Why the tick-tock of a watch is never quite even",
        author: "Tomas Verhoeven",
        published_at: "2026-10-30T18:20:00+01:00",
        minutes: 4,
        content: ARTICLE_BEAT,
    },
];

fn feed(id: i64) -> &'static DemoFeed {
    FEEDS.iter().find(|f| f.id == id).expect("every demo entry belongs to a listed demo feed")
}

fn as_feed(f: &DemoFeed) -> Feed {
    Feed { id: f.id, title: f.title.into(), category: Category { id: f.category.0, title: f.category.1.into() } }
}

/// Every demo article counts as both unread and read, so the history has something to show too.
pub fn feed_counts() -> Vec<FeedCount> {
    FEEDS
        .iter()
        .map(|f| {
            let n = ENTRIES.iter().filter(|e| e.feed == f.id).count() as i64;
            FeedCount { feed: as_feed(f), unread: n, read: n }
        })
        .collect()
}

/// A page, the way Miniflux pages: unread oldest first, published after `past`, or read newest first, published
/// before it (0: from the start). History shows the same articles again, marked read.
pub fn entries(sel: Selection, status: EntryStatus, past: i64, limit: usize) -> EntryPage {
    let mut all = selection(sel);
    if status == EntryStatus::Read {
        all.reverse();
    }
    all.retain(|e| match status {
        _ if past <= 0 => true,
        EntryStatus::Unread => unix_seconds(&e.published_at) > past,
        EntryStatus::Read => unix_seconds(&e.published_at) < past,
    });
    let total = all.len() as i64;
    let entries = all.into_iter().take(limit).map(|e| Entry { status, ..e }).collect();
    EntryPage { total, entries }
}

pub fn newest_unread_id(sel: Selection) -> i64 {
    selection(sel).iter().map(|e| e.id).max().unwrap_or(0)
}

pub fn unread_count(sel: Selection, up_to: i64) -> i64 {
    selection(sel).iter().filter(|e| e.id <= up_to).count() as i64
}

/// Pretends to: nothing is ever marked read in demo mode.
pub fn mark_read_up_to(sel: Selection, up_to: i64) -> usize {
    selection(sel).iter().filter(|e| e.id <= up_to).count()
}

/// Unix seconds of a demo date; they're all written as `YYYY-MM-DDTHH:MM:SS+HH:MM`.
fn unix_seconds(date: &str) -> i64 {
    let n = |at: std::ops::Range<usize>| -> i64 { date[at].parse().expect("demo dates are well-formed") };
    let (month, day) = (n(5..7), n(8..10));
    // Days since 1970-01-01, counting years from March so the leap day comes last (Howard Hinnant's days_from_civil).
    let year = if month <= 2 { n(0..4) - 1 } else { n(0..4) };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let sign = if &date[19..20] == "-" { -1 } else { 1 };
    let offset = sign * (n(20..22) * 3600 + n(23..25) * 60);
    days * 86_400 + n(11..13) * 3600 + n(14..16) * 60 + n(17..19) - offset
}

/// The selection's articles, unread, oldest first.
fn selection(sel: Selection) -> Vec<Entry> {
    ENTRIES
        .iter()
        .enumerate()
        .filter(|(_, e)| match sel {
            Selection::All => true,
            Selection::Category { id } => feed(e.feed).category.0 == id,
            Selection::Feed { id } => e.feed == id,
        })
        .map(|(i, e)| Entry {
            id: i as i64 + 1,
            title: e.title.into(),
            url: format!("https://example.com/demo/{}", i + 1),
            author: e.author.into(),
            content: e.content.into(),
            published_at: e.published_at.into(),
            status: EntryStatus::Unread,
            reading_time: e.minutes,
            feed: EntryFeed { id: e.feed, title: feed(e.feed).title.into() },
        })
        .collect()
}

/// "Fetched" content: the same article again. Unknown ids give an empty page.
pub fn content(id: i64) -> String {
    usize::try_from(id)
        .ok()
        .and_then(|id| id.checked_sub(1))
        .and_then(|index| ENTRIES.get(index))
        .map(|e| e.content.to_string())
        .unwrap_or_default()
}

const ARTICLE_ONE_AT_A_TIME: &str = r##"
<p>For years my feed reader looked like an inbox: three panes, a wall of bold headlines, and a number in the corner that only ever went up. I skimmed titles, opened twenty tabs, and read none of them properly.</p>
<p>So I tried something different. <strong>One article at a time, oldest first.</strong> No list to scan, no counter to fight. Just the next thing, and a single key to move on.</p>
<figure>
  <img src="data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 800 300'%3E%3Cdefs%3E%3ClinearGradient id='s' x1='0' y1='0' x2='0' y2='1'%3E%3Cstop offset='0' stop-color='%23233128'/%3E%3Cstop offset='1' stop-color='%23101310'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='800' height='300' fill='url(%23s)'/%3E%3Ccircle cx='600' cy='90' r='38' fill='%23dbdab8' opacity='.85'/%3E%3Cpath d='M0 230 L140 150 L250 210 L390 110 L520 200 L640 140 L800 220 L800 300 L0 300Z' fill='%233f8f6d' opacity='.55'/%3E%3Cpath d='M0 260 L180 200 L330 250 L470 190 L620 250 L800 210 L800 300 L0 300Z' fill='%231f2b23'/%3E%3C/svg%3E" alt="Mountains at dusk">
  <figcaption>Fewer things on screen, more attention for the one that is.</figcaption>
</figure>
<h2>What changed</h2>
<p>The first surprise was how quickly the backlog shrank. When skipping an article costs exactly as much as reading it — one key press — you stop hoarding. Things that matter get read; the rest gets a polite <em>next</em>.</p>
<blockquote>The best interface for reading is the one that disappears while you read.</blockquote>
<p>The second surprise was that I started enjoying long pieces again. With nothing else competing for space, a 3,000-word essay feels like a book chapter instead of a chore.</p>
<h2>The rules I settled on</h2>
<ul>
  <li><strong>Unread only.</strong> If I've read it, I don't need to see it again.</li>
  <li><strong>Oldest first.</strong> The queue is a conversation, not a news ticker.</li>
  <li><strong>Skip is not read.</strong> Some things deserve a second look tomorrow.</li>
</ul>
<p>None of this is new, of course. It's just the way we read newspapers for a century, before the infinite scroll arrived.</p>
"##;

const ARTICLE_MOVEMENT: &str = r##"
<p>Open the caseback of a simple hand-wound watch and you'll find fewer parts than you might expect: a mainspring, a train of four wheels, an escapement and a balance. Together they turn a few turns of a crown into forty hours of steady time.</p>
<h2>The mainspring</h2>
<p>Everything starts with a coiled strip of alloy in the barrel. Winding tightens it; as it relaxes it turns the barrel, which drives the <strong>going train</strong>: centre wheel, third wheel, fourth wheel, escape wheel.</p>
<h2>The escapement</h2>
<p>Left alone, the spring would unwind in seconds. The escapement lets that energy out in tiny, equal portions. The pallet fork locks and unlocks the escape wheel, and gives the balance a small push each time.</p>
<blockquote>A watch is a controlled leak of energy, and the escapement is the valve.</blockquote>
<p>The balance swings back and forth, typically 28,800 times an hour, and that rhythm is what the hands actually measure.</p>
"##;

const ARTICLE_RUST: &str = r##"
<p>Most of my home-lab services are tiny: a webhook here, a scraper there. Over time I've settled on a handful of habits that keep them boring in the best way.</p>
<h2>Fail loudly at startup</h2>
<p>Read all configuration once, validate it, and refuse to start if anything is missing. A service that crashes immediately is far easier to fix than one that fails at 3 a.m.</p>
<pre><code class="language-rust">fn load() -&gt; Result&lt;Config, String&gt; {
    let text = std::fs::read_to_string(path())
        .map_err(|e| format!("Could not read config: {e}"))?;
    let config: Config = toml::from_str(&amp;text)
        .map_err(|e| format!("Invalid config: {e}"))?;
    if config.api_key.trim().is_empty() {
        return Err("Add your API key".into());
    }
    Ok(config)
}</code></pre>
<h2>Timeouts everywhere</h2>
<p>Every outgoing request gets a timeout. The default of “wait forever” is how one slow upstream turns into a pile of stuck tasks.</p>
<h2>Log decisions, not noise</h2>
<p>A line per request is noise. A line saying <code>skipped 14 items: already seen</code> is a decision you'll want to find later.</p>
"##;

const ARTICLE_SLOW_WEB: &str = r##"
<p>Not everything needs to be live. Some of my favourite corners of the web update once a week, by hand, from someone who clearly enjoys it.</p>
<p>RSS is the quiet technology that makes those corners easy to follow. No algorithm decides what you see, and nothing is optimised for your attention. It's just a list of things people wrote, in the order they wrote them.</p>
<blockquote>Subscribe to people, not platforms.</blockquote>
"##;

const ARTICLE_COLOURS: &str = r##"
<p>Constraints make design easier, not harder. For the last project I allowed myself one background, one foreground, one accent and two greys. That's it.</p>
<p>The result felt calmer than anything I'd made with a full palette, and it adapted to dark mode almost for free.</p>
"##;

const ARTICLE_DOTFILES: &str = r##"
<p>My whole setup fits in a sentence: a tiling window manager, one terminal, one editor, and a theme that every app follows. If I can't explain a config file in one line, it goes.</p>
"##;

const ARTICLE_ERRORS: &str = r##"
<p>“Something went wrong” is not an error message. A good one says what happened, why, and what to do next, in that order.</p>
<p>Treat every error string as a tiny piece of UI. Someone will read it at the worst possible moment.</p>
"##;

const ARTICLE_BEAT: &str = r##"
<p>Hold a mechanical watch to your ear and the tick-tock should sound perfectly even. When it doesn't, watchmakers call it being “out of beat”, and it's one of the first things they check on a timing machine.</p>
"##;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_belongs_to_a_feed_and_ids_are_unique() {
        for e in ENTRIES {
            assert!(FEEDS.iter().any(|f| f.id == e.feed), "entry {:?} has no feed", e.title);
        }
        let mut ids: Vec<_> = FEEDS.iter().map(|f| f.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), FEEDS.len());
    }

    #[test]
    fn entries_are_oldest_first() {
        let dates: Vec<_> = ENTRIES.iter().map(|e| e.published_at).collect();
        let mut sorted = dates.clone();
        sorted.sort();
        assert_eq!(dates, sorted);
    }

    #[test]
    fn feed_counts_match_entries() {
        let counts = feed_counts();
        assert_eq!(counts.len(), FEEDS.len());
        let total: i64 = counts.iter().map(|c| c.unread).sum();
        assert_eq!(total, ENTRIES.len() as i64);
        assert!(counts.iter().all(|c| c.unread > 0), "every demo feed has something to read");
    }

    fn ids(page: &EntryPage) -> Vec<i64> {
        page.entries.iter().map(|e| e.id).collect()
    }

    #[test]
    fn selections_filter_consistently() {
        let all = entries(Selection::All, EntryStatus::Unread, 0, 100);
        assert_eq!(all.total, ENTRIES.len() as i64);
        assert!(all.entries.iter().all(|e| e.status == EntryStatus::Unread));
        assert_eq!(ids(&all), (1..=ENTRIES.len() as i64).collect::<Vec<_>>());
        assert_eq!(newest_unread_id(Selection::All), ENTRIES.len() as i64);

        let dev = entries(Selection::Category { id: 1 }, EntryStatus::Unread, 0, 100).entries;
        assert!(dev.iter().all(|e| [1, 2].contains(&e.feed.id)));
        assert_eq!(dev.len(), all.entries.iter().filter(|e| [1, 2].contains(&e.feed.id)).count());

        let watches = entries(Selection::Feed { id: 5 }, EntryStatus::Unread, 0, 100).entries;
        assert_eq!(watches.len(), 2);
        assert!(watches.iter().all(|e| e.feed.title == "Escapement Journal"));
        assert_eq!(unread_count(Selection::Feed { id: 5 }, watches[0].id), 1);
        assert_eq!(mark_read_up_to(Selection::Feed { id: 5 }, watches[1].id), 2);

        assert!(entries(Selection::Feed { id: 99 }, EntryStatus::Unread, 0, 100).entries.is_empty());
        assert_eq!(newest_unread_id(Selection::Feed { id: 99 }), 0);
    }

    #[test]
    fn unread_pages_go_on_after_a_time() {
        let first = entries(Selection::All, EntryStatus::Unread, 0, 3);
        assert_eq!((ids(&first), first.total), (vec![1, 2, 3], ENTRIES.len() as i64));
        let past = unix_seconds(&first.entries[2].published_at);
        let next = entries(Selection::All, EntryStatus::Unread, past, 3);
        assert_eq!((ids(&next), next.total), (vec![4, 5, 6], ENTRIES.len() as i64 - 3));
    }

    #[test]
    fn history_is_read_newest_first_and_paged() {
        let n = ENTRIES.len() as i64;
        let all = entries(Selection::All, EntryStatus::Read, 0, 100);
        assert_eq!(all.total, n);
        assert!(all.entries.iter().all(|e| e.status == EntryStatus::Read));
        assert_eq!(ids(&all), (1..=n).rev().collect::<Vec<_>>());

        let page = entries(Selection::All, EntryStatus::Read, unix_seconds(&all.entries[1].published_at), 3);
        assert_eq!(ids(&page), ids(&all)[2..5]);
        assert_eq!(page.total, n - 2);
        assert!(entries(Selection::All, EntryStatus::Read, 1, 3).entries.is_empty());
    }

    #[test]
    fn unix_seconds_reads_the_demo_dates() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00+00:00"), 0);
        assert_eq!(unix_seconds("2026-10-24T08:12:00+02:00"), 1_792_822_320);
        assert_eq!(unix_seconds("2024-02-29T23:00:00-01:00"), 1_709_251_200);
        for e in ENTRIES {
            assert!(unix_seconds(e.published_at) > 0, "{}", e.published_at);
        }
    }

    #[test]
    fn content_by_id_is_safe_for_any_id() {
        assert_eq!(content(1), ARTICLE_ONE_AT_A_TIME);
        assert_eq!(content(ENTRIES.len() as i64), ARTICLE_BEAT);
        for bad in [0, -1, i64::MIN, i64::MAX, ENTRIES.len() as i64 + 1] {
            assert_eq!(content(bad), "", "id {bad}");
        }
    }
}
