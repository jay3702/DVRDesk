//! Checks for a newer DVRDesk Native release on GitHub — scoped strictly
//! to this app's own `native-v*` tag prefix. The old Tauri-based DVRDesk
//! publishes under a plain `v*` scheme in the *same* repo, and GitHub's
//! `/releases/latest` endpoint doesn't distinguish between the two (a
//! real, confirmed problem: publishing this app's first release broke the
//! other app's own update-checker, since `/releases/latest` is repo-wide,
//! not tag-prefix-scoped, and started returning `native-v2.0.1` instead of
//! whatever the old app's own latest `v*` release was) — so this fetches
//! the release list and filters explicitly by tag prefix, rather than
//! trusting "latest" to mean what it sounds like it means.

use serde::Deserialize;

const REPO: &str = "jay3702/DVRDesk";

pub struct UpdateInfo {
    /// Just the version portion, e.g. `"2.0.2"` — the `native-v` prefix is
    /// stripped since callers only want to compare/display the version.
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

/// `Ok(None)` covers both "already up to date" and "no native-v* release
/// found at all" — callers only ever want to know whether to show an
/// update banner, not why not.
pub async fn fetch_update_info() -> Result<Option<UpdateInfo>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=10");
    let client = reqwest::Client::builder()
        .user_agent("dvrdesk-native")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Network error checking for updates: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API error {}: GET /releases", resp.status()));
    }
    let releases: Vec<Release> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse releases response: {e}"))?;

    let Some(latest) = releases
        .into_iter()
        .find(|r| !r.draft && !r.prerelease && r.tag_name.starts_with("native-v"))
    else {
        return Ok(None);
    };

    let latest_version = latest.tag_name.trim_start_matches("native-v");
    let current_version = env!("CARGO_PKG_VERSION");
    if is_version_newer(latest_version, current_version) {
        Ok(Some(UpdateInfo {
            version: latest_version.to_string(),
            url: latest.html_url,
        }))
    } else {
        Ok(None)
    }
}

fn parse_version_parts(v: &str) -> Vec<u64> {
    v.split(['.', '-'])
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect()
}

fn is_version_newer(latest: &str, current: &str) -> bool {
    let a = parse_version_parts(latest);
    let b = parse_version_parts(current);
    let len = a.len().max(b.len());
    for i in 0..len {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        if av != bv {
            return av > bv;
        }
    }
    false
}
