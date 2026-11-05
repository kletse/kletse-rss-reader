//! Connecting Readeck through a browser: press one button, approve Onward there, and it's done, with no token to make
//! and paste. The calls are in readeck.rs; this is how far an attempt has got and what's on screen meanwhile. Nothing here
//! waits. `App` turns each answer into the next effect, and the windows draw `GrantView`.
//!
//! An attempt has a number, as a load does. Closing the form, changing the address or starting again takes a new one,
//! and every answer is compared with it: the wait and the poll are separate effects so that a stopped attempt makes no
//! further request, and a poll already on its way changes nothing when it lands. A token only ever comes back here
//! checked, and an attempt that's over drops it.

use reqwest::Url;

use crate::app::ConnectRow;
use crate::readeck::{Asked, Code, Poll, Ticket, Token};
use crate::secrets::Place;
use serde::Serialize;

/// Added to the wait by each `slow_down`.
const SLOW_DOWN_MS: u64 = 5000;

const USE_A_TOKEN: &str = "Use a token instead: in Readeck, open Profile → API Tokens, make one with “Bookmarks: write”, \
                           and paste it into API token above.";
const CONNECT: &str = "Connect in the browser";
const OPEN: &str = "Open in browser";
const AGAIN: &str = "Start again";

/// What a row of the view does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Does {
    Start,
    Open(String),
}

#[derive(Debug, Default)]
enum Stage {
    #[default]
    Idle,
    /// Everything before there's a code.
    Asking,
    Waiting(Waiting),
    /// Approved, and the token checked: being stored. The form's Save waits for this.
    Keeping,
    /// Saved. The form's token field knows nothing of it, so the token stays here for the form's Save (`token_for`).
    Kept(Token),
    /// This server is one to paste a token for.
    TokenOnly {
        why: &'static str,
    },
    Failed {
        why: String,
        /// Worth another go.
        again: bool,
    },
}

#[derive(Debug)]
struct Waiting {
    ticket: Ticket,
    user_code: String,
    address: String,
    /// What the row opens, only if it's safe to: see `openable`.
    open: Option<String>,
    wait_ms: u64,
    /// Time left on the code, going by the waits and how long the polls took.
    left_ms: u64,
    /// Why the last poll got no answer.
    trouble: String,
}

/// The next thing for `App` to send off.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    Wait(u64),
    Keep(Token),
}

/// An attempt, and where it's got to.
#[derive(Debug, Default)]
pub struct Grant {
    attempt: u64,
    /// The address asked, cleaned up. What came back counts for this one and no other.
    url: String,
    stage: Stage,
    /// Where a save put the token, for what's said about it: not while the form's open, as it'd take the form's place.
    place: Option<Place>,
}

/// The server chose this address, so it's opened only if it's plain http(s) on the host that was typed in. A name
/// before the `@` is left out too: it's there to make a link read as something else.
fn openable(entered: &str, address: &str) -> bool {
    let (Ok(entered), Ok(there)) = (Url::parse(entered), Url::parse(address)) else { return false };
    matches!(there.scheme(), "http" | "https")
        && there.username().is_empty()
        && there.password().is_none()
        && there.host_str().is_some()
        && there.host_str() == entered.host_str()
}

impl Grant {
    /// For a new `App`: an answer on its way can't match.
    pub fn after(other: &Grant) -> Self {
        Self { attempt: other.attempt + 1, ..Self::default() }
    }

    pub fn attempt(&self) -> u64 {
        self.attempt
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn start(&mut self, url: String) -> u64 {
        self.stop();
        self.url = url;
        self.stage = Stage::Asking;
        self.attempt
    }

    /// Stops what's going on: nothing that comes back for it counts after this.
    pub fn stop(&mut self) {
        self.attempt += 1;
        self.url.clear();
        self.stage = Stage::Idle;
    }

    /// The form's gone. What's left to say about the save comes back, as the form is no longer in its way.
    pub fn closed(&mut self) -> Option<Place> {
        self.stop();
        self.place.take()
    }

    /// The form's Save, which closes the form if it works: a wait doesn't outlive that, and one that fails can start
    /// again. What's kept stays, as the Save still needs it.
    pub fn saving(&mut self) {
        if matches!(self.stage, Stage::Asking | Stage::Waiting(_)) {
            self.stop();
        }
    }

    /// Something in the address box changed. A token on its way to the store is under the address it was asked for,
    /// where it's right, so that goes on.
    pub fn address_edited(&mut self, url: &str) {
        let done = matches!(self.stage, Stage::Idle | Stage::Keeping);
        if !done && url != self.url {
            self.stop();
        }
    }

    pub fn fail(&mut self, why: String) {
        self.stage = Stage::Failed { why, again: true };
    }

    pub fn current(&self, attempt: u64) -> bool {
        attempt == self.attempt
    }

    pub fn keep_place(&mut self, place: Option<Place>) {
        self.place = place;
    }

    pub fn kept(&mut self, attempt: u64, token: Token) {
        if self.current(attempt) {
            self.stage = Stage::Kept(token);
        }
    }

    /// The token just kept, if this form's Save should use it instead of what the field holds.
    pub fn token_for(&self, url: &str) -> Option<&Token> {
        match &self.stage {
            Stage::Kept(token) if url == self.url => Some(token),
            _ => None,
        }
    }

    /// What became of asking for a code: how long to wait before the first poll, if there's a code.
    pub fn asked(&mut self, attempt: u64, answer: Asked) -> Option<u64> {
        if !self.current(attempt) {
            return None;
        }
        match answer {
            Asked::Code(code) => return Some(self.waiting(code)),
            Asked::NoGrant => {
                self.stage = Stage::TokenOnly {
                    why: "This server doesn't offer connecting in the browser, which Readeck has from version 0.21.",
                };
            }
            Asked::NoPublicNames => {
                self.stage = Stage::TokenOnly {
                    why: "This Readeck can't connect in the browser, as it can't look up public web addresses from where \
                          it runs.",
                };
            }
            Asked::Failed(why) => self.fail(why),
        }
        None
    }

    fn waiting(&mut self, code: Code) -> u64 {
        let Code { ticket, user_code, address, address_with_code, expires_in_secs, interval_secs } = code;
        let open = [address_with_code.as_ref(), Some(&address)];
        let open = open.into_iter().flatten().find(|there| openable(&self.url, there)).cloned();
        let wait_ms = interval_secs * 1000;
        let left_ms = expires_in_secs * 1000;
        self.stage =
            Stage::Waiting(Waiting { ticket, user_code, address, open, wait_ms, left_ms, trouble: String::new() });
        wait_ms
    }

    /// The wait is over: what to poll with, if this attempt is still going.
    pub fn due(&self, attempt: u64) -> Option<Ticket> {
        match &self.stage {
            Stage::Waiting(waiting) if self.current(attempt) => Some(waiting.ticket.clone()),
            _ => None,
        }
    }

    /// An answer to a poll, which took `took_ms`.
    pub fn polled(&mut self, attempt: u64, answer: Poll, took_ms: u64) -> Option<Next> {
        if !self.current(attempt) {
            return None;
        }
        let Stage::Waiting(waiting) = &mut self.stage else { return None };
        waiting.left_ms = waiting.left_ms.saturating_sub(waiting.wait_ms.saturating_add(took_ms));
        match answer {
            Poll::Pending => waiting.trouble.clear(),
            Poll::SlowDown => {
                waiting.trouble.clear();
                waiting.wait_ms += SLOW_DOWN_MS;
            }
            Poll::Failed(why) => waiting.trouble = why,
            Poll::Token(token) => {
                self.stage = Stage::Keeping;
                return Some(Next::Keep(token));
            }
            Poll::Denied => {
                self.fail("That was turned down in Readeck, so nothing changed. Start again if it was a slip.".into());
                return None;
            }
            Poll::Expired => {
                self.fail(Self::ran_out(""));
                return None;
            }
            Poll::ClientGone => {
                let why = "That took too long: Readeck forgets a request like this after 10 minutes. Start again for a \
                           new code.";
                self.fail(why.into());
                return None;
            }
            Poll::Refused(why) => {
                self.fail(why);
                return None;
            }
        }
        if waiting.left_ms > 0 {
            return Some(Next::Wait(waiting.wait_ms));
        }
        let why = Self::ran_out(&waiting.trouble);
        self.fail(why);
        None
    }

    fn ran_out(trouble: &str) -> String {
        let why = "The code ran out before it was approved, so nothing changed. Start again for a new one.";
        if trouble.is_empty() { why.into() } else { format!("{why} The last try got no answer: {trouble}") }
    }

    /// What can be pressed, top to bottom. `checking`: a save is on its way, which this mustn't run into.
    fn rows(&self, checking: bool) -> Vec<(&'static str, Does)> {
        if checking {
            return Vec::new();
        }
        match &self.stage {
            Stage::Idle => vec![(CONNECT, Does::Start)],
            Stage::Waiting(waiting) => {
                let open = waiting.open.clone().map(|url| (OPEN, Does::Open(url)));
                open.into_iter().chain([(AGAIN, Does::Start)]).collect()
            }
            Stage::Failed { again: true, .. } => vec![(AGAIN, Does::Start)],
            Stage::Asking | Stage::Keeping | Stage::Kept(_) | Stage::TokenOnly { .. } | Stage::Failed { .. } => {
                Vec::new()
            }
        }
    }

    pub fn row(&self, row: usize, checking: bool) -> Option<Does> {
        self.rows(checking).into_iter().nth(row).map(|(_, does)| does)
    }

    pub fn view(&self, checking: bool) -> GrantView {
        let rows = self.rows(checking).into_iter();
        let rows = rows.map(|(label, _)| ConnectRow { label: label.to_string(), url: None }).collect();
        let mut view = GrantView { stage: "idle", failed: false, ..GrantView::default() };
        view.rows = rows;
        match &self.stage {
            Stage::Idle => view.text = "Or leave the token empty and approve Onward in your browser.".into(),
            Stage::Asking => {
                view.stage = "asking";
                view.text = "Asking Readeck for a code…".into();
            }
            Stage::Waiting(waiting) => {
                view.stage = "waiting";
                view.text =
                    "Open this address on any device signed in to Readeck and enter the code. Onward carries on \
                             by itself once you've approved it."
                        .into();
                view.code.clone_from(&waiting.user_code);
                view.address.clone_from(&waiting.address);
                let foreign = "Onward won't open this address itself, as it's not on the server you entered. Check it \
                               before you go there.";
                let foreign = if waiting.open.is_none() { foreign } else { "" };
                let still = (!waiting.trouble.is_empty()).then(|| format!("{} Still trying.", waiting.trouble));
                view.note = still.unwrap_or_else(|| foreign.to_string());
            }
            Stage::Keeping => {
                view.stage = "keeping";
                view.text = "Approved. Saving…".into();
            }
            Stage::Kept(_) => {
                view.stage = "kept";
                view.text = format!("Connected to {}. It's saved, with no token to paste.", self.url);
            }
            Stage::TokenOnly { why } => {
                view.stage = "token_only";
                view.text = format!("{why} {USE_A_TOKEN}");
            }
            Stage::Failed { why, .. } => {
                view.stage = "failed";
                view.failed = true;
                view.text.clone_from(why);
            }
        }
        view
    }
}

/// The browser way in as the windows draw it, under Readeck's fields. All of it is safe to show, the code included.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GrantView {
    /// "idle", "asking", "waiting", "keeping", "kept", "token_only" or "failed".
    pub stage: &'static str,
    pub text: String,
    /// `text` is bad news.
    pub failed: bool,
    /// The code to enter, and where.
    pub code: String,
    pub address: String,
    /// What else there is to know about either, while waiting.
    pub note: String,
    /// What can be pressed: `App::readeck_row`. These open nothing a terminal should print: `address` has that.
    pub rows: Vec<ConnectRow>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn only_http_on_the_host_that_was_typed_in_is_opened() {
        let entered = "https://readeck.example.com";
        for fine in [
            "https://readeck.example.com/device",
            "http://READECK.example.com:8000/device?user_code=A",
            "https://readeck.example.com/device",
        ] {
            assert!(openable(entered, fine), "{fine}");
        }
        for not in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ssh://readeck.example.com/",
            "https://readeck.example.com.evil.test/device",
            "https://evil.test/readeck.example.com",
            "https://readeck.example.com@evil.test/device",
            "https://user@readeck.example.com/device",
            "https://example.com/device",
            "/device",
            "",
        ] {
            assert!(!openable(entered, not), "{not}");
        }
        assert!(!openable("not a url", "https://readeck.example.com/device"));
    }
}
