//! Live TV program guide (EPG) + recording creation — Clicker-style: a
//! schedule grid with recording status, and the ability to record a single
//! airing or create a recurring pass directly from a clicked program.
//!
//! None of this exists in the old Tauri app to port from. Every endpoint
//! here was found by reading the Channels DVR server's own admin `bundle.js`
//! (fetched directly from the live server) and independently confirmed via
//! `curl` against the real DVR — not guessed or assumed from generic REST
//! conventions.
#![allow(dead_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One programme slot from the XMLTV guide feed, scoped to what the grid
/// and the record dialog actually need — XMLTV carries a lot more (ratings,
/// credits, video quality) that this app has no use for yet.
///
/// Also doubles as the wire format for the optional `guide-history-service`
/// companion (see `guide_history.rs`) — that service independently defines
/// the same shape (it's a separate, GUI-free crate, not a shared lib), so
/// field names here must stay in sync with it by hand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuideProgram {
    pub channel: String,
    pub start: i64,
    pub stop: i64,
    pub title: String,
    pub desc: Option<String>,
    pub categories: Vec<String>,
    pub image: Option<String>,
    /// `<series-id system="tms">` — present for recurring series content,
    /// absent for movies/one-offs. Drives whether the record dialog offers
    /// "Create Pass" at all.
    pub series_id: Option<String>,
    /// `<episode-num system="tms">` — identical in format to the native
    /// API's `ProgramID` (confirmed by cross-referencing the same program
    /// in both XMLTV and `/dvr/guide/airings/{seriesID}`), so it doubles as
    /// the lookup key into `/dvr/programs`'s recorded-status dict.
    pub program_id: Option<String>,
    pub is_new: bool,
}

/// `GET /devices/ANY/guide/xmltv?duration=<seconds>` — confirmed via curl
/// that `duration` genuinely bounds the response (479KB@1h, 5.9MB@24h,
/// ~0.2s over LAN either way) and that this is the same endpoint the admin
/// UI's own "Copy EPG URL" feature uses, just with a shorter window — a
/// real, stable, intended path, not an internal implementation detail.
pub async fn fetch_guide(
    server_url: &str,
    duration_secs: u32,
) -> Result<Vec<GuideProgram>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/devices/ANY/guide/xmltv?duration={duration_secs}");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "API error {}: GET /devices/ANY/guide/xmltv",
            resp.status()
        ));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read guide response: {e}"))?;
    parse_xmltv(&body)
}

fn parse_xmltv_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_str(s, "%Y%m%d%H%M%S %z")
        .ok()
        .map(|dt| dt.timestamp())
}

#[derive(Default)]
struct ProgBuilder {
    start: Option<String>,
    stop: Option<String>,
    channel: Option<String>,
    title: String,
    desc: Option<String>,
    categories: Vec<String>,
    image: Option<String>,
    series_id: Option<String>,
    program_id: Option<String>,
    is_new: bool,
}

impl ProgBuilder {
    fn build(self) -> Option<GuideProgram> {
        let channel = self.channel?;
        let start = parse_xmltv_time(self.start.as_deref()?)?;
        let stop = parse_xmltv_time(self.stop.as_deref()?)?;
        Some(GuideProgram {
            channel,
            start,
            stop,
            title: self.title,
            desc: self.desc,
            categories: self.categories,
            image: self.image,
            series_id: self.series_id.filter(|s| !s.is_empty()),
            program_id: self.program_id.filter(|s| !s.is_empty()),
            is_new: self.is_new,
        })
    }
}

/// Hand-rolled event-based parse rather than `quick_xml`'s serde-derive path
/// — XMLTV's repeating, irregularly-shaped `<programme>` children (optional
/// `<desc>`, repeated `<category>`, empty-element flags like `<new/>`) don't
/// map cleanly onto a single derived struct, and we only need a handful of
/// fields out of a much larger real schema.
fn parse_xmltv(xml: &str) -> Result<Vec<GuideProgram>, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut programs = Vec::new();
    let mut cur: Option<ProgBuilder> = None;
    let mut cur_element: Vec<u8> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_vec();
                match name.as_slice() {
                    b"programme" => {
                        let mut b = ProgBuilder::default();
                        for attr in e.attributes().flatten() {
                            let value = attr
                                .decode_and_unescape_value(reader.decoder())
                                .unwrap_or_default()
                                .to_string();
                            match attr.key.as_ref() {
                                b"start" => b.start = Some(value),
                                b"stop" => b.stop = Some(value),
                                b"channel" => b.channel = Some(value),
                                _ => {}
                            }
                        }
                        cur = Some(b);
                    }
                    b"new" => {
                        if let Some(c) = cur.as_mut() {
                            c.is_new = true;
                        }
                    }
                    b"icon" => {
                        if let Some(c) = cur.as_mut() {
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"src" {
                                    c.image = Some(
                                        attr.decode_and_unescape_value(reader.decoder())
                                            .unwrap_or_default()
                                            .to_string(),
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
                cur_element = name;
            }
            Ok(Event::Text(t)) => {
                if let Some(c) = cur.as_mut() {
                    let text = t.unescape().unwrap_or_default().to_string();
                    match cur_element.as_slice() {
                        b"title" => c.title = text,
                        b"desc" => c.desc = Some(text),
                        b"category" => c.categories.push(text),
                        b"series-id" => c.series_id = Some(text),
                        b"episode-num" => c.program_id = Some(text),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() == b"programme" {
                    if let Some(p) = cur.take().and_then(ProgBuilder::build) {
                        programs.push(p);
                    }
                }
                cur_element.clear();
            }
            Ok(_) => {}
            Err(e) => return Err(format!("Guide XML parse error: {e}")),
        }
        buf.clear();
    }

    Ok(programs)
}

/// A program's status per `/dvr/programs` — `Recorded` carries the file id
/// directly (confirmed via `curl`: the `"recorded-<id>"`/`"imported-<id>"`
/// suffix *is* the recording's own `id`, e.g. program `EP056252540776` →
/// `recorded-95827` → `GET /api/v1/episodes/95827` returns that exact
/// recording), used to jump straight to a past recorded program without a
/// separate lookup. `Recording` is the distinct "actively recording right
/// now" state (confirmed via `curl` against a program being recorded live),
/// kept separate from `Recorded` for a distinct grid indicator — anything
/// else (queued/etc.) is left unrecognized here and covered more robustly
/// by `fetch_jobs` matching on Channel+Time instead of trusting every
/// possible state-string prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramStatus {
    Recorded { file_id: String },
    Recording,
}

/// `GET /dvr/programs` — bulk `ProgramID -> "state[-id]"` dict.
pub async fn fetch_recorded_status(
    server_url: &str,
) -> Result<HashMap<String, ProgramStatus>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/dvr/programs");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /dvr/programs", resp.status()));
    }
    let raw: HashMap<String, String> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse /dvr/programs response: {e}"))?;

    let mut out = HashMap::new();
    for (program_id, status) in raw {
        let mut parts = status.splitn(2, '-');
        let state = parts.next().unwrap_or("");
        let rest = parts.next();
        match state {
            "recorded" | "imported" => {
                if let Some(file_id) = rest {
                    out.insert(
                        program_id,
                        ProgramStatus::Recorded {
                            file_id: file_id.to_string(),
                        },
                    );
                }
            }
            "recording" => {
                out.insert(program_id, ProgramStatus::Recording);
            }
            _ => {}
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    #[serde(rename = "Time")]
    pub time: i64,
    #[serde(rename = "Duration")]
    pub duration: i64,
    #[serde(rename = "Channels", default)]
    pub channels: Vec<String>,
    #[serde(rename = "Channel", default)]
    pub channel: String,
    #[serde(rename = "Skipped", default)]
    pub skipped: bool,
    #[serde(rename = "Failed", default)]
    pub failed: bool,
    #[serde(rename = "Dead", default)]
    pub dead: bool,
    /// Populated once the job has actually started writing a file —
    /// confirmed via `curl` against real active jobs (empty for
    /// not-yet-started future jobs, a real recording id for ones already
    /// in progress). Lets a currently-airing slot with an active recording
    /// offer "play the recording" as a direct-play target, the same id
    /// `api::recordings::fetch_recording_by_id` already knows how to use.
    #[serde(rename = "FileID", default)]
    pub file_id: String,
}

impl Job {
    /// A job is "live" for grid-badging purposes if it hasn't already
    /// failed/died/been skipped — those still show up in `/dvr/jobs` for a
    /// while after the fact but shouldn't read as "this will record."
    pub fn is_active(&self) -> bool {
        !self.failed && !self.dead && !self.skipped
    }

    pub fn matches(&self, channel: &str, time: i64) -> bool {
        if self.time != time {
            return false;
        }
        self.channel == channel || self.channels.iter().any(|c| c == channel)
    }
}

/// `GET /dvr/jobs` — scheduled/active/recently-attempted recordings.
/// Cross-referenced against guide slots by Channel+Time (robust regardless
/// of which `ProgramID` scheme applies to that slot) to badge
/// "Scheduled"/"Recording now" cells.
pub async fn fetch_jobs(server_url: &str) -> Result<Vec<Job>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/dvr/jobs");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /dvr/jobs", resp.status()));
    }
    resp.json::<Vec<Job>>()
        .await
        .map_err(|e| format!("Failed to parse jobs response: {e}"))
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuleEq {
    #[serde(rename = "SeriesID", default)]
    pub series_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "EQ", default)]
    pub eq: RuleEq,
    #[serde(rename = "Paused", default)]
    pub paused: bool,
}

impl Rule {
    pub fn series_id(&self) -> Option<&str> {
        self.eq.series_id.as_deref()
    }
}

/// `GET /dvr/rules` — existing passes. Used to badge series that already
/// have one and to decide whether the dialog offers "Create Pass" at all.
pub async fn fetch_rules(server_url: &str) -> Result<Vec<Rule>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/dvr/rules");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /dvr/rules", resp.status()));
    }
    resp.json::<Vec<Rule>>()
        .await
        .map_err(|e| format!("Failed to parse rules response: {e}"))
}

/// `GET /dvr/guide/airings/{seriesID}` — confirmed (by reading the admin
/// bundle's own record-dialog code) to be exactly what the real app fetches
/// before offering "Record"/"Create Pass" for a series episode: full native
/// `Airing` JSON objects for every upcoming instance of the series, plus
/// any rule already covering it. The native schema is large and irregular
/// (nested `Raw.program.ratings[]` etc.) and is only ever round-tripped
/// back to the server unchanged here, never displayed field-by-field, so
/// it's carried as `serde_json::Value` rather than fully modeled.
pub async fn fetch_series_airings(
    server_url: &str,
    series_id: &str,
) -> Result<(Vec<serde_json::Value>, bool), String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/dvr/guide/airings/{series_id}");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "API error {}: GET /dvr/guide/airings/{series_id}",
            resp.status()
        ));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse series airings response: {e}"))?;

    let airings = body
        .get("airings")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let has_rule = body
        .get("rules")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    Ok((airings, has_rule))
}

/// Matches a clicked guide slot against a series' native airings list by
/// Channel+Time (the only identifiers guaranteed present and correct on
/// both sides) to find the exact object `create_job`/`create_pass` need.
pub fn find_matching_airing(
    airings: &[serde_json::Value],
    channel: &str,
    time: i64,
) -> Option<serde_json::Value> {
    airings
        .iter()
        .find(|a| {
            let t = a.get("Time").and_then(|v| v.as_i64());
            let ch = a.get("Channel").and_then(|v| v.as_str());
            let in_channels = a
                .get("Channels")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().any(|c| c.as_str() == Some(channel)))
                .unwrap_or(false);
            t == Some(time) && (ch == Some(channel) || in_channels)
        })
        .cloned()
}

/// Best-effort native-shaped `Airing` for content with no `series-id`
/// (movies/one-offs) — the API has no per-item lookup for these, so it's
/// built directly from the XMLTV fields already in hand. Channel/Time/
/// Duration (the fields that actually control the recording, at the job's
/// top level) are exact; the nested nice-to-have metadata is approximate.
pub fn build_fallback_airing(program: &GuideProgram) -> serde_json::Value {
    serde_json::json!({
        "Source": "tms",
        "Channel": program.channel,
        "Channels": [program.channel],
        "Time": program.start,
        "Duration": (program.stop - program.start).max(0),
        "Title": program.title,
        "Summary": program.desc,
        "FullSummary": program.desc,
        "Image": program.image,
        "Categories": program.categories,
        "Genres": program.categories,
        "ProgramID": program.program_id,
    })
}

/// `GET /dvr` root status blob's `padding: {start, end}` (seconds, as
/// strings) — used to seed the record dialog's padding fields with the
/// server's own configured defaults, matching what the admin UI itself
/// does, rather than a hardcoded guess.
pub async fn fetch_default_padding(server_url: &str) -> Result<(i64, i64), String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/dvr");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /dvr", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse /dvr response: {e}"))?;

    let get = |key: &str| {
        body.get("padding")
            .and_then(|p| p.get(key))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    };
    Ok((get("start"), get("end")))
}

/// `POST /dvr/jobs/new` — a one-time recording. Body shape confirmed
/// byte-for-byte from the admin bundle's own click handler:
/// `{Name, Time: airing.Time - padStart, Duration: airing.Duration + padStart + padEnd,
/// Channels: airing.Channels||[airing.Channel], Airing: airing}`. `time`/
/// `duration` passed in here are already padding-adjusted by the caller.
pub async fn create_job(
    server_url: &str,
    name: &str,
    time: i64,
    duration: i64,
    channels: &[String],
    airing: serde_json::Value,
) -> Result<(), String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/dvr/jobs/new");
    let body = serde_json::json!({
        "Name": name,
        "Time": time,
        "Duration": duration,
        "Channels": channels,
        "Airing": airing,
    });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: POST /dvr/jobs/new", resp.status()));
    }
    Ok(())
}

/// `POST /dvr/rules/new` — a recurring pass. Body shape confirmed from the
/// admin bundle's own new-rule-from-series-group initializer (the same
/// `EQ.SeriesID`/`Tags:"New"` shape it seeds when creating a pass from a
/// browse view).
pub async fn create_pass(
    server_url: &str,
    name: &str,
    image: Option<&str>,
    series_id: &str,
    new_only: bool,
    pad_start: i64,
    pad_end: i64,
) -> Result<(), String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/dvr/rules/new");

    let mut eq = serde_json::json!({ "SeriesID": series_id });
    if new_only {
        eq["Tags"] = serde_json::json!("New");
    }
    let body = serde_json::json!({
        "Name": name,
        "Image": image,
        "EQ": eq,
        "PaddingStart": pad_start,
        "PaddingEnd": pad_end,
    });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: POST /dvr/rules/new", resp.status()));
    }
    Ok(())
}
