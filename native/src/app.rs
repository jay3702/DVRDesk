use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::api;
use crate::async_bridge::{self, AsyncBridge, Msg};
use crate::player::captions::{CaptionKind, CaptionMode, CaptionTrack};
use crate::player::Player;
use crate::screen::Screen;
use crate::state::now_playing::NowPlaying;
use crate::state::AppSettings;
use crate::theme;
use crate::ui;

/// How often the Recent screen re-fetches while it's open — see
/// `RecentState::last_fetched`'s doc comment for why this exists at all.
/// Short enough that a newly-finished recording shows up promptly, long
/// enough not to hammer the server every frame.
const RECENT_POLL_INTERVAL: Duration = Duration::from_secs(120);

pub struct App {
    settings: AppSettings,
    settings_ui: ui::settings::SettingsState,
    screen: Screen,
    bridge: AsyncBridge,
    player: Result<Player, String>,

    // --- Phase 2 spike carryover: manual manifest-URL playback test panel.
    // Superseded by real playback wiring (double-click a recording) in
    // Phase 5 — kept for now so player-level testing doesn't regress while
    // the shell is being built out.
    test_manifest_url: String,
    test_loaded_url: Option<String>,
    test_sid: u32,

    recent: ui::recent::RecentState,
    tv_shows: ui::tv_shows::TvShowsState,
    movies: ui::movies::MoviesState,
    library: ui::library::LibraryState,
    collections: ui::collections::CollectionsState,
    search: ui::search::SearchState,
    live: ui::live::LiveState,

    now_playing: Option<NowPlaying>,
    // Playback-persistence bookkeeping — direct port of VideoPlayer.tsx's
    // hasAppliedResumeRef/hasMarkedWatchedRef/saveInFlightRef/
    // lastSavedPlaybackRef guards. save_in_flight prevents overlapping PUT
    // requests the same way the old code's ref did.
    has_applied_resume: bool,
    has_marked_watched: bool,
    save_in_flight: bool,
    last_saved_position: f64,

    // Caption state for the current playback session — mirrors
    // VideoPlayer.tsx's local hasSrt/hasBroadcast/captionMode useState, not
    // part of NowPlaying since the old store never tracked this either.
    caption_tracks: Vec<CaptionTrack>,
    caption_mode: CaptionMode,
    applied_caption_sid: Option<u32>,

    // Commercial auto-skip — port of VideoPlayer.tsx's skipAds toggle and
    // adBlocks logic. disabled_ad_blocks is reserved for the manual-seek
    // override tracking the old app has; not populated yet since there's no
    // seek/scrub UI in the overlay for a user to trigger it with.
    skip_ads: bool,
    disabled_ad_blocks: HashSet<usize>,
    skip_toast_until: Option<Instant>,

    /// Last time the player overlay saw pointer activity (movement, click,
    /// or press) — reset whenever a new playback session starts. Drives
    /// auto-hiding the live-channel control chrome after a few seconds of
    /// inactivity so the video can fill the frame; recordings keep their
    /// controls up regardless (see `player_overlay::show`'s own gating).
    player_controls_active_since: Instant,

    // Diagnostics — gated on settings.diagnostics_enabled, matching the
    // old app's Shift+S stats overlay / Copy Report button.
    show_stats: bool,

    /// Set when navigating to a recording from a past guide slot whose
    /// series has an active pass — shown as a small dismissible banner
    /// above the detail screen. Informational only; this app has no
    /// pass-management UI to link to.
    pending_pass_notice: Option<String>,

    /// `Some` once a genuinely newer `native-v*` release is found — see
    /// `api/github.rs`'s doc comment for why this is scoped to that tag
    /// prefix specifically rather than trusting GitHub's own "latest"
    /// notion. Checked once, fired from `App::new()`, not gated behind any
    /// particular screen since it's shown globally like `pending_pass_notice`.
    update_info: Option<crate::api::github::UpdateInfo>,
    update_dismissed: bool,

    downloads: crate::downloads::Downloads,
    deploys: crate::deploy::Deploys,
    channel_genres: crate::channel_genres::ChannelGenres,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

        // A real palette (ported from the old Tauri/React app's own
        // `themes.css`) in place of egui's stock gray/blue defaults — set
        // once here, not every frame; `ctx.set_theme()` (called per-frame
        // in `update()`) only *selects* between these two, it doesn't reset
        // them.
        cc.egui_ctx
            .style_mut_of(egui::Theme::Dark, |s| s.visuals = theme::dark_visuals());
        cc.egui_ctx
            .style_mut_of(egui::Theme::Light, |s| s.visuals = theme::light_visuals());

        let settings = AppSettings::load();
        let settings_ui = ui::settings::SettingsState::new(&settings);

        let player = match cc.get_proc_address {
            Some(get_proc_address) => Player::new(get_proc_address),
            None => Err(
                "cc.get_proc_address is None — eframe is not using the Glow renderer".to_string(),
            ),
        };

        let bridge = AsyncBridge::new();
        let downloads = crate::downloads::Downloads::new(
            bridge.runtime.handle().clone(),
            settings.download_path(),
            cc.egui_ctx.clone(),
        );
        let deploys =
            crate::deploy::Deploys::new(bridge.runtime.handle().clone(), cc.egui_ctx.clone());
        let channel_genres = crate::channel_genres::ChannelGenres::load(
            crate::paths::data_dir().map(|d| d.join("channel_genres.json")),
        );

        // Fired once, unconditionally — not gated behind any screen, same
        // reasoning as the old Tauri app's own equivalent `useEffect`
        // (runs once on mount regardless of which route is active).
        {
            let tx = bridge.tx.clone();
            let ctx = cc.egui_ctx.clone();
            bridge.runtime.spawn(async move {
                let result = api::github::fetch_update_info().await;
                async_bridge::send_and_repaint(&tx, &ctx, Msg::UpdateCheckResult(result));
            });
        }

        Self {
            settings,
            settings_ui,
            screen: Screen::default(),
            bridge,
            player,
            test_manifest_url: String::new(),
            test_loaded_url: None,
            test_sid: 1,

            recent: ui::recent::RecentState::default(),
            tv_shows: ui::tv_shows::TvShowsState::default(),
            movies: ui::movies::MoviesState::default(),
            library: ui::library::LibraryState::default(),
            collections: ui::collections::CollectionsState::default(),
            search: ui::search::SearchState::default(),
            live: ui::live::LiveState::default(),

            now_playing: None,
            has_applied_resume: false,
            has_marked_watched: false,
            save_in_flight: false,
            last_saved_position: -1.0,

            caption_tracks: Vec::new(),
            caption_mode: CaptionMode::Off,
            applied_caption_sid: None,

            skip_ads: true,
            disabled_ad_blocks: HashSet::new(),
            skip_toast_until: None,

            player_controls_active_since: Instant::now(),

            show_stats: false,

            pending_pass_notice: None,
            update_info: None,
            update_dismissed: false,

            downloads,
            deploys,
            channel_genres,
        }
    }

    /// Starts playback of a recording. A *completed* one plays immediately,
    /// synchronously, via the direct `stream.mpg` endpoint (confirmed in
    /// the Phase 2 spike to start fastest and be the only path carrying
    /// caption tracks). A still-recording one needs its HLS variant URL
    /// resolved first — see `api::recordings::resolve_hls_variant_url`'s
    /// doc comment for the root cause and why that can't be a plain
    /// synchronous URL build the way the completed case is — so that
    /// branch dispatches an async fetch instead and returns immediately;
    /// playback actually starts once `Msg::HlsVariantResolved` lands.
    ///
    /// Takes the `Recording` directly rather than an id to look up in one
    /// particular screen's cache — every screen that can play something
    /// (Recent, TV Shows, Movies) has its own list already in hand and
    /// just calls this with the item from it.
    fn start_playback(&mut self, ctx: &egui::Context, rec: &api::types::Recording) {
        let Some(server_url) = self.active_server_url() else {
            return;
        };

        if rec.completed {
            let network_url = api::recordings::direct_play_url(&server_url, &rec.id);
            // Play from the local copy if one's finished downloading —
            // no server round trip at all. `manifest_url` still carries
            // the real network URL regardless (the diagnostics/stats
            // panel only ever reads that field, not `file_path`).
            let local = self.downloads.local_path(&rec.id);
            let play_url = local
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| network_url.clone());
            self.start_now_playing(
                &play_url,
                NowPlaying {
                    id: rec.id.clone(),
                    title: rec.title.clone(),
                    file_path: local.map(|p| p.to_string_lossy().into_owned()),
                    commercials: rec.commercials.clone(),
                    manifest_url: Some(network_url),
                    resume_time: rec.playback_time,
                    recording_kind: Some(rec.recording_kind()),
                },
            );
            return;
        }

        let tx = self.bridge.tx.clone();
        let ctx2 = ctx.clone();
        let recording = Box::new(rec.clone());
        self.bridge.runtime.spawn(async move {
            let result = api::recordings::resolve_hls_variant_url(&server_url, &recording.id).await;
            async_bridge::send_and_repaint(
                &tx,
                &ctx2,
                Msg::HlsVariantResolved { recording, result },
            );
        });
    }

    /// Same as `start_playback` but for library videos — a genuinely
    /// different type (`api::types::Video`, not `Recording`; see its doc
    /// comment for why), always played with `recording_kind: None` since
    /// library videos have no playback-time/watched mutation endpoints at
    /// all (matches the old app's `playItem(..., null)` for these).
    fn start_playback_video(&mut self, video: &api::types::Video) {
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        let url = api::recordings::direct_play_url(&server_url, &video.id);

        self.start_now_playing(
            &url,
            NowPlaying {
                id: video.id.clone(),
                title: video
                    .video_title
                    .clone()
                    .unwrap_or_else(|| video.title.clone()),
                file_path: None,
                commercials: Vec::new(),
                manifest_url: Some(url.clone()),
                resume_time: video.playback_time,
                recording_kind: None,
            },
        );
    }

    /// Live channels: always HLS (no direct-file equivalent exists — see
    /// `api::live`'s doc comment), always `recording_kind: None` (no
    /// resume/watched/persistence, matching the old app exactly).
    fn start_playback_channel(&mut self, channel: &api::types::Channel) {
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        let url = api::live::live_manifest_url(&server_url, channel);

        self.start_now_playing(
            &url,
            NowPlaying {
                id: channel.id.clone(),
                title: format!("{}  {}", channel.number, channel.name),
                file_path: None,
                commercials: Vec::new(),
                manifest_url: Some(url.clone()),
                resume_time: 0.0,
                recording_kind: None,
            },
        );
    }

    /// Shared reset of every per-session playback guard — factored out once
    /// a second caller (library videos) needed the identical bookkeeping.
    fn start_now_playing(&mut self, url: &str, now_playing: NowPlaying) {
        if let Ok(player) = &self.player {
            player.load_url(url);
        }

        self.now_playing = Some(now_playing);
        self.has_applied_resume = false;
        self.has_marked_watched = false;
        self.save_in_flight = false;
        self.last_saved_position = -1.0;

        self.caption_tracks = Vec::new();
        self.caption_mode = CaptionMode::Off;
        self.applied_caption_sid = None;

        self.skip_ads = true;
        self.disabled_ad_blocks = HashSet::new();
        self.skip_toast_until = None;

        // Controls start visible on every new tune/play, then auto-hide
        // (live only) after a few seconds of inactivity — not hidden
        // instantly on the very frame playback starts.
        self.player_controls_active_since = Instant::now();
    }

    /// Refreshes the known caption tracks and applies `caption_mode` if the
    /// desired track differs from what's currently selected in mpv. Cheap
    /// to call every frame — see `player::captions` for why that (rather
    /// than trying to catch one precise "file loaded" event) is what
    /// actually fixes the sid-timing race found during manual testing.
    fn tick_captions(&mut self) {
        let Ok(player) = &self.player else {
            return;
        };
        self.caption_tracks = player.caption_tracks();

        let desired_sid = match self.caption_mode {
            CaptionMode::Off => Some(0),
            CaptionMode::PyCaptions => {
                crate::player::captions::find_by_kind(&self.caption_tracks, CaptionKind::PyCaptions)
            }
            CaptionMode::Broadcast => {
                crate::player::captions::find_by_kind(&self.caption_tracks, CaptionKind::Broadcast)
            }
        };

        if let Some(sid) = desired_sid {
            if self.applied_caption_sid != Some(sid) {
                player.set_sid(sid);
                self.applied_caption_sid = Some(sid);
            }
        }
    }

    /// Auto-skips past commercial blocks — port of VideoPlayer.tsx's
    /// onTimeUpdate ad-skip check. Manual-override tracking (re-enabling
    /// auto-skip after seeking well before a block, disabling it after
    /// seeking back into one) isn't ported yet — no seek/scrub UI exists
    /// for a user to trigger that distinction with.
    fn tick_commercial_skip(&mut self) {
        if !self.skip_ads {
            return;
        }
        let Some(now_playing) = &self.now_playing else {
            return;
        };
        let Ok(player) = &self.player else {
            return;
        };

        let pos = player.position_secs().unwrap_or(0.0);
        let blocks = crate::player::commercial_skip::ad_blocks(&now_playing.commercials);
        if let Some((_, seek_to)) =
            crate::player::commercial_skip::block_to_skip(&blocks, &self.disabled_ad_blocks, pos)
        {
            player.seek(seek_to);
            self.skip_toast_until = Some(Instant::now() + Duration::from_millis(1500));
        }
    }

    /// Resume-on-open, mark-watched-at-90%, and debounced/coalesced
    /// position persistence — ported from VideoPlayer.tsx's onTimeUpdate
    /// handler and persistPlaybackUpdate/queuePlaybackUpdate. Called every
    /// frame while a recording is playing; the guards above (not raw
    /// per-frame firing) are what keep this cheap and non-overlapping.
    fn tick_playback_persistence(&mut self, ctx: &egui::Context) {
        let Some(now_playing) = &self.now_playing else {
            return;
        };
        // Live has no recording_kind and no playback_time/watched mutation
        // support — matches the old app's canSyncPlayback() guard exactly.
        if now_playing.recording_kind.is_none() {
            return;
        }
        let Ok(player) = &self.player else {
            return;
        };

        let pos = player.position_secs().unwrap_or(0.0);
        let dur = player.duration_secs().unwrap_or(0.0);

        if !self.has_applied_resume
            && now_playing.resume_time > 5.0
            && dur.is_finite()
            && dur > 0.0
            && now_playing.resume_time < (dur - 10.0).max(10.0)
        {
            player.seek(now_playing.resume_time);
            self.has_applied_resume = true;
        }

        let mut mark_watched_now = false;
        let mut force_save = false;
        if !self.has_marked_watched && dur > 0.0 && pos / dur >= 0.9 {
            self.has_marked_watched = true;
            mark_watched_now = true;
            force_save = true;
        }

        let delta = (pos - self.last_saved_position).abs();
        if !self.save_in_flight && (force_save || delta >= 20.0) {
            let Some(server_url) = self.active_server_url() else {
                return;
            };
            self.save_in_flight = true;
            let id = now_playing.id.clone();
            let id_for_msg = id.clone();
            let tx = self.bridge.tx.clone();
            let ctx = ctx.clone();
            self.bridge.runtime.spawn(async move {
                let result = api::recordings::persist_playback_update(
                    &server_url,
                    &id,
                    pos,
                    mark_watched_now,
                )
                .await;
                async_bridge::send_and_repaint(
                    &tx,
                    &ctx,
                    Msg::PlaybackPersisted {
                        id: id_for_msg,
                        position: pos,
                        watched: mark_watched_now,
                        result,
                    },
                );
            });
        }
    }

    /// Fires a final best-effort save on close, matching the old cleanup
    /// effect's `queuePlaybackUpdate(video.currentTime, true)`. Routed
    /// through the same Msg path as the periodic saves (not just fired and
    /// forgotten) so the local recordings cache gets patched with this
    /// final position too — otherwise the last few seconds between the last
    /// periodic save and clicking Close wouldn't be reflected if the same
    /// recording is reopened later in this session.
    fn stop_playback(&mut self, ctx: &egui::Context) {
        if let (Some(now_playing), Ok(player)) = (&self.now_playing, &self.player) {
            if now_playing.recording_kind.is_some() {
                if let Some(server_url) = self.active_server_url() {
                    let pos = player.position_secs().unwrap_or(0.0);
                    let id = now_playing.id.clone();
                    let tx = self.bridge.tx.clone();
                    let ctx = ctx.clone();
                    self.bridge.runtime.spawn(async move {
                        let result =
                            api::recordings::persist_playback_update(&server_url, &id, pos, false)
                                .await;
                        async_bridge::send_and_repaint(
                            &tx,
                            &ctx,
                            Msg::PlaybackPersisted {
                                id,
                                position: pos,
                                watched: false,
                                result,
                            },
                        );
                    });
                }
            }
            player.stop();
        }
        self.now_playing = None;
    }

    fn drain_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.bridge.rx.try_recv() {
            match msg {
                Msg::UpdateCheckResult(result) => {
                    if let Ok(Some(info)) = result {
                        self.update_info = Some(info);
                    }
                    // A failed/empty check is silently ignored, same as
                    // the old app's own update-checker — nothing useful to
                    // show the user either way, and this isn't worth an
                    // error banner of its own.
                }
                Msg::ProbeResult {
                    server_id,
                    reachable,
                } => {
                    self.settings_ui.probe_completed(&server_id, reachable);
                }
                Msg::RecordingsLoaded(result) => {
                    self.recent.items = match result {
                        Ok(recordings) => ui::Loaded::Ready(recordings),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::RecentChannelsLoaded(result) => {
                    // Non-fatal either way — the row-rendering code just
                    // treats anything but `Loaded::Ready` as "no logos
                    // available yet," same as a channel simply not being
                    // in the map. `Err` (not retried, unlike `Idle`) so a
                    // failed fetch doesn't loop forever re-requesting it.
                    self.recent.channels = match result {
                        Ok(channels) => ui::Loaded::Ready(channels),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::PlaybackPersisted {
                    id,
                    position,
                    watched,
                    result,
                } => {
                    self.save_in_flight = false;
                    match result {
                        Ok(()) => {
                            self.last_saved_position = position;
                            // Patch every in-memory cache that might hold
                            // this recording — the server has the new
                            // position now, but without this, reopening the
                            // same recording later in this session (from
                            // any screen, not just the one played from)
                            // would still read the stale playback_time from
                            // its initial fetch and resume from 0 instead of
                            // where the save left off (confirmed as an
                            // actual bug, not just theoretical).
                            patch_playback(&mut self.recent.items, &id, position, watched);
                            patch_playback(&mut self.tv_shows.episodes, &id, position, watched);
                            patch_playback(&mut self.movies.items, &id, position, watched);
                        }
                        Err(e) => eprintln!("playback persistence: {e}"),
                    }
                }
                Msg::MoviesLoaded(result) => {
                    self.movies.items = match result {
                        Ok(movies) => ui::Loaded::Ready(movies),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::VideoGroupsLoaded(result) => {
                    self.library.groups = match result {
                        Ok(groups) => ui::Loaded::Ready(groups),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::VideosLoaded { group_id, result } => {
                    if self.library.videos_group_id.as_deref() == Some(group_id.as_str()) {
                        self.library.videos = match result {
                            Ok(videos) => ui::Loaded::Ready(videos),
                            Err(e) => ui::Loaded::Err(e),
                        };
                    }
                }
                Msg::SearchShowsLoaded(result) => {
                    self.search.shows = match result {
                        Ok(v) => ui::Loaded::Ready(v),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::SearchEpisodesLoaded(result) => {
                    self.search.episodes = match result {
                        Ok(v) => ui::Loaded::Ready(v),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::SearchMoviesLoaded(result) => {
                    self.search.movies = match result {
                        Ok(v) => ui::Loaded::Ready(v),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::SearchVideosLoaded(result) => {
                    self.search.videos = match result {
                        Ok(v) => ui::Loaded::Ready(v),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::ChannelsLoaded { server_id, result } => {
                    if self.settings.active_server_id.as_deref() == Some(server_id.as_str()) {
                        self.live.channels = match result {
                            Ok(v) => ui::Loaded::Ready(v),
                            Err(e) => ui::Loaded::Err(e),
                        };
                        self.seed_channel_genres_if_ready();
                    }
                }
                Msg::ShowsLoaded(result) => {
                    self.tv_shows.shows = match result {
                        Ok(shows) => ui::Loaded::Ready(shows),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::EpisodesLoaded { show_id, result } => {
                    // Only apply if this is still the show the user has
                    // selected — a stale in-flight fetch from a show they've
                    // since navigated away from shouldn't clobber newer data.
                    if self.tv_shows.episodes_show_id.as_deref() == Some(show_id.as_str()) {
                        self.tv_shows.episodes = match result {
                            Ok(episodes) => ui::Loaded::Ready(episodes),
                            Err(e) => ui::Loaded::Err(e),
                        };
                    }
                }
                Msg::WatchedToggled {
                    id,
                    watched,
                    result,
                } => {
                    if let Err(e) = result {
                        eprintln!("watched-toggle: {e} — rolling back optimistic update");
                        // Roll back the optimistic flip applied at click time.
                        patch_watched(&mut self.tv_shows.episodes, &id, !watched);
                        patch_watched(&mut self.recent.items, &id, !watched);
                        patch_watched(&mut self.movies.items, &id, !watched);
                    }
                }
                Msg::Trashed { id, result } => {
                    if let Err(e) = result {
                        eprintln!("trash: {e}");
                    } else {
                        // Grab the trashed episode's show/watched status
                        // before removing it — needed to patch the TV Shows
                        // left-panel show list (episode-count badge, and
                        // removing the show entirely once its last episode
                        // is gone), which otherwise stays stale forever
                        // since `tv_shows.shows` is fetched once and cached.
                        let trashed_show = find_recording(&self.tv_shows.episodes, &id)
                            .map(|r| (r.show_id.clone(), r.watched));

                        remove_recording(&mut self.tv_shows.episodes, &id);
                        remove_recording(&mut self.recent.items, &id);
                        remove_recording(&mut self.movies.items, &id);

                        if let Some((Some(show_id), was_watched)) = trashed_show {
                            let show_removed = patch_show_episode_removed(
                                &mut self.tv_shows.shows,
                                &show_id,
                                was_watched,
                            );
                            if let Screen::TvShows {
                                show_id: sel_show,
                                episode_id: sel_ep,
                                ..
                            } = &mut self.screen
                            {
                                if sel_ep.as_deref() == Some(id.as_str()) {
                                    *sel_ep = None;
                                }
                                if show_removed && sel_show.as_deref() == Some(show_id.as_str()) {
                                    *sel_show = None;
                                    *sel_ep = None;
                                }
                            }
                        }
                    }
                }
                Msg::GuideLoaded { server_id, result } => {
                    if self.settings.active_server_id.as_deref() == Some(server_id.as_str()) {
                        self.live.guide = match result {
                            Ok(v) => ui::Loaded::Ready(v),
                            Err(e) => ui::Loaded::Err(e),
                        };
                        self.seed_channel_genres_if_ready();
                    }
                }
                Msg::GuideJobsLoaded { server_id, result } => {
                    if self.settings.active_server_id.as_deref() == Some(server_id.as_str()) {
                        self.live.jobs = match result {
                            Ok(v) => ui::Loaded::Ready(v),
                            Err(e) => ui::Loaded::Err(e),
                        };
                    }
                }
                Msg::GuideRulesLoaded { server_id, result } => {
                    if self.settings.active_server_id.as_deref() == Some(server_id.as_str()) {
                        self.live.rules = match result {
                            Ok(v) => ui::Loaded::Ready(v),
                            Err(e) => ui::Loaded::Err(e),
                        };
                    }
                }
                Msg::CollectionsLoaded(result) => {
                    self.collections.collections = match result {
                        Ok(v) => ui::Loaded::Ready(v),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::CollectionShowsLoaded {
                    collection_id,
                    result,
                } => {
                    if self.collections.shows_collection_id.as_deref()
                        == Some(collection_id.as_str())
                    {
                        self.collections.shows = match result {
                            Ok(v) => ui::Loaded::Ready(v),
                            Err(e) => ui::Loaded::Err(e),
                        };
                    }
                }
                Msg::GuideRecordedLoaded { server_id, result } => {
                    if self.settings.active_server_id.as_deref() == Some(server_id.as_str()) {
                        self.live.recorded = match result {
                            Ok(v) => ui::Loaded::Ready(v),
                            Err(e) => ui::Loaded::Err(e),
                        };
                    }
                }
                Msg::GuideDefaultPaddingLoaded { server_id, result } => {
                    // Best-effort — a failure here just means the dialog
                    // falls back to its own hardcoded (0, 60) default
                    // rather than the server's configured one.
                    if self.settings.active_server_id.as_deref() == Some(server_id.as_str()) {
                        if let Ok(padding) = result {
                            self.live.default_padding = Some(padding);
                        }
                    }
                }
                Msg::GuideSeriesAiringLoaded {
                    channel,
                    time,
                    result,
                } => {
                    // Ignore a stale response for a dialog the user has
                    // since closed or replaced with a different program's.
                    let matches_open_dialog = self
                        .live
                        .dialog
                        .as_ref()
                        .map(|d| d.program.channel == channel && d.program.start == time)
                        .unwrap_or(false);
                    if !matches_open_dialog {
                        continue;
                    }
                    let Some(dialog) = self.live.dialog.as_mut() else {
                        continue;
                    };
                    match result {
                        Ok((airings, has_rule)) => {
                            dialog.existing_pass = has_rule;
                            match api::guide::find_matching_airing(&airings, &channel, time) {
                                Some(airing) => dialog.airing = ui::Loaded::Ready(airing),
                                None => {
                                    dialog.airing = ui::Loaded::Ready(
                                        api::guide::build_fallback_airing(&dialog.program),
                                    );
                                }
                            }
                        }
                        Err(e) => dialog.airing = ui::Loaded::Err(e),
                    }
                }
                Msg::GuideJobCreated(result) => {
                    if let Some(dialog) = self.live.dialog.as_mut() {
                        dialog.busy = false;
                        dialog.message = Some(match &result {
                            Ok(()) => Ok("Recording scheduled.".to_string()),
                            Err(e) => Err(e.clone()),
                        });
                    }
                    if result.is_ok() {
                        self.live.jobs = ui::Loaded::Idle;
                        self.live.recorded = ui::Loaded::Idle;
                    }
                }
                Msg::GuidePassCreated(result) => {
                    if let Some(dialog) = self.live.dialog.as_mut() {
                        dialog.busy = false;
                        dialog.message = Some(match &result {
                            Ok(()) => Ok("Pass created.".to_string()),
                            Err(e) => Err(e.clone()),
                        });
                        if result.is_ok() {
                            dialog.existing_pass = true;
                        }
                    }
                    if result.is_ok() {
                        self.live.rules = ui::Loaded::Idle;
                    }
                }
                Msg::GuideHistoryLoaded(result) => {
                    self.live.history = match result {
                        Ok(v) => ui::Loaded::Ready(v),
                        Err(e) => ui::Loaded::Err(e),
                    };
                }
                Msg::HistoryServiceProbeResult(result) => {
                    self.settings_ui.history_probe_completed(result);
                }
                Msg::PastRecordingLoaded { result, has_pass } => match result {
                    Ok(rec) => {
                        use crate::state::now_playing::RecordingKind;
                        match rec.recording_kind() {
                            RecordingKind::Episode => {
                                self.screen = Screen::TvShows {
                                    show_id: rec.show_id.clone(),
                                    episode_id: Some(rec.id.clone()),
                                    filter: None,
                                };
                            }
                            RecordingKind::Movie => {
                                self.screen = Screen::Movies {
                                    movie_id: Some(rec.id.clone()),
                                };
                            }
                        }
                        self.pending_pass_notice = if has_pass {
                            Some("This series has an active pass.".to_string())
                        } else {
                            None
                        };
                    }
                    Err(e) => eprintln!("past recording lookup: {e}"),
                },
                Msg::PlayFetchedRecording(result) => match result {
                    Ok(rec) => self.start_playback(ctx, &rec),
                    Err(e) => eprintln!("in-progress recording lookup: {e}"),
                },
                Msg::HlsVariantResolved { recording, result } => match result {
                    Ok(url) => {
                        self.start_now_playing(
                            &url.clone(),
                            NowPlaying {
                                id: recording.id.clone(),
                                title: recording.title.clone(),
                                file_path: None,
                                commercials: recording.commercials.clone(),
                                manifest_url: Some(url),
                                resume_time: recording.playback_time,
                                recording_kind: Some(recording.recording_kind()),
                            },
                        );
                    }
                    Err(e) => eprintln!(
                        "HLS variant resolution for in-progress recording {}: {e}",
                        recording.id
                    ),
                },
            }
        }
    }

    fn active_server_url(&self) -> Option<String> {
        let active_id = self.settings.active_server_id.as_deref()?;
        self.settings
            .servers
            .iter()
            .find(|s| s.id == active_id)
            .map(|s| s.url.clone())
    }

    /// Switching servers from the sidebar dropdown needs to actually
    /// switch — every screen's cached data belongs to whichever server was
    /// active when it was fetched, and each screen's `ensure_*_loaded`
    /// only fetches once (`Loaded::Idle` is its trigger), so without this
    /// picking a different server would silently keep showing the
    /// previous one's data until the app restarted. Resets every screen's
    /// fetch state back to `Idle` so the currently-visible one refetches
    /// immediately and the rest pick up the new server next time they're
    /// visited — and stops any active playback first, since it would
    /// otherwise keep streaming from a server that's no longer selected.
    ///
    /// Resetting to `Idle` alone isn't quite enough with more than one
    /// server configured, though: switching to server B re-fetches Live's
    /// channels/guide, but if the user switches back to server A *before*
    /// B's fetch lands, B's response arrives after A is active again — and
    /// with nothing tying that response to the server it was requested
    /// for, it would silently overwrite A's live grid with B's channels
    /// (confirmed as the cause of a real report: adding a second server
    /// and clicking around it made the *first* server's live grid look
    /// wrong). `Msg::ChannelsLoaded`/`GuideLoaded`/etc. carry the
    /// `server_id` they were fetched for and their handlers drop the
    /// result if it no longer matches `active_server_id`, the same guard
    /// `EpisodesLoaded` already used for the equivalent per-show race.
    fn switch_active_server(&mut self, ctx: &egui::Context, new_id: String) {
        if self.settings.active_server_id.as_deref() == Some(new_id.as_str()) {
            return;
        }
        self.stop_playback(ctx);
        self.settings.active_server_id = Some(new_id);
        if let Err(e) = self.settings.save() {
            eprintln!("settings: failed to save: {e}");
        }

        self.recent.items = ui::Loaded::Idle;
        self.recent.channels = ui::Loaded::Idle;
        self.tv_shows.shows = ui::Loaded::Idle;
        self.tv_shows.episodes = ui::Loaded::Idle;
        self.tv_shows.episodes_show_id = None;
        self.movies.items = ui::Loaded::Idle;
        self.library.groups = ui::Loaded::Idle;
        self.library.videos = ui::Loaded::Idle;
        self.library.videos_group_id = None;
        self.search.shows = ui::Loaded::Idle;
        self.search.episodes = ui::Loaded::Idle;
        self.search.movies = ui::Loaded::Idle;
        self.search.videos = ui::Loaded::Idle;
        self.live.channels = ui::Loaded::Idle;
        self.live.guide = ui::Loaded::Idle;
        self.live.jobs = ui::Loaded::Idle;
        self.live.rules = ui::Loaded::Idle;
        self.live.recorded = ui::Loaded::Idle;
        self.live.history = ui::Loaded::Idle;
        self.live.default_padding = None;
        self.live.dialog = None;
        self.live.play_choice = None;
        self.live.scrolled_to_now = false;
    }

    /// Lazily kicks off the recordings fetch the first time the Recent
    /// screen actually needs data — the Rust analogue of the old page's
    /// `useEffect`-on-mount fetch, since there's no mount lifecycle here.
    fn ensure_recent_loaded(&mut self, ctx: &egui::Context) {
        // Refetches every `RECENT_POLL_INTERVAL` while this screen is open,
        // not just once — see `RecentState::last_fetched`'s doc comment for
        // why a one-shot `Loaded::Idle` gate (every other screen's pattern)
        // isn't enough here. A `Loading`/`Err` state is left alone rather
        // than re-triggered on a timer — a fetch already in flight or one
        // that just failed shouldn't get a second one stacked on top.
        let due = match self.recent.items {
            ui::Loaded::Idle => true,
            ui::Loaded::Ready(_) | ui::Loaded::Err(_) => self
                .recent
                .last_fetched
                .map_or(true, |t| t.elapsed() >= RECENT_POLL_INTERVAL),
            ui::Loaded::Loading => false,
        };
        if due {
            if let Some(server_url) = self.active_server_url() {
                // Only shows a loading spinner on the very first fetch — a
                // background refresh should be invisible unless/until it
                // actually changes something, not flash the list away.
                if matches!(self.recent.items, ui::Loaded::Idle) {
                    self.recent.items = ui::Loaded::Loading;
                }
                self.recent.last_fetched = Some(Instant::now());
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result = api::recordings::fetch_recordings(&server_url).await;
                    async_bridge::send_and_repaint(&tx, &ctx2, Msg::RecordingsLoaded(result));
                });
            }
        }
        // Purely for the channel-logo lookup on list rows — independent
        // of (and not blocking) the recordings fetch above.
        if matches!(self.recent.channels, ui::Loaded::Idle) {
            if let Some(server_url) = self.active_server_url() {
                self.recent.channels = ui::Loaded::Loading;
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let mut result = api::live::fetch_channels(&server_url).await;
                    if let Ok(channels) = &mut result {
                        api::live::resolve_station_logo_variants(channels, &server_url).await;
                    }
                    async_bridge::send_and_repaint(&tx, &ctx2, Msg::RecentChannelsLoaded(result));
                });
            }
        }
    }

    fn ensure_movies_loaded(&mut self, ctx: &egui::Context) {
        if !matches!(self.movies.items, ui::Loaded::Idle) {
            return;
        }
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        self.movies.items = ui::Loaded::Loading;
        let tx = self.bridge.tx.clone();
        let ctx = ctx.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::recordings::fetch_movies(&server_url).await;
            async_bridge::send_and_repaint(&tx, &ctx, Msg::MoviesLoaded(result));
        });
    }

    /// Seeds any channel that doesn't have a genre assigned yet — see
    /// `channel_genres.rs`'s doc comment for why this only ever fills in
    /// *empty* slots. Needs both channels and the guide loaded, and the two
    /// fetches race independently (channels' own fetch also resolves
    /// logo-variant fallbacks, real extra latency that means the guide
    /// reliably finishes *first* in practice) — called from both
    /// `Msg::ChannelsLoaded` and `Msg::GuideLoaded`'s handlers so whichever
    /// one actually completes second is the one that does the seeding.
    fn seed_channel_genres_if_ready(&mut self) {
        let (Some(server_id), ui::Loaded::Ready(channels), ui::Loaded::Ready(guide)) = (
            self.settings.active_server_id.clone(),
            &self.live.channels,
            &self.live.guide,
        ) else {
            return;
        };
        let now = chrono::Utc::now().timestamp();
        let changed = self
            .channel_genres
            .seed_missing(&server_id, channels, guide, now);
        if changed {
            if let Err(e) = self.channel_genres.save() {
                crate::logline!("channel_genres: failed to save: {e}");
            }
        }
    }

    fn ensure_live_loaded(&mut self, ctx: &egui::Context) {
        let active_server_id = self.settings.active_server_id.clone();
        if matches!(self.live.channels, ui::Loaded::Idle) {
            if let (Some(server_id), Some(server_url)) =
                (active_server_id.clone(), self.active_server_url())
            {
                self.live.channels = ui::Loaded::Loading;
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let mut result = api::live::fetch_channels(&server_url).await;
                    if let Ok(channels) = &mut result {
                        api::live::resolve_station_logo_variants(channels, &server_url).await;
                    }
                    async_bridge::send_and_repaint(
                        &tx,
                        &ctx2,
                        Msg::ChannelsLoaded { server_id, result },
                    );
                });
            }
        }
        if matches!(self.live.guide, ui::Loaded::Idle) {
            if let (Some(server_id), Some(server_url)) =
                (active_server_id.clone(), self.active_server_url())
            {
                self.live.guide = ui::Loaded::Loading;
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result =
                        api::guide::fetch_guide(&server_url, ui::live::GUIDE_DURATION_SECS).await;
                    async_bridge::send_and_repaint(&tx, &ctx2, Msg::GuideLoaded { server_id, result });
                });
            }
        }
        if matches!(self.live.jobs, ui::Loaded::Idle) {
            if let (Some(server_id), Some(server_url)) =
                (active_server_id.clone(), self.active_server_url())
            {
                self.live.jobs = ui::Loaded::Loading;
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result = api::guide::fetch_jobs(&server_url).await;
                    async_bridge::send_and_repaint(
                        &tx,
                        &ctx2,
                        Msg::GuideJobsLoaded { server_id, result },
                    );
                });
            }
        }
        if matches!(self.live.rules, ui::Loaded::Idle) {
            if let (Some(server_id), Some(server_url)) =
                (active_server_id.clone(), self.active_server_url())
            {
                self.live.rules = ui::Loaded::Loading;
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result = api::guide::fetch_rules(&server_url).await;
                    async_bridge::send_and_repaint(
                        &tx,
                        &ctx2,
                        Msg::GuideRulesLoaded { server_id, result },
                    );
                });
            }
        }
        if matches!(self.live.recorded, ui::Loaded::Idle) {
            if let (Some(server_id), Some(server_url)) =
                (active_server_id.clone(), self.active_server_url())
            {
                self.live.recorded = ui::Loaded::Loading;
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result = api::guide::fetch_recorded_status(&server_url).await;
                    async_bridge::send_and_repaint(
                        &tx,
                        &ctx2,
                        Msg::GuideRecordedLoaded { server_id, result },
                    );
                });
            }
        }
        if self.live.default_padding.is_none() {
            if let (Some(server_id), Some(server_url)) =
                (active_server_id.clone(), self.active_server_url())
            {
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result = api::guide::fetch_default_padding(&server_url).await;
                    async_bridge::send_and_repaint(
                        &tx,
                        &ctx2,
                        Msg::GuideDefaultPaddingLoaded { server_id, result },
                    );
                });
            }
        }
        // Entirely optional — only fetched when the user has pointed
        // Settings at a running `guide-history-service` instance.
        if matches!(self.live.history, ui::Loaded::Idle) {
            if let Some(service_url) = self.settings.history_service_url.clone() {
                self.live.history = ui::Loaded::Loading;
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let now = chrono::Utc::now().timestamp();
                    let result = api::guide_history::fetch_history(
                        &service_url,
                        now - ui::live::HISTORY_LOOKBACK_SECS,
                        now,
                    )
                    .await;
                    async_bridge::send_and_repaint(&tx, &ctx2, Msg::GuideHistoryLoaded(result));
                });
            }
        }
    }

    /// Dispatches whatever the Live guide grid/dialog asked for this frame.
    /// Most cases are self-contained UI state changes already applied
    /// in-place by `ui::live::show()`; only the two async-requiring cases
    /// (a series airing lookup, and actually creating a job/pass) need
    /// `App`'s runtime, which is why this exists as a separate step rather
    /// than being handled entirely inside `ui::live`.
    fn handle_live_action(&mut self, ctx: &egui::Context, action: ui::live::LiveAction) {
        use ui::guide_dialog::DialogAction;
        use ui::live::LiveAction;

        match action {
            LiveAction::None => {}
            LiveAction::Play(channel) => self.start_playback_channel(&channel),
            LiveAction::OpenPastRecording { file_id, has_pass } => {
                let Some(server_url) = self.active_server_url() else {
                    return;
                };
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result =
                        api::recordings::fetch_recording_by_id(&server_url, &file_id).await;
                    async_bridge::send_and_repaint(
                        &tx,
                        &ctx2,
                        Msg::PastRecordingLoaded { result, has_pass },
                    );
                });
            }
            LiveAction::PlayRecording { file_id } => {
                let Some(server_url) = self.active_server_url() else {
                    return;
                };
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result =
                        api::recordings::fetch_recording_by_id(&server_url, &file_id).await;
                    async_bridge::send_and_repaint(&tx, &ctx2, Msg::PlayFetchedRecording(result));
                });
            }
            LiveAction::LookupSeriesAiring {
                series_id,
                channel,
                time,
            } => {
                let Some(server_url) = self.active_server_url() else {
                    return;
                };
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result = api::guide::fetch_series_airings(&server_url, &series_id).await;
                    async_bridge::send_and_repaint(
                        &tx,
                        &ctx2,
                        Msg::GuideSeriesAiringLoaded {
                            channel,
                            time,
                            result,
                        },
                    );
                });
            }
            LiveAction::Dialog(DialogAction::None) | LiveAction::Dialog(DialogAction::Close) => {}
            LiveAction::Dialog(DialogAction::Record {
                name,
                time,
                duration,
                channels,
                airing,
            }) => {
                let Some(server_url) = self.active_server_url() else {
                    return;
                };
                if let Some(d) = self.live.dialog.as_mut() {
                    d.busy = true;
                    d.message = None;
                }
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result = api::guide::create_job(
                        &server_url,
                        &name,
                        time,
                        duration,
                        &channels,
                        airing,
                    )
                    .await;
                    async_bridge::send_and_repaint(&tx, &ctx2, Msg::GuideJobCreated(result));
                });
            }
            LiveAction::Dialog(DialogAction::CreatePass {
                name,
                image,
                series_id,
                new_only,
                pad_start,
                pad_end,
            }) => {
                let Some(server_url) = self.active_server_url() else {
                    return;
                };
                if let Some(d) = self.live.dialog.as_mut() {
                    d.busy = true;
                    d.message = None;
                }
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result = api::guide::create_pass(
                        &server_url,
                        &name,
                        image.as_deref(),
                        &series_id,
                        new_only,
                        pad_start,
                        pad_end,
                    )
                    .await;
                    async_bridge::send_and_repaint(&tx, &ctx2, Msg::GuidePassCreated(result));
                });
            }
        }
    }

    /// Loads all four search corpora up front the first time the Search
    /// screen is visited — matches the old app's "load everything on
    /// mount, filter client-side" design (no server-side search endpoint
    /// exists to call instead).
    fn ensure_search_loaded(&mut self, ctx: &egui::Context) {
        if !matches!(self.search.shows, ui::Loaded::Idle) {
            return;
        }
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        self.search.shows = ui::Loaded::Loading;
        self.search.episodes = ui::Loaded::Loading;
        self.search.movies = ui::Loaded::Loading;
        self.search.videos = ui::Loaded::Loading;

        let tx = self.bridge.tx.clone();
        let ctx2 = ctx.clone();
        let url = server_url.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::recordings::fetch_shows(&url).await;
            async_bridge::send_and_repaint(&tx, &ctx2, Msg::SearchShowsLoaded(result));
        });

        let tx = self.bridge.tx.clone();
        let ctx2 = ctx.clone();
        let url = server_url.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::recordings::fetch_all_episodes(&url).await;
            async_bridge::send_and_repaint(&tx, &ctx2, Msg::SearchEpisodesLoaded(result));
        });

        let tx = self.bridge.tx.clone();
        let ctx2 = ctx.clone();
        let url = server_url.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::recordings::fetch_movies(&url).await;
            async_bridge::send_and_repaint(&tx, &ctx2, Msg::SearchMoviesLoaded(result));
        });

        let tx = self.bridge.tx.clone();
        let ctx2 = ctx.clone();
        let url = server_url;
        self.bridge.runtime.spawn(async move {
            let result = api::library::fetch_all_videos(&url).await;
            async_bridge::send_and_repaint(&tx, &ctx2, Msg::SearchVideosLoaded(result));
        });
    }

    fn ensure_library_loaded(&mut self, ctx: &egui::Context) {
        if !matches!(self.library.groups, ui::Loaded::Idle) {
            return;
        }
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        self.library.groups = ui::Loaded::Loading;
        let tx = self.bridge.tx.clone();
        let ctx = ctx.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::library::fetch_video_groups(&server_url).await;
            async_bridge::send_and_repaint(&tx, &ctx, Msg::VideoGroupsLoaded(result));
        });
    }

    fn ensure_videos_loaded(&mut self, ctx: &egui::Context, group_id: &str) {
        if self.library.videos_group_id.as_deref() == Some(group_id) {
            return;
        }
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        self.library.videos = ui::Loaded::Loading;
        self.library.videos_group_id = Some(group_id.to_string());
        let group_id = group_id.to_string();
        let tx = self.bridge.tx.clone();
        let ctx = ctx.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::library::fetch_videos(&server_url, &group_id).await;
            async_bridge::send_and_repaint(&tx, &ctx, Msg::VideosLoaded { group_id, result });
        });
    }

    fn ensure_collections_loaded(&mut self, ctx: &egui::Context) {
        if !matches!(self.collections.collections, ui::Loaded::Idle) {
            return;
        }
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        self.collections.collections = ui::Loaded::Loading;
        let tx = self.bridge.tx.clone();
        let ctx = ctx.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::collections::fetch_collections(&server_url).await;
            async_bridge::send_and_repaint(&tx, &ctx, Msg::CollectionsLoaded(result));
        });
    }

    fn ensure_collection_shows_loaded(&mut self, ctx: &egui::Context, collection_id: &str) {
        if self.collections.shows_collection_id.as_deref() == Some(collection_id) {
            return;
        }
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        self.collections.shows = ui::Loaded::Loading;
        self.collections.shows_collection_id = Some(collection_id.to_string());
        let collection_id = collection_id.to_string();
        let tx = self.bridge.tx.clone();
        let ctx = ctx.clone();
        self.bridge.runtime.spawn(async move {
            let result =
                api::collections::fetch_collection_shows(&server_url, &collection_id).await;
            async_bridge::send_and_repaint(
                &tx,
                &ctx,
                Msg::CollectionShowsLoaded {
                    collection_id,
                    result,
                },
            );
        });
    }

    fn ensure_tv_shows_loaded(&mut self, ctx: &egui::Context) {
        if !matches!(self.tv_shows.shows, ui::Loaded::Idle) {
            return;
        }
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        self.tv_shows.shows = ui::Loaded::Loading;
        let tx = self.bridge.tx.clone();
        let ctx = ctx.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::recordings::fetch_shows(&server_url).await;
            async_bridge::send_and_repaint(&tx, &ctx, Msg::ShowsLoaded(result));
        });
    }

    /// Fetches episodes whenever the selected show (driven by
    /// `Screen::TvShows.show_id`) differs from whichever show
    /// `tv_shows.episodes` currently holds data for.
    fn ensure_episodes_loaded(&mut self, ctx: &egui::Context, show_id: &str) {
        if self.tv_shows.episodes_show_id.as_deref() == Some(show_id) {
            return;
        }
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        self.tv_shows.episodes = ui::Loaded::Loading;
        self.tv_shows.episodes_show_id = Some(show_id.to_string());
        let show_id = show_id.to_string();
        let tx = self.bridge.tx.clone();
        let ctx = ctx.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::recordings::fetch_episodes(&server_url, &show_id).await;
            async_bridge::send_and_repaint(&tx, &ctx, Msg::EpisodesLoaded { show_id, result });
        });
    }

    /// Optimistic watched-toggle: flips the local cache immediately, fires
    /// the mutation, and rolls back on failure via `Msg::WatchedToggled` in
    /// `drain_messages` — matches the old app's `toggleWatched` exactly.
    fn toggle_watched(&mut self, ctx: &egui::Context, id: &str, new_watched: bool) {
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        patch_watched(&mut self.tv_shows.episodes, id, new_watched);
        patch_watched(&mut self.recent.items, id, new_watched);
        patch_watched(&mut self.movies.items, id, new_watched);

        let id_owned = id.to_string();
        let tx = self.bridge.tx.clone();
        let ctx = ctx.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::recordings::set_watched(&server_url, &id_owned, new_watched).await;
            async_bridge::send_and_repaint(
                &tx,
                &ctx,
                Msg::WatchedToggled {
                    id: id_owned,
                    watched: new_watched,
                    result,
                },
            );
        });
    }

    fn trash(&mut self, ctx: &egui::Context, id: &str) {
        let Some(server_url) = self.active_server_url() else {
            return;
        };
        let id_owned = id.to_string();
        let tx = self.bridge.tx.clone();
        let ctx = ctx.clone();
        self.bridge.runtime.spawn(async move {
            let result = api::recordings::trash_recording(&server_url, &id_owned).await;
            async_bridge::send_and_repaint(
                &tx,
                &ctx,
                Msg::Trashed {
                    id: id_owned,
                    result,
                },
            );
        });
    }

    /// Icon-only when the sidebar is collapsed (icon plus a hover tooltip
    /// carrying the label, so the item stays identifiable without
    /// permanently spending width on text); icon and label side by side
    /// when expanded.
    ///
    /// The icon is a real color image (`assets/icons/*.png`, pre-rendered
    /// from the same emoji the old app displays, via the system's actual
    /// `NotoColorEmoji.ttf` — see the plan doc for why: egui's own text
    /// renderer has no color-glyph support at all, confirmed by reading
    /// `epaint`'s source, so a plain-text icon can never be more than
    /// monochrome no matter which character or font is used). `Button`'s
    /// own `.selected()` gives the same highlight `selectable_label` did,
    /// now for an image+text button instead of a pure-text one.
    fn nav_button(
        &mut self,
        ui: &mut egui::Ui,
        icon: egui::ImageSource<'static>,
        label: &'static str,
        target: Screen,
    ) {
        let selected = self.screen.nav_label() == label;
        let image = egui::Image::new(icon).fit_to_exact_size(egui::vec2(24.0, 24.0));

        let button = if self.settings.sidebar_collapsed {
            egui::Button::image(image)
        } else {
            // `RichText::size` only overrides the size — color is left to
            // `Button`'s own selected/hover painting, same as before.
            egui::Button::image_and_text(image, egui::RichText::new(label).size(15.0))
        }
        .selected(selected)
        .frame(false);

        let resp = ui.add(button);
        let resp = if self.settings.sidebar_collapsed {
            resp.on_hover_text(label)
        } else {
            resp
        };
        if resp.clicked() {
            self.screen = target;
            resp.request_focus();
        }
    }

    fn settings_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let action =
            ui::settings::show(ui, &mut self.settings_ui, &mut self.settings, &self.deploys);

        if let Some((server_id, url)) = action.probe_server {
            let tx = self.bridge.tx.clone();
            let probe_ctx = ctx.clone();
            self.bridge.runtime.spawn(async move {
                let reachable = api::client::probe_url(&url, Duration::from_millis(2500)).await;
                async_bridge::send_and_repaint(
                    &tx,
                    &probe_ctx,
                    Msg::ProbeResult {
                        server_id,
                        reachable,
                    },
                );
            });
        }
        if action.settings_changed {
            if let Err(e) = self.settings.save() {
                eprintln!("settings: failed to save: {e}");
            }
        }
        if let Some(id) = action.switch_server {
            self.switch_active_server(ctx, id);
        }
        if let Some(url) = action.probe_history_service {
            let tx = self.bridge.tx.clone();
            let probe_ctx = ctx.clone();
            self.bridge.runtime.spawn(async move {
                let result = api::guide_history::probe(&url).await;
                async_bridge::send_and_repaint(
                    &tx,
                    &probe_ctx,
                    Msg::HistoryServiceProbeResult(result),
                );
            });
        }
        if let Some((id, target)) = action.test_deploy_target {
            self.deploys.test_connection(id, target);
        }
        if let Some((id, target)) = action.run_deploy_target {
            self.deploys.deploy(id, target);
        }
        if let Some((id, target)) = action.check_deploy_status {
            self.deploys.check_status(id, target);
        }
        if let Some((id, target)) = action.remove_deploy_target {
            self.deploys.remove(id, target);
        }
        if action.reset_channel_genres {
            self.channel_genres.reset();
            if let Err(e) = self.channel_genres.save() {
                eprintln!("channel_genres: failed to save after reset: {e}");
            }
        }

        ui.separator();
        ui.collapsing(
            "Player test panel (temporary — Phase 2 spike carryover)",
            |ui| {
                self.player_test_panel(ui, ctx);
            },
        );
    }

    fn player_test_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label("Manifest/stream URL:");
            ui.add(egui::TextEdit::singleline(&mut self.test_manifest_url).desired_width(500.0));
            let play_clicked = ui.button("Play").clicked();
            match &self.player {
                Ok(player) => {
                    if play_clicked && !self.test_manifest_url.trim().is_empty() {
                        player.load_url(self.test_manifest_url.trim());
                        self.test_loaded_url = Some(self.test_manifest_url.trim().to_string());
                    }
                    if ui
                        .button(if player.paused() { "Resume" } else { "Pause" })
                        .clicked()
                    {
                        player.set_pause(!player.paused());
                    }
                    ui.label("sid:");
                    ui.add(egui::DragValue::new(&mut self.test_sid).range(0..=8));
                    if ui.button("Apply").clicked() {
                        player.set_sid(self.test_sid);
                    }
                }
                Err(e) => {
                    ui.colored_label(egui::Color32::RED, format!("Player unavailable: {e}"));
                }
            }
        });

        if let (Ok(player), Some(loaded)) = (&self.player, &self.test_loaded_url) {
            let pos = player.position_secs().unwrap_or(0.0);
            let dur = player.duration_secs().unwrap_or(0.0);
            ui.label(format!(
                "Loaded: {loaded}  —  {pos:.2}s / {dur:.2}s  —  {}",
                if player.paused() { "paused" } else { "playing" }
            ));

            let (rect, _response) =
                ui.allocate_exact_size(egui::vec2(640.0, 360.0), egui::Sense::hover());
            ui.painter()
                .add(crate::player::paint_callback(player, rect, ctx));
            ctx.request_repaint();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_messages(ctx);

        // Applied unconditionally every frame rather than only on change —
        // cheap and idempotent, and avoids needing a separate "did the
        // settings screen just change this" signal for two fields that live
        // entirely outside egui's own widget state.
        ctx.set_theme(self.settings.theme);
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            if self.settings.window_always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            },
        ));

        // No `.resizable(true)`/fixed `.default_width()` — left to size
        // itself to content each frame (egui's `SidePanel` always stores
        // its actual rendered width and uses that as next frame's starting
        // width regardless of the resizable flag; the earlier resize-bug
        // investigation on the list panels turned up this exact mechanism
        // as the *cause* of a bug there, but it's exactly the *feature*
        // wanted here — "just wide enough for the menu text," collapsing
        // to icon-width and expanding back automatically with no drag
        // needed). Only a `min_width` floor is set, low enough to allow
        // the icon-only collapsed state (egui's own default floor, 96px,
        // would otherwise force it wider than that).
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .min_width(28.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if !self.settings.sidebar_collapsed {
                        ui.heading("DVRDesk");
                    }
                    let (toggle_icon, hover) = if self.settings.sidebar_collapsed {
                        ("☰", "Expand menu")
                    } else {
                        ("◀", "Collapse menu")
                    };
                    if ui.button(toggle_icon).on_hover_text(hover).clicked() {
                        self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed;
                        if let Err(e) = self.settings.save() {
                            eprintln!("settings: failed to save: {e}");
                        }
                    }
                });
                sidebar_separator(ui);

                if !self.settings.sidebar_collapsed && !self.settings.servers.is_empty() {
                    let current_name = self
                        .settings
                        .active_server_id
                        .as_deref()
                        .and_then(|id| self.settings.servers.iter().find(|s| s.id == id))
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "Select server".to_string());
                    let mut switch_to: Option<String> = None;
                    egui::ComboBox::from_id_salt("sidebar_server")
                        // `ComboBox` has a hardcoded 100px minimum width
                        // (`Spacing::combo_width`) applied regardless of
                        // how short the selected text actually is — with
                        // nothing else in this sidebar needing anywhere
                        // near that, it alone was close to doubling the
                        // panel's auto-fit width past what "DVRDesk ◀"
                        // needs. `.width(0.0)` removes that floor,
                        // letting it size to its actual selected text.
                        .width(0.0)
                        .selected_text(current_name)
                        .show_ui(ui, |ui| {
                            for server in &self.settings.servers {
                                let selected = self.settings.active_server_id.as_deref()
                                    == Some(server.id.as_str());
                                if ui.selectable_label(selected, &server.name).clicked() {
                                    switch_to = Some(server.id.clone());
                                }
                            }
                        });
                    if let Some(id) = switch_to {
                        self.switch_active_server(ctx, id);
                    }
                    sidebar_separator(ui);
                }

                self.nav_button(
                    ui,
                    egui::include_image!("../assets/icons/recent.png"),
                    "Recent",
                    Screen::Recent,
                );
                self.nav_button(
                    ui,
                    egui::include_image!("../assets/icons/live.png"),
                    "Live",
                    Screen::Live,
                );
                self.nav_button(
                    ui,
                    egui::include_image!("../assets/icons/tvshows.png"),
                    "TV Shows",
                    Screen::tv_shows(),
                );
                self.nav_button(
                    ui,
                    egui::include_image!("../assets/icons/movies.png"),
                    "Movies",
                    Screen::movies(),
                );
                self.nav_button(
                    ui,
                    egui::include_image!("../assets/icons/videos.png"),
                    "Videos",
                    Screen::library(),
                );
                self.nav_button(
                    ui,
                    egui::include_image!("../assets/icons/collections.png"),
                    "Collections",
                    Screen::collections(),
                );
                self.nav_button(
                    ui,
                    egui::include_image!("../assets/icons/downloads.png"),
                    "Downloads",
                    Screen::Downloads,
                );
                self.nav_button(
                    ui,
                    egui::include_image!("../assets/icons/search.png"),
                    "Search",
                    Screen::Search,
                );
                self.nav_button(
                    ui,
                    egui::include_image!("../assets/icons/settings.png"),
                    "Settings",
                    Screen::Settings,
                );
            });

        if matches!(self.screen, Screen::Recent) {
            self.ensure_recent_loaded(ctx);
        }
        let tv_shows_show_id = match &self.screen {
            Screen::TvShows { show_id, .. } => Some(show_id.clone()),
            _ => None,
        };
        if let Some(show_id) = tv_shows_show_id {
            self.ensure_tv_shows_loaded(ctx);
            if let Some(show_id) = show_id {
                self.ensure_episodes_loaded(ctx, &show_id);
            }
        }
        if matches!(self.screen, Screen::Movies { .. }) {
            self.ensure_movies_loaded(ctx);
        }
        let library_group_id = match &self.screen {
            Screen::Library { group_id, .. } => Some(group_id.clone()),
            _ => None,
        };
        if let Some(group_id) = library_group_id {
            self.ensure_library_loaded(ctx);
            if let Some(group_id) = group_id {
                self.ensure_videos_loaded(ctx, &group_id);
            }
        }
        let collections_id = match &self.screen {
            Screen::Collections { collection_id, .. } => Some(collection_id.clone()),
            _ => None,
        };
        if let Some(collection_id) = collections_id {
            self.ensure_collections_loaded(ctx);
            if let Some(collection_id) = collection_id {
                // Only a "shows" collection has content this app knows how
                // to fetch/render yet (see `api/collections.rs`'s doc
                // comment) — avoid firing a fetch for a type `ui/
                // collections.rs` will just show a "not supported" message
                // for anyway.
                let is_shows = matches!(&self.collections.collections, ui::Loaded::Ready(list)
                    if list.iter().any(|c| c.id == collection_id && c.collection_type == "shows"));
                if is_shows {
                    self.ensure_collection_shows_loaded(ctx, &collection_id);
                }
            }
        }
        if matches!(self.screen, Screen::Search) {
            self.ensure_search_loaded(ctx);
        }
        if matches!(self.screen, Screen::Live) {
            self.ensure_live_loaded(ctx);
        }

        let mut toggle_watched_action: Option<(String, bool)> = None;
        let mut trash_action: Option<String> = None;
        let mut download_action: Option<api::types::Recording> = None;
        let mut download_play_action: Option<String> = None;
        let mut play_video: Option<api::types::Video> = None;
        let mut live_action = ui::live::LiveAction::None;

        // Keyboard list navigation deliberately relies on egui's own
        // built-in Tab-focus system rather than a custom arrow-key scheme —
        // a first attempt at the latter conflicted with egui's internal
        // handling in ways `InputState::consume_key` couldn't suppress
        // (egui's own Tab/focus movement apparently doesn't go through the
        // consumable-key API at all). egui already provides everything
        // needed here for free: Tab/Shift+Tab cycles focusable widgets in
        // creation order (sidebar, then the current screen's list, matching
        // the natural visual order), and a focused clickable widget's
        // `Response::clicked()` already returns true on Enter/Space — no
        // custom activation logic needed. Each screen calls
        // `response.request_focus()` on click so mouse and keyboard focus
        // stay in sync (previously out of sync: click set this app's own
        // selection highlight but not egui's separate focus-border one).

        let play_rec = egui::CentralPanel::default()
            .show(ctx, |ui| {
                if !self.update_dismissed {
                    if let Some(info) = &self.update_info {
                        let version = info.version.clone();
                        let url = info.url.clone();
                        egui::Frame::none()
                            .fill(ui.visuals().hyperlink_color.gamma_multiply(0.15))
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "New version available: {version} (current: v{})",
                                        env!("CARGO_PKG_VERSION")
                                    ));
                                    ui.hyperlink_to("View release", &url);
                                    if ui.small_button("✖").clicked() {
                                        self.update_dismissed = true;
                                    }
                                });
                            });
                        ui.add_space(4.0);
                    }
                }
                if let Some(notice) = self.pending_pass_notice.clone() {
                    egui::Frame::none()
                        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.15))
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(&notice);
                                if ui.small_button("✖").clicked() {
                                    self.pending_pass_notice = None;
                                }
                            });
                        });
                    ui.add_space(4.0);
                }
                match self.screen.clone() {
                    Screen::Settings => {
                        self.settings_screen(ui, ctx);
                        None
                    }
                    Screen::Recent => {
                        let Some(server_url) = self.active_server_url() else {
                            ui.label("Configure a server in Settings first.");
                            return None;
                        };
                        let action = ui::recent::show(ui, &mut self.recent, &server_url);
                        toggle_watched_action = action.toggle_watched;
                        trash_action = action.trash;
                        download_action = action.download;
                        action.play
                    }
                    Screen::TvShows {
                        show_id,
                        episode_id,
                        filter,
                    } => {
                        let Some(server_url) = self.active_server_url() else {
                            ui.label("Configure a server in Settings first.");
                            return None;
                        };
                        let nav = ui::tv_shows::show(
                            ui,
                            &mut self.tv_shows,
                            show_id.as_deref(),
                            episode_id.as_deref(),
                            filter.as_deref(),
                            &server_url,
                        );
                        if let Some(new_screen) = nav.new_screen {
                            self.screen = new_screen;
                        }
                        toggle_watched_action = nav.toggle_watched;
                        trash_action = nav.trash;
                        download_action = nav.download;
                        nav.play
                    }
                    Screen::Movies { movie_id } => {
                        let Some(server_url) = self.active_server_url() else {
                            ui.label("Configure a server in Settings first.");
                            return None;
                        };
                        let action = ui::movies::show(
                            ui,
                            &mut self.movies,
                            movie_id.as_deref(),
                            &server_url,
                        );
                        if let Some(new_screen) = action.new_screen {
                            self.screen = new_screen;
                        }
                        toggle_watched_action = action.toggle_watched;
                        trash_action = action.trash;
                        download_action = action.download;
                        action.play
                    }
                    Screen::Library { group_id, video_id } => {
                        if let Some(server_url) = self.active_server_url() {
                            let action = ui::library::show(
                                ui,
                                &mut self.library,
                                group_id.as_deref(),
                                video_id.as_deref(),
                                &server_url,
                            );
                            if let Some(new_screen) = action.new_screen {
                                self.screen = new_screen;
                            }
                            play_video = action.play;
                        } else {
                            ui.label("Configure a server in Settings first.");
                        }
                        None
                    }
                    Screen::Collections { collection_id, .. } => {
                        if let Some(server_url) = self.active_server_url() {
                            let action = ui::collections::show(
                                ui,
                                &mut self.collections,
                                collection_id.as_deref(),
                                &server_url,
                            );
                            if let Some(new_screen) = action.new_screen {
                                self.screen = new_screen;
                            }
                        } else {
                            ui.label("Configure a server in Settings first.");
                        }
                        None
                    }
                    Screen::Search => {
                        if self.active_server_url().is_none() {
                            ui.label("Configure a server in Settings first.");
                        } else if let Some(new_screen) = ui::search::show(ui, &mut self.search) {
                            self.screen = new_screen;
                        }
                        None
                    }
                    Screen::Downloads => {
                        if let Some(server_url) = self.active_server_url() {
                            let action =
                                ui::downloads_screen::show(ui, &server_url, &self.downloads);
                            if let Some(id) = action.pause {
                                self.downloads.pause(&id);
                            }
                            if let Some((id, record)) = action.resume {
                                self.downloads.start(&id, record);
                            }
                            if let Some(id) = action.remove {
                                self.downloads.remove(&id);
                            }
                            if action.clear_finished {
                                self.downloads.clear_finished();
                            }
                            if let Some(id) = action.play {
                                download_play_action = Some(id);
                            }
                        } else {
                            ui.label("Configure a server in Settings first.");
                        }
                        None
                    }
                    Screen::Live => {
                        if let Some(server_url) = self.active_server_url() {
                            let server_id = self.settings.active_server_id.as_deref().unwrap_or("");
                            live_action = ui::live::show(
                                ui,
                                &mut self.live,
                                self.settings.show_hidden_live_channels,
                                &server_url,
                                &self.channel_genres,
                                server_id,
                            );
                        } else {
                            ui.label("Configure a server in Settings first.");
                        }
                        None
                    }
                }
            })
            .inner;

        if let Some(rec) = play_rec {
            self.start_playback(ctx, &rec);
        }
        if let Some(video) = play_video {
            self.start_playback_video(&video);
        }
        self.handle_live_action(ctx, live_action);
        if let Some((id, watched)) = toggle_watched_action {
            self.toggle_watched(ctx, &id, watched);
        }
        if let Some(id) = trash_action {
            self.trash(ctx, &id);
        }
        if let Some(rec) = download_action {
            if let Some(server_url) = self.active_server_url() {
                use crate::downloads::Status;
                let already = self.downloads.status(&rec.id);
                self.pending_pass_notice = Some(match already {
                    Some(Status::Done(_)) => format!("Already downloaded: {}", rec.title),
                    Some(Status::Active(_)) | Some(Status::Queued) => {
                        format!("Already downloading: {}", rec.title)
                    }
                    _ => {
                        let url = api::recordings::direct_play_url(&server_url, &rec.id);
                        let subtitle = rec.episode_title.as_ref().map(|ep| {
                            match (rec.season_number, rec.episode_number) {
                                (Some(s), Some(e)) => format!("S{s}E{e} — {ep}"),
                                _ => ep.clone(),
                            }
                        });
                        self.downloads.start(
                            &rec.id,
                            crate::downloads::DownloadRecord {
                                url,
                                title: rec.title.clone(),
                                subtitle,
                                thumbnail_url: rec
                                    .thumbnail_url
                                    .clone()
                                    .or_else(|| rec.image_url.clone()),
                            },
                        );
                        format!(
                            "Download started: {} — see the Downloads screen for progress.",
                            rec.title
                        )
                    }
                });
            }
        }
        if let Some(id) = download_play_action {
            if let Some(server_url) = self.active_server_url() {
                let tx = self.bridge.tx.clone();
                let ctx2 = ctx.clone();
                self.bridge.runtime.spawn(async move {
                    let result = api::recordings::fetch_recording_by_id(&server_url, &id).await;
                    async_bridge::send_and_repaint(&tx, &ctx2, Msg::PlayFetchedRecording(result));
                });
            }
        }

        if self.now_playing.is_some() {
            self.tick_playback_persistence(ctx);
            self.tick_captions();
            self.tick_commercial_skip();
        }

        let mut close_playback = false;
        let mut new_caption_mode = None;
        let mut new_skip_ads = None;
        let mut new_show_stats = None;
        if let Some(now_playing) = &self.now_playing {
            if let Ok(player) = &self.player {
                let showing_toast = self
                    .skip_toast_until
                    .is_some_and(|until| Instant::now() < until);
                let action = ui::player_overlay::show(
                    ctx,
                    player,
                    now_playing,
                    &self.caption_tracks,
                    self.caption_mode,
                    self.skip_ads,
                    showing_toast,
                    self.settings.skip_intervals,
                    &self.settings.keybindings,
                    self.settings.diagnostics_enabled,
                    self.show_stats,
                    &mut self.player_controls_active_since,
                );
                close_playback = action.close;
                new_caption_mode = action.new_caption_mode;
                new_skip_ads = action.new_skip_ads;
                new_show_stats = action.new_show_stats;
            }
        }
        if let Some(show_stats) = new_show_stats {
            self.show_stats = show_stats;
        }
        if let Some(mode) = new_caption_mode {
            self.caption_mode = mode;
        }
        if let Some(skip_ads) = new_skip_ads {
            self.skip_ads = skip_ads;
        }
        if close_playback {
            self.stop_playback(ctx);
        }
    }
}

/// A `ui.separator()` substitute for the auto-fit sidebar: the plain widget
/// stretches to `ui.available_size_before_wrap()`, which for a `SidePanel`
/// is the panel's *allocated* width from last frame's stored `PanelState` —
/// not the width its content actually needs this frame. That's fine in a
/// fixed-width panel, but it defeats the sidebar's content-driven auto-sizing:
/// a full-width separator's own rect becomes part of the measured content
/// bounding box that gets stored as *this* frame's width, so once the panel
/// has ever been wide (e.g. expanded, or a stale width persisted from a
/// previous run) the separator keeps reporting that same width back forever,
/// even after collapsing — confirmed directly via a diagnostic trace showing
/// `min_rect` at 19px (icon-only content) alongside a stretched separator
/// still reporting 184px. Bounding the separator's own width to whatever the
/// sidebar has *actually* laid out so far (`ui.min_rect().width()`) breaks
/// that feedback loop without changing anything else's sizing.
fn sidebar_separator(ui: &mut egui::Ui) {
    let content_width = ui.min_rect().width().max(20.0);
    ui.scope(|ui| {
        ui.set_max_width(content_width);
        ui.separator();
    });
}

/// Shared by every screen's optimistic-watched-toggle rollback and success
/// path — patches whichever cached list actually contains this recording,
/// no-op if it doesn't (e.g. a screen that hasn't fetched this item at all).
fn patch_watched(items: &mut ui::Loaded<Vec<api::types::Recording>>, id: &str, watched: bool) {
    if let ui::Loaded::Ready(list) = items {
        if let Some(rec) = list.iter_mut().find(|r| r.id == id) {
            rec.watched = watched;
        }
    }
}

/// Patches playback_time (and watched, if marked) after a successful save —
/// same "keep the local cache in sync with what was actually persisted"
/// principle as `patch_watched`.
fn patch_playback(
    items: &mut ui::Loaded<Vec<api::types::Recording>>,
    id: &str,
    position: f64,
    watched: bool,
) {
    if let ui::Loaded::Ready(list) = items {
        if let Some(rec) = list.iter_mut().find(|r| r.id == id) {
            rec.playback_time = position;
            if watched {
                rec.watched = true;
            }
        }
    }
}

fn remove_recording(items: &mut ui::Loaded<Vec<api::types::Recording>>, id: &str) {
    if let ui::Loaded::Ready(list) = items {
        list.retain(|r| r.id != id);
    }
}

fn find_recording<'a>(
    items: &'a ui::Loaded<Vec<api::types::Recording>>,
    id: &str,
) -> Option<&'a api::types::Recording> {
    match items {
        ui::Loaded::Ready(list) => list.iter().find(|r| r.id == id),
        _ => None,
    }
}

/// Decrements a show's episode-count/unwatched badge after one of its
/// episodes is trashed, removing the show entirely once its count reaches
/// zero (it no longer exists as a show server-side at that point). Returns
/// `true` if the show was removed.
fn patch_show_episode_removed(
    shows: &mut ui::Loaded<Vec<api::types::Show>>,
    show_id: &str,
    was_watched: bool,
) -> bool {
    let ui::Loaded::Ready(list) = shows else {
        return false;
    };
    let Some(s) = list.iter_mut().find(|s| s.id == show_id) else {
        return false;
    };
    s.episode_count = s.episode_count.saturating_sub(1);
    if !was_watched {
        s.number_unwatched = s.number_unwatched.saturating_sub(1);
    }
    if s.episode_count == 0 {
        list.retain(|s| s.id != show_id);
        true
    } else {
        false
    }
}
