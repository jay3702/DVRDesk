//! Rust equivalent of the live-channel half of `client.ts`/`Live.tsx`.
//! Deliberately much simpler than the old app's `resolveLiveManifestUrl`,
//! which HEAD-probed several candidate URLs in sequence to work around
//! hls.js/WebKitGTK fragility — confirmed in the Phase 2 risk spike that
//! mpv plays the direct `/devices/ANY/channels/<number>/hls/master.m3u8`
//! URL reliably on its own, so that whole defensive-probing layer isn't
//! needed here.

use std::collections::HashMap;

use super::types::Channel;

pub async fn fetch_channels(server_url: &str) -> Result<Vec<Channel>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/channels");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /api/v1/channels", resp.status()));
    }
    resp.json::<Vec<Channel>>()
        .await
        .map_err(|e| format!("Failed to parse channels response: {e}"))
}

/// Confirmed this session that this assumption from the original plan was
/// wrong: live channels *do* have a direct-file equivalent, exactly like
/// recordings' `/dvr/files/{id}/stream.mpg`. `/devices/ANY/channels/{number}/hls/master.m3u8`
/// works, but — confirmed via `curl` timing the DVR server directly,
/// independent of this app or mpv entirely — takes 3-12 seconds to even
/// respond, because the server has to acquire/lock the tuner *and* start
/// transcoding/segmenting into HLS before it can return anything at all.
/// `/devices/ANY/channels/{number}/stream.mpg` (found by extracting the
/// literal URL-building strings out of Clicker's own binary, since no
/// source was available) returns the raw tuner stream directly — confirmed
/// via `curl` to have a time-to-first-byte around 0.5s regardless of
/// channel, and confirmed end-to-end with this app's own player to cut
/// real tune time from 5-9s down to 1.4-2.1s on the same channels, no mpv
/// option changes needed at all.
pub fn live_manifest_url(server_url: &str, channel: &Channel) -> String {
    let base = server_url.trim_end_matches('/');
    format!("{base}/devices/ANY/channels/{}/stream.mpg", channel.number)
}

/// Port of the old app's `channelLogos.ts` — used by Recent's list rows to
/// show the channel/network logo a recording aired on, instead of the
/// recording's own (episode-specific) thumbnail.
///
/// `logo_url` covers essentially every channel in practice (confirmed via
/// `curl` against the real API), but a `station_id`-based fallback URL
/// through the server's own TMS image proxy is kept for parity with the
/// old app, in case a channel is ever missing it.
pub fn channel_logo_url(channel: &Channel, server_url: &str) -> Option<String> {
    if let Some(direct) = channel
        .logo_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(direct.to_string());
    }
    let station = channel
        .station_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let base = server_url.trim_end_matches('/');
    Some(format!(
        "{base}/tmsimg/assets/s{station}_ll_h15_ab.png?w=360&h=270"
    ))
}

/// For channels with no direct `logo_url`, probes the TMS proxy's known
/// letter-suffix variants (`_ab`/`_ac`/`_aa`) and fills in whichever one
/// actually exists — ports the old app's `<img onError>` variant-retry
/// chain (`nextLogoVariant`), which egui's declarative image loading has
/// no equivalent for (a failed load just shows a fixed error placeholder,
/// with no "try the next URL" hook to attach to). Confirmed necessary
/// against the real server, not theoretical: CNN's own station id 404s on
/// `_ab` specifically while `_ac`/`_aa` both resolve fine.
///
/// Uses `GET`, not `HEAD` — confirmed via `curl` that this server 404s on
/// *every* `HEAD` request to this endpoint regardless of whether the same
/// URL succeeds via `GET` (it doesn't implement `HEAD` here at all, not a
/// question of which variant exists). The images are small, so fetching
/// the body just to confirm existence is an acceptable tradeoff.
///
/// Every channel's own (up to 3-variant) probe chain runs concurrently via
/// `JoinSet`, not sequentially — a first version awaited each channel's
/// GETs one after another and, with enough logo-less channels on a real
/// system, that measurably stalled the whole Recent screen's load (traced
/// directly: the channel-fetch task simply hadn't finished within a
/// 15-second test window). Concurrency bounds total latency to roughly the
/// slowest single channel's chain instead of the sum of all of them.
pub async fn resolve_station_logo_variants(channels: &mut [Channel], server_url: &str) {
    let base = server_url.trim_end_matches('/').to_string();
    let client = reqwest::Client::new();

    let mut set = tokio::task::JoinSet::new();
    for (idx, ch) in channels.iter().enumerate() {
        if ch
            .logo_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
        {
            continue;
        }
        let Some(station) = ch.station_id.clone().filter(|s| !s.trim().is_empty()) else {
            continue;
        };
        let base = base.clone();
        let client = client.clone();
        set.spawn(async move {
            for variant in ["ab", "ac", "aa"] {
                let url = format!(
                    "{base}/tmsimg/assets/s{}_ll_h15_{variant}.png?w=360&h=270",
                    station.trim()
                );
                if let Ok(resp) = client.get(&url).send().await {
                    if resp.status().is_success() {
                        return (idx, Some(url));
                    }
                }
            }
            (idx, None)
        });
    }

    while let Some(res) = set.join_next().await {
        if let Ok((idx, Some(url))) = res {
            channels[idx].logo_url = Some(url);
        }
    }
}

/// Keyed by `id`, `name`, and `number` (each stored both as-is and
/// lowercased) — matching the old app's `buildChannelLogoMap`, since a
/// `Recording.channel` value isn't reliably one specific field.
pub fn build_channel_logo_map(channels: &[Channel], server_url: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for ch in channels {
        let Some(logo) = channel_logo_url(ch, server_url) else {
            continue;
        };
        for key in [ch.id.as_str(), ch.name.as_str(), ch.number.as_str()] {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            map.insert(key.to_string(), logo.clone());
            map.insert(key.to_lowercase(), logo.clone());
        }
    }
    map
}

pub fn logo_for_channel_key(key: Option<&str>, map: &HashMap<String, String>) -> Option<String> {
    let key = key.map(str::trim).filter(|s| !s.is_empty())?;
    map.get(key)
        .or_else(|| map.get(&key.to_lowercase()))
        .cloned()
}
