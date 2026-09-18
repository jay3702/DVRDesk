//! Rust equivalent of `api/library.ts` — Channels DVR's "Library"/custom
//! video groups feature. No mutation endpoints exist for these at all
//! (confirmed against the real API — no watched/trash for library videos),
//! so unlike `api/recordings.rs` this is read-only.

use super::types::{Video, VideoGroup};

pub async fn fetch_video_groups(server_url: &str) -> Result<Vec<VideoGroup>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/video_groups");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "API error {}: GET /api/v1/video_groups",
            resp.status()
        ));
    }
    resp.json::<Vec<VideoGroup>>()
        .await
        .map_err(|e| format!("Failed to parse video groups response: {e}"))
}

/// Flat, all-groups video feed — used only by Search. Confirmed via `curl`
/// to match the per-group endpoint's shape.
pub async fn fetch_all_videos(server_url: &str) -> Result<Vec<Video>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/videos");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /api/v1/videos", resp.status()));
    }
    resp.json::<Vec<Video>>()
        .await
        .map_err(|e| format!("Failed to parse videos response: {e}"))
}

pub async fn fetch_videos(server_url: &str, group_id: &str) -> Result<Vec<Video>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/video_groups/{group_id}/videos");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "API error {}: GET /api/v1/video_groups/{group_id}/videos",
            resp.status()
        ));
    }
    resp.json::<Vec<Video>>()
        .await
        .map_err(|e| format!("Failed to parse videos response: {e}"))
}
