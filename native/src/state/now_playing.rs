//! Ephemeral now-playing state — mirrors `nowPlaying*` fields from the old
//! Zustand store exactly. Never persisted; lives only in `App` for the
//! duration of a playback session.
//!
//! Not wired into `App` yet — real playback (double-click a recording) is
//! Phase 5. Kept here now so the state shape is settled ahead of that.
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingKind {
    Episode,
    Movie,
}

#[derive(Debug, Clone)]
pub struct NowPlaying {
    pub id: String,
    pub title: String,
    pub file_path: Option<String>,
    /// Flat [start, end, start, end, ...] seconds, same shape as the old
    /// `nowPlayingCommercials: number[]`.
    pub commercials: Vec<f64>,
    pub manifest_url: Option<String>,
    pub resume_time: f64,
    /// `None` means live (no resume/watched-mutation support), matching the
    /// old store's `recordingKind: 'episode' | 'movie' | null`.
    pub recording_kind: Option<RecordingKind>,
}
