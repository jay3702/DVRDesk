//! Rust equivalent of `client.ts`'s transport plumbing. Notably simpler than
//! the old app's version: that one routed every request through Tauri's
//! HTTP plugin specifically to bypass webview CORS restrictions — a native
//! process has no CORS concept at all, so a plain `reqwest::Client` is the
//! whole story.

use std::time::Duration;

const STATUS_PATHS: &[&str] = &["/api/v1/status", "/api/status", "/status"];

/// Mirrors the old `probeUrl()`: tries a few candidate status endpoints in
/// order, treating *any* HTTP response — even an error status — as
/// "reachable." We only care whether the server is there and answering on
/// this address, not whether this particular path is the right API shape.
pub async fn probe_url(base_url: &str, timeout: Duration) -> bool {
    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let base = base_url.trim_end_matches('/');
    for path in STATUS_PATHS {
        let url = format!("{base}{path}");
        if client.get(&url).send().await.is_ok() {
            return true;
        }
    }
    false
}
