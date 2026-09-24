//! Serde equivalents of `api/types.ts`. Only the fields actually used so
//! far are declared — Channels DVR's real API returns more fields than any
//! given screen needs; add more as later screens require them, matching how
//! the old `Channel` type was already documented as "intentionally minimal."
//!
//! Several fields/methods here (season/episode numbers, image URLs,
//! recording_kind) aren't read by Phase 4's minimal RecentRecordings UI yet
//! but are needed by Phase 5 (playback) and later screens — kept now so the
//! deserialized shape is settled.
#![allow(dead_code)]

use serde::Deserialize;

/// The unified `/api/v1/all?source=recordings` feed type — covers both
/// finished and in-progress recordings (episodes and movies alike).
#[derive(Debug, Clone, Deserialize)]
pub struct Recording {
    pub id: String,
    #[serde(default)]
    pub show_id: Option<String>,
    /// Used by the old app's "Mark as Not Recorded" action
    /// (`DELETE /dvr/programs/{program_id}`) — not wired up yet, kept for
    /// when that lands.
    #[serde(default)]
    pub program_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub episode_title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub full_summary: Option<String>,
    #[serde(default)]
    pub season_number: Option<u32>,
    #[serde(default)]
    pub episode_number: Option<u32>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub playback_time: f64,
    #[serde(default)]
    pub commercials: Vec<f64>,
    #[serde(default)]
    pub watched: bool,
    #[serde(default)]
    pub favorited: bool,
    #[serde(default)]
    pub completed: bool,
    /// Milliseconds since epoch, matching the raw API's `created_at`.
    pub created_at: i64,

    // --- Detail-header-only fields (not needed by any list row, so left
    // off the struct until now) — confirmed present on the real API by the
    // old app's `types.ts` (`duration`/`favorited`/`delayed`/`cancelled`/
    // `corrupted` are all non-optional there), `#[serde(default)]`d anyway
    // per this file's usual defensive style.
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content_rating: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub delayed: bool,
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default)]
    pub corrupted: bool,

    // --- Sort-only fields, needed for the episode-list sort options.
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub original_air_date: Option<String>,

    /// Which channel this aired/recorded on — a channel number or id
    /// string (confirmed via `curl`, e.g. `"9218"` or `"11.3"`), used to
    /// look up that channel's logo for Recent's list rows. Not a foreign
    /// key into `Channel.id` specifically — the old app's own
    /// `logoForChannelKey` matches it against a channel's `id`, `name`,
    /// *or* `number` interchangeably, so this type doesn't try to be more
    /// precise than the API actually is.
    #[serde(default)]
    pub channel: Option<String>,
}

/// `/api/v1/shows` — note `name`, not `title` (matches the old app's own
/// observation that this asymmetry exists in the real API).
#[derive(Debug, Clone, Deserialize)]
pub struct Show {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub episode_count: u32,
    #[serde(default)]
    pub number_unwatched: u32,
    #[serde(default)]
    pub favorited: bool,
    /// Milliseconds since epoch; 0 if the show has no recordings yet.
    #[serde(default)]
    pub last_recorded_at: i64,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

/// `/api/v1/video_groups`.
#[derive(Debug, Clone, Deserialize)]
pub struct VideoGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub video_count: u32,
    #[serde(default)]
    pub number_unwatched: u32,
    #[serde(default)]
    pub favorited: bool,
    #[serde(default)]
    pub created_at: i64,
}

/// `/api/v1/video_groups/{id}/videos` — confirmed via curl against the real
/// server: `title` is actually the *group's* name here, not this video's
/// own title (`video_title` is). Genuinely different shape from
/// `Recording` (no summary, no commercials, no mutation endpoints exist for
/// these at all), so it gets its own type rather than being force-fit.
#[derive(Debug, Clone, Deserialize)]
pub struct Video {
    pub id: String,
    #[serde(default)]
    pub video_group_id: Option<String>,
    /// Confusingly the *group* name — see struct doc comment.
    pub title: String,
    #[serde(default)]
    pub video_title: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub playback_time: f64,
    #[serde(default)]
    pub watched: bool,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub created_at: i64,
}

/// `/api/v1/channels` — confirmed via curl that `favorited`/`hd`/`hidden`
/// are only present on *some* channels (absent = false via `#[serde(default)]`).
/// Deliberately not merging in the DVR guide's separate favorites set or
/// secondary logo map the old app also consulted — a known simplification
/// for this pass, not because the data doesn't matter. `encrypted` is the
/// one exception: it's merged in from that same guide endpoint by
/// `fetch_channels()` since it has no equivalent in this response at all
/// (see that function's doc comment).
#[derive(Debug, Clone, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub number: String,
    #[serde(default)]
    pub logo_url: Option<String>,
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub favorited: bool,
    #[serde(default)]
    pub hd: bool,
    #[serde(default)]
    pub hidden: bool,
    /// TMS station id, used as a fallback logo-URL source (via the
    /// server's `/tmsimg/assets/...` proxy) for the rare channel with no
    /// direct `logo_url` — matches the old app's `channelLogoUrl()`.
    #[serde(default)]
    pub station_id: Option<String>,
    /// Not part of `/api/v1/channels` — never populated by `serde`. Set
    /// after the fact by `fetch_channels()` from the separate
    /// `/dvr/guide/channels` admin endpoint, the only place the server
    /// exposes per-channel DRM status. HDHomeRun Prime (and other
    /// CableCARD) sources report plenty of channels as DRM-locked that the
    /// server still lists as tunable; those can never actually play, so
    /// they're treated the same as a hidden channel in the Live TV filter.
    #[serde(default, skip_deserializing)]
    pub encrypted: bool,
}

impl Recording {
    /// Same inference RecentRecordings.tsx used: `show_id` present means an
    /// episode, absent means a movie.
    pub fn recording_kind(&self) -> crate::state::now_playing::RecordingKind {
        use crate::state::now_playing::RecordingKind;
        if self.show_id.is_some() {
            RecordingKind::Episode
        } else {
            RecordingKind::Movie
        }
    }
}
