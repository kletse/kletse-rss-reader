//! The feed server the app reads from: Miniflux's own API, or the Google Reader API. backend.rs calls this and
//! doesn't care which; each method hands the call to the client for the configured kind.

use crate::error::Error;
use crate::feeds::{Counts, EntryPage, EntryStatus, Selection};
use crate::greader;
use crate::miniflux;
use crate::paging::Order;

#[derive(Debug)]
pub enum Server {
    Miniflux(miniflux::Client),
    GReader(greader::Client),
}

impl Server {
    /// Checks the address and credentials; returns the username.
    pub async fn check(&self) -> Result<String, Error> {
        match self {
            Server::Miniflux(c) => c.check().await,
            Server::GReader(c) => c.check().await,
        }
    }

    /// Every feed with its counts, and how many entries are starred.
    pub async fn feed_counts(&self) -> Result<Counts, Error> {
        match self {
            Server::Miniflux(c) => c.feed_counts().await,
            Server::GReader(c) => c.feed_counts().await,
        }
    }

    /// A page of the selection's entries with `status`; for `Selection::Starred`, read and unread alike: the
    /// status is ignored there.
    pub async fn entries(
        &self,
        sel: Selection,
        status: EntryStatus,
        order: Order,
        past: i64,
        limit: usize,
    ) -> Result<EntryPage, Error> {
        match self {
            Server::Miniflux(c) => c.entries(sel, status, order, past, limit).await,
            Server::GReader(c) => c.entries(sel, status, order, past, limit).await,
        }
    }

    pub async fn search(&self, query: &str, offset: usize, limit: usize) -> Result<EntryPage, Error> {
        match self {
            Server::Miniflux(c) => c.search(query, offset, limit).await,
            Server::GReader(c) => c.search(query, offset, limit).await,
        }
    }

    pub async fn newest_unread_id(&self, sel: Selection) -> Result<i64, Error> {
        match self {
            Server::Miniflux(c) => c.newest_unread_id(sel).await,
            Server::GReader(c) => c.newest_unread_id(sel).await,
        }
    }

    pub async fn unread_count(&self, sel: Selection, up_to: i64) -> Result<i64, Error> {
        match self {
            Server::Miniflux(c) => c.unread_count(sel, up_to).await,
            Server::GReader(c) => c.unread_count(sel, up_to).await,
        }
    }

    pub async fn mark_read_up_to(&self, sel: Selection, up_to: i64) -> Result<usize, Error> {
        match self {
            Server::Miniflux(c) => c.mark_read_up_to(sel, up_to).await,
            Server::GReader(c) => c.mark_read_up_to(sel, up_to).await,
        }
    }

    pub async fn set_status(&self, ids: &[i64], status: EntryStatus) -> Result<(), Error> {
        match self {
            Server::Miniflux(c) => c.set_status(ids, status).await,
            Server::GReader(c) => c.set_status(ids, status).await,
        }
    }

    /// Stars or unstars an entry. Call it only to change it: Miniflux can only toggle.
    pub async fn set_starred(&self, id: i64, starred: bool) -> Result<(), Error> {
        match self {
            Server::Miniflux(c) => c.set_starred(id, starred).await,
            Server::GReader(c) => c.set_starred(id, starred).await,
        }
    }

    pub async fn fetch_content(&self, id: i64) -> Result<String, Error> {
        match self {
            Server::Miniflux(c) => c.fetch_content(id).await,
            Server::GReader(c) => c.fetch_content(id).await,
        }
    }

    pub async fn refresh_all(&self) -> Result<(), Error> {
        match self {
            Server::Miniflux(c) => c.refresh_all().await,
            Server::GReader(c) => c.refresh_all().await,
        }
    }
}
