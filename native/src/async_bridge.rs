//! egui does not repaint on background state change the way React does —
//! only on input or an explicit `request_repaint()`. Every async result
//! must be paired with a repaint or the UI silently freezes until the next
//! mouse move. `std::sync::mpsc` (not `tokio::sync::mpsc`) is deliberate:
//! the receiving side is `App::update()`, a plain synchronous function
//! polled once per frame via `try_recv()`, not an async context.

use std::sync::mpsc;

use tokio::runtime::Runtime;

pub enum Msg {
    ProbeResult {
        server_id: String,
        reachable: bool,
    },
    RecordingsLoaded(Result<Vec<crate::api::types::Recording>, String>),
    /// An in-progress recording's HLS variant URL, resolved before playback
    /// can actually start for it — see
    /// `api::recordings::resolve_hls_variant_url`'s doc comment for why a
    /// completed recording doesn't need this async round trip at all (it
    /// plays synchronously via the direct endpoint instead). The
    /// `Recording` is carried along so `App` can rebuild `NowPlaying` once
    /// the URL is known, without needing to hold or re-fetch it separately.
    HlsVariantResolved {
        recording: Box<crate::api::types::Recording>,
        result: Result<String, String>,
    },
    PlaybackPersisted {
        id: String,
        position: f64,
        /// Whether this save also requested marking the recording watched
        /// (mirrors what was actually sent, so `Ok` lets the caller patch
        /// its local cache without re-deriving the condition).
        watched: bool,
        result: Result<(), String>,
    },
    MoviesLoaded(Result<Vec<crate::api::types::Recording>, String>),
    VideoGroupsLoaded(Result<Vec<crate::api::types::VideoGroup>, String>),
    VideosLoaded {
        group_id: String,
        result: Result<Vec<crate::api::types::Video>, String>,
    },
    CollectionsLoaded(Result<Vec<crate::api::collections::LibraryCollection>, String>),
    CollectionShowsLoaded {
        collection_id: String,
        result: Result<Vec<crate::api::types::Show>, String>,
    },
    // Search loads its own independent copies of all four corpora up front
    // rather than sharing state with the screens that also fetch some of
    // this data (TV Shows/Movies/Library) — simpler than building a shared
    // cache layer, at the cost of one redundant fetch if a user visits both.
    SearchShowsLoaded(Result<Vec<crate::api::types::Show>, String>),
    SearchEpisodesLoaded(Result<Vec<crate::api::types::Recording>, String>),
    SearchMoviesLoaded(Result<Vec<crate::api::types::Recording>, String>),
    SearchVideosLoaded(Result<Vec<crate::api::types::Video>, String>),
    ChannelsLoaded(Result<Vec<crate::api::types::Channel>, String>),
    /// Separate from `ChannelsLoaded` (Live's own fetch) — Recent fetches
    /// its own copy purely to build a channel-logo lookup for list rows.
    RecentChannelsLoaded(Result<Vec<crate::api::types::Channel>, String>),
    ShowsLoaded(Result<Vec<crate::api::types::Show>, String>),
    EpisodesLoaded {
        show_id: String,
        result: Result<Vec<crate::api::types::Recording>, String>,
    },
    /// Optimistic-update-with-rollback pattern (matches the old app's
    /// `toggleWatched`): caller flips the UI immediately on click, this
    /// confirms or rolls back once the server actually responds.
    WatchedToggled {
        id: String,
        watched: bool,
        result: Result<(), String>,
    },
    Trashed {
        id: String,
        result: Result<(), String>,
    },
    GuideLoaded(Result<Vec<crate::api::guide::GuideProgram>, String>),
    GuideJobsLoaded(Result<Vec<crate::api::guide::Job>, String>),
    GuideRulesLoaded(Result<Vec<crate::api::guide::Rule>, String>),
    GuideRecordedLoaded(
        Result<std::collections::HashMap<String, crate::api::guide::ProgramStatus>, String>,
    ),
    GuideHistoryLoaded(Result<Vec<crate::api::guide::GuideProgram>, String>),
    HistoryServiceProbeResult(Result<(), String>),
    /// Delivered when a past guide slot's recorded-status hit resolves to a
    /// real `Recording` — the caller navigates to it and (if the series
    /// has a pass) shows the pass indicator.
    PastRecordingLoaded {
        result: Result<crate::api::types::Recording, String>,
        has_pass: bool,
    },
    /// Delivered when a currently-recording guide slot's "Play Recording"
    /// choice resolves to a real `Recording` — the caller starts playing
    /// it directly (unlike `PastRecordingLoaded`, which navigates to a
    /// detail screen instead).
    PlayFetchedRecording(Result<crate::api::types::Recording, String>),
    GuideDefaultPaddingLoaded(Result<(i64, i64), String>),
    /// The dialog's `channel`/`time` are carried along so a stale response
    /// (the user closed this program's dialog and opened a different one
    /// before the lookup finished) can be safely ignored instead of
    /// clobbering the now-current dialog's state.
    GuideSeriesAiringLoaded {
        channel: String,
        time: i64,
        result: Result<(Vec<serde_json::Value>, bool), String>,
    },
    GuideJobCreated(Result<(), String>),
    GuidePassCreated(Result<(), String>),
}

pub struct AsyncBridge {
    pub runtime: Runtime,
    pub tx: mpsc::Sender<Msg>,
    pub rx: mpsc::Receiver<Msg>,
}

impl AsyncBridge {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        let (tx, rx) = mpsc::channel();
        Self { runtime, tx, rx }
    }
}

/// Use this instead of a raw `tx.send(msg)` everywhere an async task
/// delivers a result, so the "must repaint after sending" discipline can't
/// be forgotten piecemeal.
pub fn send_and_repaint(tx: &mpsc::Sender<Msg>, ctx: &egui::Context, msg: Msg) {
    let _ = tx.send(msg);
    ctx.request_repaint();
}
