//! Rust equivalent of `api/recordings.ts` — covers shows/episodes now too,
//! matching the old app's own file boundary (its `recordings.ts` handled
//! shows/episodes/movies/recordings together, not one file per screen).
//! `mark_as_not_recorded`/the `DvrFile`/`RuleID` lookup it depends on are
//! deliberately not ported yet — a smaller, rarer action deferred to keep
//! this pass's scope bounded.
#![allow(dead_code)]

use super::types::{Recording, Show};

pub async fn fetch_shows(server_url: &str) -> Result<Vec<Show>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/shows");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /api/v1/shows", resp.status()));
    }
    resp.json::<Vec<Show>>()
        .await
        .map_err(|e| format!("Failed to parse shows response: {e}"))
}

/// Flat, all-shows episode feed — used only by Search, which builds its own
/// client-side keyword index rather than calling a server-side search
/// endpoint (Channels DVR doesn't have one). Same shape as `/api/v1/all`
/// and `/api/v1/shows/{id}/episodes`, confirmed via `curl`.
pub async fn fetch_all_episodes(server_url: &str) -> Result<Vec<Recording>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/episodes");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /api/v1/episodes", resp.status()));
    }
    resp.json::<Vec<Recording>>()
        .await
        .map_err(|e| format!("Failed to parse episodes response: {e}"))
}

pub async fn fetch_movies(server_url: &str) -> Result<Vec<Recording>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/movies?sort=date_added&order=desc");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /api/v1/movies", resp.status()));
    }
    resp.json::<Vec<Recording>>()
        .await
        .map_err(|e| format!("Failed to parse movies response: {e}"))
}

pub async fn fetch_episodes(server_url: &str, show_id: &str) -> Result<Vec<Recording>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/shows/{show_id}/episodes?sort=date_added&order=desc");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "API error {}: GET /api/v1/shows/{show_id}/episodes",
            resp.status()
        ));
    }
    resp.json::<Vec<Recording>>()
        .await
        .map_err(|e| format!("Failed to parse episodes response: {e}"))
}

/// Standalone watched-toggle (as opposed to `persist_playback_update`'s
/// combined save+mark-watched during playback) — same endpoint either way,
/// used by the show/episode list's toggle button.
pub async fn set_watched(server_url: &str, id: &str, watched: bool) -> Result<(), String> {
    let base = server_url.trim_end_matches('/');
    let action = if watched { "watch" } else { "unwatch" };
    let url = format!("{base}/dvr/files/{id}/{action}");

    let resp = reqwest::Client::new()
        .put(&url)
        .send()
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: PUT {action}", resp.status()));
    }
    Ok(())
}

pub async fn trash_recording(server_url: &str, id: &str) -> Result<(), String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/dvr/files/{id}");

    let resp = reqwest::Client::new()
        .delete(&url)
        .send()
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "API error {}: DELETE /dvr/files/{id}",
            resp.status()
        ));
    }
    Ok(())
}

pub async fn fetch_recordings(server_url: &str) -> Result<Vec<Recording>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/all?sort=date_added&order=desc&source=recordings");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /api/v1/all", resp.status()));
    }

    resp.json::<Vec<Recording>>()
        .await
        .map_err(|e| format!("Failed to parse recordings response: {e}"))
}

/// Port of `persistPlaybackUpdate` from the old app: saves the current
/// position, and — if `mark_watched` — also flags the recording watched,
/// sequentially in one call (mirrors the old function doing both in one
/// place rather than as two independently-triggered mutations). Both
/// episode and movie endpoints are identical (`/dvr/files/{id}/...`) — the
/// old app's separate `setEpisodeWatched`/`setMovieWatched` helpers were
/// the same URL either way, so there's no kind-specific branching here.
/// Best-effort by design, same as the old app: a failure here should never
/// interrupt playback, just get logged by the caller.
pub async fn persist_playback_update(
    server_url: &str,
    id: &str,
    position_secs: f64,
    mark_watched: bool,
) -> Result<(), String> {
    let base = server_url.trim_end_matches('/');
    let clamped = position_secs.max(0.0).floor() as i64;
    let client = reqwest::Client::new();

    let pt_url = format!("{base}/dvr/files/{id}/playback_time/{clamped}");
    let resp = client
        .put(&pt_url)
        .send()
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: PUT playback_time", resp.status()));
    }

    if mark_watched {
        let watch_url = format!("{base}/dvr/files/{id}/watch");
        let resp = client
            .put(&watch_url)
            .send()
            .await
            .map_err(|e| format!("Network error reaching {base}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("API error {}: PUT watch", resp.status()));
        }
    }

    Ok(())
}

/// Fetches a single recording by its file id — used to jump from a past
/// guide slot's recorded-status hit straight to the actual recording.
/// Confirmed via `curl` that `GET /api/v1/episodes/{id}` returns the right
/// data for *both* episodes and movies (the endpoint name is misleading —
/// it isn't episode-specific), so one function covers both kinds; the
/// caller uses `Recording::recording_kind()` on the result to know which
/// screen to navigate to.
pub async fn fetch_recording_by_id(server_url: &str, id: &str) -> Result<Recording, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/episodes/{id}");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "API error {}: GET /api/v1/episodes/{id}",
            resp.status()
        ));
    }
    resp.json::<Recording>()
        .await
        .map_err(|e| format!("Failed to parse recording response: {e}"))
}

/// Recordings have a direct raw-file endpoint that bypasses Channels DVR's
/// HLS packaging entirely — confirmed during the Phase 2 risk spike to
/// start instantly (matching Clicker) and to be the *only* path that
/// carries embedded caption tracks, since the HLS endpoints (remux or
/// transcoded) strip subtitle tracks regardless of encoder mode. (Live
/// channels also have a direct-file equivalent, `api::live::live_manifest_url`
/// — an earlier version of this comment claimed otherwise; see that
/// function's own doc comment for how that was confirmed.)
///
/// Only used for a *completed* recording — see `resolve_hls_variant_url`
/// for why an in-progress one needs a different endpoint entirely, not
/// just this one swapped in.
pub fn direct_play_url(server_url: &str, recording_id: &str) -> String {
    let base = server_url.trim_end_matches('/');
    format!("{base}/dvr/files/{recording_id}/stream.mpg")
}

/// For a recording that's still being actively written — confirmed via
/// direct, repeated live testing that `direct_play_url`'s raw `stream.mpg`
/// endpoint reproducibly never starts playback for one of these (position/
/// duration/audio never become available, even after 800+ frames), while
/// the exact same code path works within ~2 seconds for a completed file.
///
/// Handing mpv the **master** HLS playlist directly (the obvious next
/// thing to try, and also confirmedly broken) turned out to have its own,
/// different root cause — found by finally building temporary mpv log-
/// message plumbing (this app had none; blind option-guessing had stopped
/// being productive) and reading mpv's own log in real time. It showed
/// ffmpeg's HLS demuxer opening *every one* of the master's ~8 ABR variant
/// playlists in turn, each requiring its own network round-trip, before
/// considering the file loaded — normal ffmpeg behavior for building a
/// multi-variant format context, and fast enough on a short/bounded
/// playlist, but this account's in-progress file already had 1192
/// segments (Channels DVR keeps every segment from the start of the
/// recording rather than a sliding window — confirmed via `curl` diffing
/// an in-progress playlist against a completed one), so 8 sequential
/// variant probes against a playlist that size took far longer than any
/// reasonable load should — consistent with every test timing out well
/// past what a completed file's load ever needed.
///
/// Fix: skip the master entirely. This fetches it once (a plain, cheap
/// `GET`, not routed through mpv), and returns the *first* variant's own
/// media-playlist URL directly — the passthrough/no-transcode one, always
/// listed first, confirmed via `curl`. Handing mpv a single-variant
/// playlist means ffmpeg's demuxer has exactly one stream to probe, not
/// eight, which is what actually made the load fast again in testing.
pub async fn resolve_hls_variant_url(
    server_url: &str,
    recording_id: &str,
) -> Result<String, String> {
    let base = server_url.trim_end_matches('/');
    let master_url = format!("{base}/dvr/files/{recording_id}/hls/master.m3u8");

    let resp = reqwest::get(&master_url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET {master_url}", resp.status()));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read HLS master playlist: {e}"))?;

    let mut lines = body.lines();
    while let Some(line) = lines.next() {
        if line.starts_with("#EXT-X-STREAM-INF") {
            if let Some(url_line) = lines.next() {
                let url_line = url_line.trim();
                if !url_line.is_empty() && !url_line.starts_with('#') {
                    return Ok(url_line.to_string());
                }
            }
        }
    }
    Err("HLS master playlist had no stream variants".to_string())
}
