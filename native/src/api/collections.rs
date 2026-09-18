//! Channels DVR's "Library Collections" — a server-side, customizable
//! grouping of *recordings* (confirmed against the real API: `GET
//! /api/v1/collections` returns `[{"id","collection_type","name",
//! "content_count",...}]`; `GET /api/v1/collections/{id}/content` returns
//! the member items). Not to be confused with "Channel Collections"
//! (`GET /dvr/collections/channels`, `api/guide.rs`'s old, now-removed
//! `Collection`/`fetch_collections` — a genuinely different DVR feature
//! that groups live channels, not recordings).
//!
//! Only `collection_type == "shows"` is deserialized here — confirmed
//! live against the real server that a "shows" collection's content items
//! are field-for-field identical to `types::Show` (`id`/`name`/`summary`/
//! `image_url`/`episode_count`/`number_unwatched`/`favorited`/
//! `last_recorded_at`/`created_at`/`updated_at`), so no new type was
//! needed for that case. No `"movies"`-type collection exists on the
//! account this was verified against, so that shape is unconfirmed —
//! `ui/collections.rs` shows a "not supported yet" message for any other
//! `collection_type` rather than guessing at its item shape.

use super::types::Show;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LibraryCollection {
    pub id: String,
    #[serde(default)]
    pub collection_type: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub content_count: u32,
}

pub async fn fetch_collections(server_url: &str) -> Result<Vec<LibraryCollection>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/collections");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "API error {}: GET /api/v1/collections",
            resp.status()
        ));
    }
    resp.json::<Vec<LibraryCollection>>()
        .await
        .map_err(|e| format!("Failed to parse collections response: {e}"))
}

/// Only meaningful for a `collection_type == "shows"` collection — see the
/// module doc comment for why other types aren't fetched through this.
pub async fn fetch_collection_shows(server_url: &str, id: &str) -> Result<Vec<Show>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/collections/{id}/content");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "API error {}: GET /api/v1/collections/{id}/content",
            resp.status()
        ));
    }
    resp.json::<Vec<Show>>()
        .await
        .map_err(|e| format!("Failed to parse collection content response: {e}"))
}
