//! Client for the optional, separately-run `guide-history-service`
//! companion — see that crate's own doc comment for why it exists (the
//! DVR's own guide API is forward-only, so seeing "what was on yesterday"
//! needs something continuously capturing slots before they age out,
//! independent of whether this app happens to be running). Entirely
//! optional: only called when `AppSettings.history_service_url` is set.

use super::guide::GuideProgram;

pub async fn fetch_history(
    service_url: &str,
    from: i64,
    to: i64,
) -> Result<Vec<GuideProgram>, String> {
    let base = service_url.trim_end_matches('/');
    let url = format!("{base}/history?from={from}&to={to}");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching history service {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "History service error {}: GET /history",
            resp.status()
        ));
    }
    resp.json::<Vec<GuideProgram>>()
        .await
        .map_err(|e| format!("Failed to parse history service response: {e}"))
}

/// Used by the Settings "Test" button — just confirms something answers at
/// that URL, doesn't need the response body.
pub async fn probe(service_url: &str) -> Result<(), String> {
    let base = service_url.trim_end_matches('/');
    let url = format!("{base}/health");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "History service error {}: GET /health",
            resp.status()
        ));
    }
    Ok(())
}
