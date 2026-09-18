//! Live TV — a program guide grid (channels down the side, a scrollable
//! timeline of programs across), matching Clicker's guide rather than the
//! plain click-to-tune channel list this screen used to be. Recording
//! status (recorded / scheduled / recording now) is shown directly on the
//! grid, and clicking a future program opens `guide_dialog` to record it or
//! create a pass for its series. Clicking a currently-airing cell tunes
//! directly, same as clicking a channel row did before this rewrite.

use std::collections::{HashMap, HashSet};

use super::guide_dialog::{DialogAction, RecordDialogState};
use super::Loaded;
use crate::api::guide::{GuideProgram, Job, ProgramStatus, Rule};
use crate::api::types::Channel;
use crate::channel_genres::{ChannelGenres, Genre};

/// How far back the optional history service is asked to cover — matches
/// the grid's own forward window so scrolling feels roughly symmetric
/// around "now"; the service's own retention may hold more, but there's no
/// reason to display more than this either.
pub const HISTORY_LOOKBACK_SECS: i64 = 86_400;

/// How far ahead to fetch in one shot — confirmed fast enough (~5.9MB,
/// ~0.2s over LAN) that incremental/paginated fetching isn't worth the
/// complexity; a manual Refresh re-fetches from "now" if it goes stale.
pub const GUIDE_DURATION_SECS: u32 = 86_400;

const PIXELS_PER_MIN: f32 = 4.0;
const ROW_HEIGHT: f32 = 56.0;
const CHANNEL_COL_WIDTH: f32 = 190.0;
const LOGO_SIZE: f32 = 40.0;
const RULER_HEIGHT: f32 = 26.0;
/// Total visible gap between adjacent grid cells (both directions) — split
/// evenly as an inset on each side of every block's rect.
const CELL_SPACING: f32 = 5.0;
/// Grid cell title/description text — bigger than egui's default
/// `TextStyle::Small` (9.0) for legibility; an explicit `FontId` rather
/// than a `TextStyle` so it's independent of whatever the user's theme
/// happens to map `Small` to.
const CELL_FONT_SIZE: f32 = 12.5;
const RULER_FONT_SIZE: f32 = 11.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveSort {
    Number,
    Alphabetical,
}

/// Matches the official Channels apps' grid filter row. `Hd`/`Sd`/
/// `Favorites` read straight off `Channel` fields already fetched; the five
/// genre options read from the persisted `channel_genres::ChannelGenres`
/// table (see that module's doc comment) rather than re-deriving a genre
/// from whatever happens to be airing right now — a channel's genre-filter
/// membership needs to stay put across guide refreshes instead of flapping
/// with the moment-to-moment schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveCategory {
    All,
    Hd,
    Sd,
    Favorites,
    Movies,
    Sports,
    Drama,
    News,
    Kids,
}

impl LiveCategory {
    const ALL: [LiveCategory; 9] = [
        LiveCategory::All,
        LiveCategory::Hd,
        LiveCategory::Sd,
        LiveCategory::Favorites,
        LiveCategory::Movies,
        LiveCategory::Sports,
        LiveCategory::Drama,
        LiveCategory::News,
        LiveCategory::Kids,
    ];

    fn label(self) -> &'static str {
        match self {
            LiveCategory::All => "All Channels",
            LiveCategory::Hd => "HD Channels",
            LiveCategory::Sd => "SD Channels",
            LiveCategory::Favorites => "Favorites",
            LiveCategory::Movies => "Movies",
            LiveCategory::Sports => "Sports",
            LiveCategory::Drama => "Drama",
            LiveCategory::News => "News",
            LiveCategory::Kids => "Kids",
        }
    }

    fn genre(self) -> Option<Genre> {
        match self {
            LiveCategory::Movies => Some(Genre::Movies),
            LiveCategory::Sports => Some(Genre::Sports),
            LiveCategory::Drama => Some(Genre::Drama),
            LiveCategory::News => Some(Genre::News),
            LiveCategory::Kids => Some(Genre::Kids),
            _ => None,
        }
    }

    fn matches(self, channel: &Channel, assigned_genre: Option<Genre>) -> bool {
        match self {
            LiveCategory::All => true,
            LiveCategory::Hd => channel.hd,
            LiveCategory::Sd => !channel.hd,
            LiveCategory::Favorites => channel.favorited,
            LiveCategory::Movies
            | LiveCategory::Sports
            | LiveCategory::Drama
            | LiveCategory::News
            | LiveCategory::Kids => {
                let Some(want) = self.genre() else {
                    return false;
                };
                assigned_genre == Some(want)
            }
        }
    }
}

/// One search hit — either a whole channel (number/name matched) or a
/// specific future program (title matched) on a channel. Carries enough to
/// both reposition the grid (`row_idx`, and `start` for a horizontal jump)
/// and to identify the exact cell to highlight while it's the active match.
#[derive(Debug, Clone)]
pub enum SearchMatch {
    Channel {
        row_idx: usize,
    },
    Program {
        row_idx: usize,
        channel: String,
        start: i64,
    },
}

/// Highlight color for the currently-selected search match — distinct from
/// the blue "current program" tint, the gray hover blend, and the red
/// recording indicators, so a jumped-to cell is unambiguous at a glance.
const SEARCH_HIGHLIGHT: egui::Color32 = egui::Color32::from_rgb(255, 200, 40);

pub struct LiveState {
    pub channels: Loaded<Vec<Channel>>,
    pub guide: Loaded<Vec<GuideProgram>>,
    pub jobs: Loaded<Vec<Job>>,
    pub rules: Loaded<Vec<Rule>>,
    pub recorded: Loaded<HashMap<String, ProgramStatus>>,
    /// Only ever populated when `AppSettings.history_service_url` is set —
    /// past guide slots from the optional companion service, merged into
    /// the same timeline as the live (forward-only) guide fetch so the grid
    /// can scroll back into "yesterday". `Idle` forever (never fetched) when
    /// the setting is unset.
    pub history: Loaded<Vec<GuideProgram>>,
    pub default_padding: Option<(i64, i64)>,
    /// All Channels / HD / SD / Favorites / a genre — matches the official
    /// Channels apps' grid filter row. Intersects with `source_filter`
    /// rather than overriding it, same reasoning as that field's own doc
    /// comment: picking a category and a source shows channels in both,
    /// not one silently replacing the other.
    pub category_filter: LiveCategory,
    /// `(source_name, source_id)` — keyed on both, not just the name, since
    /// two physically distinct sources can share an identical `source_name`
    /// (confirmed via the real API: two separate HDHomeRun tuners both
    /// report `source_name: "HDHomeRun QUATRO"`, distinguished only by
    /// `source_id`, e.g. `"1072197B"` vs `"10705F3B"`) — grouping by name
    /// alone would silently merge them into one dropdown entry covering
    /// both devices' channels.
    pub source_filter: Option<(String, Option<String>)>,
    pub sort: LiveSort,
    pub search: String,
    /// Every channel/program hit for the current `search` text, in grid
    /// (row) order — populated on submit, cycled through (without
    /// recomputing) by the toolbar's ◀/▶ buttons.
    pub search_matches: Vec<SearchMatch>,
    pub search_match_idx: usize,
    /// Set once a submitted search finds no matching channel or future
    /// program, so the toolbar can show a brief "No match" instead of
    /// silently doing nothing.
    pub search_no_match: bool,
    /// One-shot scroll targets applied by the next frame's grid render, then
    /// cleared — set by a successful search (channel and/or program match)
    /// so the grid jumps there without needing real drag/scroll input.
    pub pending_scroll_x: Option<f32>,
    pub pending_scroll_y: Option<f32>,
    /// Recomputed fresh every frame by `dedupe()` — channel numbers where
    /// two or more sources currently report genuinely different names for
    /// what's being shown as a single row (confirmed cause: two HDHomeRun
    /// tuners scanning the same physical channel under different names).
    /// Drives a warning banner; empty (the common case) once every
    /// affected tuner has been rescanned.
    pub channel_name_conflicts: Vec<(String, Vec<String>)>,
    pub dialog: Option<RecordDialogState>,
    /// Set when the user clicks a currently-airing slot that also has an
    /// active in-progress recording — offers a choice between joining the
    /// live tune and playing the recording that's already being written,
    /// instead of assuming one or the other.
    pub play_choice: Option<PlayChoiceState>,
    /// Set once the timeline has been auto-scrolled to "now" after a guide
    /// load — without this it opens scrolled to the grid's leftmost edge
    /// (whichever channel happens to have the earliest currently-known
    /// program, often well before "now"), forcing a manual scroll just to
    /// see anything current. Reset to `false` whenever the guide reloads, or
    /// by the toolbar's "Now" button to re-center on demand.
    pub scrolled_to_now: bool,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            channels: Loaded::Idle,
            guide: Loaded::Idle,
            jobs: Loaded::Idle,
            rules: Loaded::Idle,
            recorded: Loaded::Idle,
            history: Loaded::Idle,
            default_padding: None,
            category_filter: LiveCategory::All,
            source_filter: None,
            sort: LiveSort::Number,
            search: String::new(),
            search_matches: Vec::new(),
            search_match_idx: 0,
            search_no_match: false,
            channel_name_conflicts: Vec::new(),
            pending_scroll_x: None,
            pending_scroll_y: None,
            dialog: None,
            play_choice: None,
            scrolled_to_now: false,
        }
    }
}

/// Linear-blend two colors — used to ease a grid cell's fill toward a
/// highlight color on hover. `egui::Color32` has no public lerp of its own
/// (unlike premultiplied-alpha color libraries built for exactly this).
fn mix(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    egui::Color32::from_rgba_premultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        lerp(a.a(), b.a()),
    )
}

pub struct PlayChoiceState {
    pub channel: Channel,
    pub file_id: String,
    pub title: String,
}

/// Normalizes away whitespace differences, not just case — confirmed a
/// real, not hypothetical, gap: the same physical channel appeared under
/// this account as both "Diya TV" and "DiyaTV" (same number, same station,
/// identical programming), which a case-only key doesn't catch, producing
/// two duplicate rows in the guide grid instead of one.
/// ATSC channel numbers are "major.minor" (e.g. "16.1", "16.10" are two
/// *different* sub-channels of major channel 16, not the same channel
/// written two ways) — parsing the whole string as one `f64` collapses
/// them to the identical value 16.1, confirmed as the actual cause of a
/// reported "two channels alternating" flicker: with the sort keys tied,
/// which one landed first depended on `dedupe()`'s `HashMap` iteration
/// order, which is re-randomized every frame. Parsing major/minor as
/// separate integers keeps them distinct and orders them correctly.
fn parse_channel_number(s: &str) -> (u32, u32) {
    let mut parts = s.splitn(2, '.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (major, minor)
}

/// Whitespace- and case-insensitive — confirmed a real, not hypothetical,
/// gap: the same physical channel appeared under this account as both
/// "Diya TV" and "DiyaTV" (same number, same station, identical
/// programming), which a case-only comparison doesn't catch.
fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// favorited(+4) + has-logo(+2) + HD(+1), same weighting as the old app.
fn priority(c: &Channel) -> i32 {
    let mut p = 0;
    if c.favorited {
        p += 4;
    }
    if c.logo_url.is_some() {
        p += 2;
    }
    if c.hd {
        p += 1;
    }
    p
}

/// Result of [`dedupe`]: the collapsed row list, plus any channel numbers
/// where two or more sources reported genuinely different names for what
/// this treated as the same channel.
struct DedupeResult {
    channels: Vec<Channel>,
    /// `(channel number, distinct names seen)` — confirmed a real,
    /// recurring cause, not a one-off: two HDHomeRun tuners scanning the
    /// same physical channel under different names (e.g. one tuner's scan
    /// picked up a slightly different station label than the other's for
    /// the identical broadcast). The underlying fix is on the tuner side
    /// (rescanning), which this app has no way to trigger — so the best it
    /// can do is stop the resulting flicker and tell the user why.
    name_conflicts: Vec<(String, Vec<String>)>,
}

/// Collapses channels sharing the same *number* across multiple sources to
/// a single row — only applied for All/Favorites filters, matching the old
/// app (a specific-source filter shows that source's exact list, undeduped).
///
/// Keyed on number alone, not number+name: two sources can legitimately
/// disagree on a channel's *name* while it's still the same physical
/// channel (confirmed via a real report — two HDHomeRun tuners scanning the
/// same channel under different names), and the previous number+name key
/// left both rows in the list, tied under the Number sort, which produced
/// the exact same `HashMap`-iteration-order flicker already root-caused
/// twice before elsewhere in this file — just via a different data
/// mismatch this time, not a sort bug. When two entries for the same number
/// have the same name (allowing for whitespace/case), the existing
/// priority-based tie-break still applies (the legitimate "prefer the
/// favorited/logoed/HD source" case). When their names genuinely differ,
/// priority is ignored entirely and the first one encountered wins — the
/// user's explicit call, since there's no principled way to prefer one
/// tuner's mis-scanned name over another's — and the number is recorded in
/// `name_conflicts` so the caller can warn about it.
fn dedupe(channels: &[&Channel]) -> DedupeResult {
    let mut best: HashMap<String, Channel> = HashMap::new();
    let mut names_seen: HashMap<String, Vec<String>> = HashMap::new();

    for &c in channels {
        let number = c.number.clone();
        let norm = normalize_name(&c.name);

        let names = names_seen.entry(number.clone()).or_default();
        if !names.iter().any(|n| normalize_name(n) == norm) {
            names.push(c.name.clone());
        }

        match best.get(&number) {
            None => {
                best.insert(number, c.clone());
            }
            Some(existing) => {
                if normalize_name(&existing.name) == norm {
                    let (existing_p, new_p) = (priority(existing), priority(c));
                    let replace = new_p > existing_p
                        || (new_p == existing_p
                            && c.source_name.as_deref().unwrap_or("")
                                < existing.source_name.as_deref().unwrap_or(""));
                    if replace {
                        best.insert(number, c.clone());
                    }
                }
                // Else: genuinely different name for the same number —
                // keep whichever is already in `best` (the first one
                // encountered); `names_seen` above already recorded the
                // conflict.
            }
        }
    }

    // Sorted by channel number, not left in `HashMap` iteration order — this
    // only drives a warning message's text, not click identity, so it can't
    // cause the click/row flicker bug the number-only key above already
    // guards against, but an unsorted order would still make the message's
    // channel list visibly reorder itself frame to frame.
    let mut name_conflicts: Vec<(String, Vec<String>)> = names_seen
        .into_iter()
        .filter(|(_, names)| names.len() > 1)
        .collect();
    name_conflicts.sort_by_key(|(number, _)| parse_channel_number(number));
    DedupeResult {
        channels: best.into_values().collect(),
        name_conflicts,
    }
}

/// Channels with no guide data at all (e.g. local camera/RTSP sources with
/// no TMS station mapping — confirmed a real, not hypothetical, case: the
/// account this was tested against has several) would otherwise render as
/// a totally blank, unclickable row. Synthesizes contiguous 1-hour
/// placeholder slots instead, so the channel can still be tuned live (the
/// "now" slot) or booked for a plain 1-hour recording (future slots) the
/// same way every other row works — no special-cased render/click logic
/// needed, these just flow through the normal program-block path.
fn synthetic_blocks(channel: &Channel, origin: i64, window_secs: i64) -> Vec<GuideProgram> {
    let mut out = Vec::new();
    let mut t = origin - (origin % 3600);
    let end = origin + window_secs;
    while t < end {
        out.push(GuideProgram {
            channel: channel.number.clone(),
            start: t,
            stop: t + 3600,
            title: format!("{} (1 hour)", channel.name),
            desc: None,
            categories: Vec::new(),
            image: None,
            series_id: None,
            program_id: None,
            is_new: false,
        });
        t += 3600;
    }
    out
}

pub fn format_time(unix: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix, 0)
        .single()
        .map(|dt| dt.format("%-I:%M %p").to_string())
        .unwrap_or_default()
}

fn format_hour_tick(unix: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix, 0)
        .single()
        .map(|dt| dt.format("%-I:%M").to_string())
        .unwrap_or_default()
}

pub enum LiveAction {
    None,
    Play(Channel),
    /// Series content needs a real lookup (`api::guide::fetch_series_airings`)
    /// to get the exact native `Airing` object a job/rule creation call
    /// needs — the only case this screen can't finish synchronously, so it's
    /// handed back to `App` (which owns the async runtime) to fulfil.
    LookupSeriesAiring {
        series_id: String,
        channel: String,
        time: i64,
    },
    Dialog(DialogAction),
    /// A past, recorded guide slot was clicked — `App` fetches the actual
    /// `Recording` by file id and navigates to it.
    OpenPastRecording {
        file_id: String,
        has_pass: bool,
    },
    /// The user chose "Play Recording" from `PlayChoiceState` — `App`
    /// fetches the in-progress `Recording` and starts playing it directly
    /// (unlike `OpenPastRecording`, which navigates to a detail screen;
    /// here the clicked slot is airing *now*, so the natural action is to
    /// just start watching, the same as clicking any other recording).
    PlayRecording {
        file_id: String,
    },
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut LiveState,
    show_hidden: bool,
    server_url: &str,
    channel_genres: &ChannelGenres,
    server_id: &str,
) -> LiveAction {
    let mut action = LiveAction::None;

    let channels = match &state.channels {
        Loaded::Idle | Loaded::Loading => {
            ui.label("Loading channels…");
            return LiveAction::None;
        }
        Loaded::Err(e) => {
            ui.colored_label(egui::Color32::RED, e);
            return LiveAction::None;
        }
        Loaded::Ready(c) => c,
    };

    let unhidden: Vec<&Channel> = channels
        .iter()
        .filter(|c| show_hidden || !c.hidden)
        .collect();

    // Distinct (name, id) pairs, not just distinct names — see
    // `LiveState::source_filter`'s doc comment for why. `name_counts`
    // then lets the dropdown only show the disambiguating id when a name
    // genuinely has more than one distinct source behind it, rather than
    // cluttering every single-source entry with an id nobody needs to see.
    let mut sources: Vec<(&str, Option<&str>)> = unhidden
        .iter()
        .filter_map(|c| {
            c.source_name
                .as_deref()
                .map(|name| (name, c.source_id.as_deref()))
        })
        .collect();
    sources.sort_unstable();
    sources.dedup();
    let mut name_counts: HashMap<&str, usize> = HashMap::new();
    for (name, _) in &sources {
        *name_counts.entry(name).or_insert(0) += 1;
    }
    let source_label_for = |name: &str, id: Option<&str>| -> String {
        if name_counts.get(name).copied().unwrap_or(0) > 1 {
            format!("{name} ({})", id.unwrap_or("?"))
        } else {
            name.to_string()
        }
    };

    let mut search_submitted = false;
    let mut search_next = false;
    let mut search_prev = false;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("live_category_filter")
            .selected_text(state.category_filter.label())
            .show_ui(ui, |ui| {
                for cat in LiveCategory::ALL {
                    if ui
                        .selectable_label(state.category_filter == cat, cat.label())
                        .clicked()
                    {
                        state.category_filter = cat;
                    }
                }
            });

        let source_label = state
            .source_filter
            .as_ref()
            .map(|(name, id)| source_label_for(name, id.as_deref()))
            .unwrap_or_else(|| "All Sources".to_string());
        egui::ComboBox::from_id_salt("live_source_filter")
            .selected_text(source_label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(state.source_filter.is_none(), "All Sources")
                    .clicked()
                {
                    state.source_filter = None;
                }
                for (name, id) in &sources {
                    let selected = state
                        .source_filter
                        .as_ref()
                        .map(|(n, i)| (n.as_str(), i.as_deref()))
                        == Some((*name, *id));
                    if ui
                        .selectable_label(selected, source_label_for(name, *id))
                        .clicked()
                    {
                        state.source_filter = Some((name.to_string(), id.map(str::to_string)));
                    }
                }
            });

        ui.separator();
        ui.label("Sort:");
        egui::ComboBox::from_id_salt("live_sort")
            .selected_text(match state.sort {
                LiveSort::Number => "Number",
                LiveSort::Alphabetical => "A-Z",
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(state.sort == LiveSort::Number, "Number")
                    .clicked()
                {
                    state.sort = LiveSort::Number;
                }
                if ui
                    .selectable_label(state.sort == LiveSort::Alphabetical, "A-Z")
                    .clicked()
                {
                    state.sort = LiveSort::Alphabetical;
                }
            });

        ui.separator();
        if ui.button("◎ Now").clicked() {
            state.scrolled_to_now = false;
            state.pending_scroll_y = Some(0.0);
        }

        ui.separator();
        let search_resp = ui.add(
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Search channel or program…")
                .desired_width(180.0),
        );
        let go_clicked = ui.button("🔎").clicked();
        if go_clicked || (search_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            search_submitted = true;
        }
        let has_matches = !state.search_matches.is_empty();
        if ui
            .add_enabled(has_matches, egui::Button::new("◀"))
            .clicked()
        {
            search_prev = true;
        }
        if ui
            .add_enabled(has_matches, egui::Button::new("▶"))
            .clicked()
        {
            search_next = true;
        }
        if has_matches {
            ui.label(format!(
                "{}/{}",
                state.search_match_idx + 1,
                state.search_matches.len()
            ));
        }
        if state.search_no_match {
            ui.colored_label(egui::Color32::RED, "No match");
        }

        ui.separator();
        if ui.button("⟲ Refresh Guide").clicked() {
            state.guide = Loaded::Idle;
            state.jobs = Loaded::Idle;
            state.rules = Loaded::Idle;
            state.recorded = Loaded::Idle;
            state.history = Loaded::Idle;
            state.scrolled_to_now = false;
        }
        match &state.guide {
            Loaded::Loading => {
                ui.spinner();
                ui.label("Loading guide…");
            }
            Loaded::Err(e) => {
                ui.colored_label(egui::Color32::RED, format!("Guide: {e}"));
            }
            _ => {}
        }
    });
    ui.separator();

    let now = chrono::Utc::now().timestamp();

    let filtered: Vec<&Channel> = unhidden
        .iter()
        .filter(|c| {
            let assigned_genre = channel_genres.get(server_id, &c.number);
            if !state.category_filter.matches(c, assigned_genre) {
                return false;
            }
            if let Some((name, id)) = &state.source_filter {
                if c.source_name.as_deref() != Some(name.as_str())
                    || c.source_id.as_deref() != id.as_deref()
                {
                    return false;
                }
            }
            true
        })
        .copied()
        .collect();

    let mut rows: Vec<Channel> = if state.source_filter.is_some() {
        state.channel_name_conflicts.clear();
        filtered.into_iter().cloned().collect()
    } else {
        let result = dedupe(&filtered);
        state.channel_name_conflicts = result.name_conflicts;
        result.channels
    };

    // `dedupe()` collects out of a `HashMap`, whose iteration order is
    // re-randomized on every fresh build (a new instance each frame) — a
    // real, confirmed source of flicker for any two rows that tie under
    // the active sort key, since which one lands first would otherwise
    // depend on that frame's arbitrary hash order. `id` is unique and
    // stable, so appending it as a last-resort tiebreaker to *both* sort
    // modes makes the row order fully deterministic regardless of ties.
    match state.sort {
        LiveSort::Number => rows.sort_by(|a, b| {
            parse_channel_number(&a.number)
                .cmp(&parse_channel_number(&b.number))
                .then_with(|| a.id.cmp(&b.id))
        }),
        LiveSort::Alphabetical => rows.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        }),
    }

    if !state.channel_name_conflicts.is_empty() {
        let numbers = state
            .channel_name_conflicts
            .iter()
            .map(|(number, names)| format!("{number} ({})", names.join(" / ")))
            .collect::<Vec<_>>()
            .join(", ");
        ui.colored_label(
            egui::Color32::from_rgb(230, 160, 30),
            format!(
                "⚠ Channel{} {numbers} {} showing under conflicting names from different tuners — your HDHomeRun tuners likely need to rescan channels.",
                if state.channel_name_conflicts.len() == 1 { "" } else { "s" },
                if state.channel_name_conflicts.len() == 1 { "is" } else { "are" },
            ),
        );
        ui.separator();
    }

    let guide_ready = matches!(state.guide, Loaded::Ready(_));

    // Active-job slots and passed series, precomputed once per frame rather
    // than rescanned per grid cell (~150 rows x ~20 programs each).
    let active_job_slots: HashSet<(String, i64)> = match &state.jobs {
        Loaded::Ready(jobs) => jobs
            .iter()
            .filter(|j| j.is_active())
            .flat_map(|j| {
                let mut keys = Vec::new();
                if !j.channel.is_empty() {
                    keys.push((j.channel.clone(), j.time));
                }
                for c in &j.channels {
                    keys.push((c.clone(), j.time));
                }
                keys
            })
            .collect(),
        _ => HashSet::new(),
    };
    // Non-empty `FileID`s only — confirmed via `curl` that `/dvr/jobs`
    // populates this once a job has actually started writing a file, empty
    // for ones still in the future. Keyed the same way as
    // `active_job_slots` (channel+time), used when the user clicks a
    // currently-airing slot to offer "play the in-progress recording" as
    // an alternative to joining the live tune.
    let active_job_file_id: HashMap<(String, i64), String> = match &state.jobs {
        Loaded::Ready(jobs) => jobs
            .iter()
            .filter(|j| j.is_active() && !j.file_id.is_empty())
            .flat_map(|j| {
                let mut keys = Vec::new();
                if !j.channel.is_empty() {
                    keys.push(((j.channel.clone(), j.time), j.file_id.clone()));
                }
                for c in &j.channels {
                    keys.push(((c.clone(), j.time), j.file_id.clone()));
                }
                keys
            })
            .collect(),
        _ => HashMap::new(),
    };
    let passed_series: HashSet<&str> = match &state.rules {
        Loaded::Ready(rules) => rules.iter().filter_map(|r| r.series_id()).collect(),
        _ => HashSet::new(),
    };
    let empty_recorded: HashMap<String, ProgramStatus> = HashMap::new();
    let recorded: &HashMap<String, ProgramStatus> = match &state.recorded {
        Loaded::Ready(m) => m,
        _ => &empty_recorded,
    };

    let guide: &[GuideProgram] = match &state.guide {
        Loaded::Ready(g) => g,
        _ => &[],
    };
    let history: &[GuideProgram] = match &state.history {
        Loaded::Ready(h) => h,
        _ => &[],
    };

    // Merge live (forward-only) and history (past) slots into one map keyed
    // by (channel, start) — inserting history first so the live fetch wins
    // on the rare occasion both cover the same still-current slot, since
    // it's the more authoritative/fresher of the two.
    let mut merged: HashMap<(&str, i64), &GuideProgram> = HashMap::new();
    for p in history {
        merged.insert((p.channel.as_str(), p.start), p);
    }
    for p in guide {
        merged.insert((p.channel.as_str(), p.start), p);
    }

    let origin = merged.values().map(|p| p.start).min().unwrap_or(now);
    let window_end = now + GUIDE_DURATION_SECS as i64;

    let mut programs_by_channel: HashMap<&str, Vec<&GuideProgram>> = HashMap::new();
    for p in merged.values() {
        programs_by_channel
            .entry(p.channel.as_str())
            .or_default()
            .push(p);
    }
    // `merged.values()` iterates in `HashMap`'s randomized-per-instance
    // order — since this whole map is rebuilt fresh every frame, that order
    // is *not* stable across frames even with identical input. Each click
    // region's widget id is built from its position within this per-channel
    // Vec (`(row_idx, prog_idx)`), and egui's click detection tracks a
    // press-then-release across frames by that id — so an unstable order
    // here meant the id occupying a given screen rect could differ between
    // the press frame and the release frame, letting a single click land on
    // whichever program happened to reshuffle into that slot, confirmed as
    // the actual cause of a reported "same click, different program every
    // time" bug. Sorting by start time makes the order (and therefore every
    // id) deterministic across frames regardless of hash-map randomness.
    for progs in programs_by_channel.values_mut() {
        progs.sort_by_key(|p| p.start);
    }

    let timeline_width = ((window_end - origin).max(0) as f32 / 60.0) * PIXELS_PER_MIN;
    // The ruler now lives outside the vertical scroll (see below, "Excel"
    // frozen-header style), so the scrolled body's own height no longer
    // includes it.
    let body_height = rows.len() as f32 * ROW_HEIGHT;

    let x_for_time = |t: i64| -> f32 { (t - origin) as f32 / 60.0 * PIXELS_PER_MIN };
    // Aligns a timestamp to its nearest *preceding* hour — used for the
    // default/"Now" position and for search jumps alike, so the viewport's
    // left edge lands on (or before) a program's own start instead of some
    // arbitrary offset into the middle of whatever block happens to be
    // there, which was clipping the start of its title off-screen.
    let hour_align = |t: i64| t - t.rem_euclid(3600);

    // Search "positions the guide at a matching channel or program" rather
    // than filtering rows out (unlike Clicker's own search, which hides
    // non-matching rows) — a channel-number/name match wins outright over a
    // program match on the same row; failing that, the soonest future-only
    // program title match. Past programs are never matched, per the user's
    // explicit "future only". Every hit across the whole grid is collected
    // (not just the first) so ◀/▶ can step through them.
    if search_submitted {
        state.search_matches.clear();
        state.search_match_idx = 0;
        state.search_no_match = false;
        let needle = state.search.trim().to_lowercase();
        if !needle.is_empty() {
            for (idx, c) in rows.iter().enumerate() {
                if c.number.to_lowercase().contains(&needle)
                    || c.name.to_lowercase().contains(&needle)
                {
                    state
                        .search_matches
                        .push(SearchMatch::Channel { row_idx: idx });
                    continue;
                }
                if let Some(progs) = programs_by_channel.get(c.number.as_str()) {
                    if let Some(p) = progs
                        .iter()
                        .filter(|p| p.start > now && p.title.to_lowercase().contains(&needle))
                        .min_by_key(|p| p.start)
                    {
                        state.search_matches.push(SearchMatch::Program {
                            row_idx: idx,
                            channel: c.number.clone(),
                            start: p.start,
                        });
                    }
                }
            }
            state.search_no_match = state.search_matches.is_empty();
        }
    }

    if (search_next || search_prev) && !state.search_matches.is_empty() {
        let len = state.search_matches.len();
        state.search_match_idx = if search_next {
            (state.search_match_idx + 1) % len
        } else {
            (state.search_match_idx + len - 1) % len
        };
    }

    if search_submitted || search_next || search_prev {
        if let Some(m) = state.search_matches.get(state.search_match_idx).cloned() {
            match m {
                SearchMatch::Channel { row_idx } => {
                    // No `RULER_HEIGHT` offset here — the ruler now lives
                    // outside the vertical scroll entirely (see the sticky
                    // header below), so row 0 sits at y=0 in this content,
                    // not y=RULER_HEIGHT.
                    state.pending_scroll_y = Some((row_idx as f32 * ROW_HEIGHT - 8.0).max(0.0));
                }
                SearchMatch::Program { row_idx, start, .. } => {
                    state.pending_scroll_y = Some((row_idx as f32 * ROW_HEIGHT - 8.0).max(0.0));
                    state.pending_scroll_x = Some(x_for_time(hour_align(start)).max(0.0));
                }
            }
        }
    }

    let current_match: Option<SearchMatch> =
        state.search_matches.get(state.search_match_idx).cloned();

    // The time ruler now lives outside the vertical scroll entirely —
    // "Excel"-style frozen header row — rather than scrolling away with the
    // channel list beneath it. Its space is reserved here (a fixed-size
    // strip, painted into later) but its actual content can't be painted
    // yet: the x position of each tick depends on the timeline body's
    // current horizontal scroll offset, which egui only knows once that
    // `ScrollArea` has actually been shown below. So: reserve now, paint
    // after — the two don't overlap on screen, so painting the ruler's
    // content later in the same frame (once the offset is known) is
    // visually identical to painting it "in order" would have been.
    let ruler_slot: Option<(egui::Rect, egui::Painter)> = if guide_ready {
        let (resp, painter) = ui
            .horizontal_top(|ui| {
                ui.add_space(CHANNEL_COL_WIDTH);
                ui.allocate_painter(
                    egui::vec2(ui.available_width(), RULER_HEIGHT),
                    egui::Sense::hover(),
                )
            })
            .inner;
        Some((resp.rect, painter))
    } else {
        None
    };

    let mut vertical_scroll = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("live_guide_vertical");
    if let Some(y) = state.pending_scroll_y.take() {
        vertical_scroll = vertical_scroll.scroll_offset(egui::vec2(0.0, y));
    }
    let timeline_offset_x: Option<f32> = vertical_scroll
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                // Frozen channel column — not wrapped in its own horizontal
                // ScrollArea, so it never scrolls sideways; shares the
                // outer vertical scroll with the timeline so rows stay
                // aligned to it.
                ui.vertical(|ui| {
                    // Zero item spacing — the timeline's row positions are
                    // computed with pure `row_idx * ROW_HEIGHT` pixel math
                    // (no per-row gap), so this column's rows must be laid
                    // out identically or they drift out of alignment with
                    // every widget egui would otherwise insert its default
                    // spacing after (confirmed as the actual cause of a
                    // reported misalignment bug, not a guess).
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.set_width(CHANNEL_COL_WIDTH);
                    // Matches the theme's own tone rather than contrasting
                    // against it: dark mode gets a dark (80% gray) plate,
                    // light mode gets a light (20% gray) one.
                    let logo_bg = if ui.visuals().dark_mode {
                        egui::Color32::from_gray(51)
                    } else {
                        egui::Color32::from_gray(204)
                    };
                    for (row_idx, c) in rows.iter().enumerate() {
                        let (rect, _resp) = ui.allocate_exact_size(
                            egui::vec2(CHANNEL_COL_WIDTH, ROW_HEIGHT),
                            egui::Sense::hover(),
                        );
                        // Same inset, rounding, and stroke as a guide grid
                        // cell — this row is now its own bordered "cell",
                        // matching the timeline's appearance exactly rather
                        // than a plain bottom hairline.
                        let inset = CELL_SPACING / 2.0;
                        let cell_rect = egui::Rect::from_min_max(
                            egui::pos2(rect.left() + inset, rect.top() + inset),
                            egui::pos2(rect.right() - inset, rect.bottom() - inset),
                        );
                        let mut cell_fill = ui.visuals().extreme_bg_color;
                        let mut cell_stroke = egui::Stroke::new(
                            1.5_f32,
                            ui.visuals().widgets.noninteractive.bg_stroke.color,
                        );
                        if matches!(&current_match, Some(SearchMatch::Channel { row_idx: r }) if *r == row_idx) {
                            cell_fill = mix(cell_fill, SEARCH_HIGHLIGHT, 0.55);
                            cell_stroke = egui::Stroke::new(2.5_f32, SEARCH_HIGHLIGHT);
                        }
                        ui.painter().rect(cell_rect, 6.0, cell_fill, cell_stroke);

                        // The logo box is always reserved at the same
                        // position and size — whether or not this channel
                        // actually has one — so `text_left` (and therefore
                        // where every row's number/name starts) lines up
                        // across the whole column regardless of which rows
                        // happen to have logo data. A channel with no logo
                        // gets a plain background-colored spacer instead of
                        // the contrast plate: nothing to contrast against,
                        // so it should just blend in rather than draw a
                        // conspicuous empty box.
                        let logo_rect = egui::Rect::from_min_size(
                            egui::pos2(cell_rect.left() + 4.0, cell_rect.center().y - LOGO_SIZE / 2.0),
                            egui::vec2(LOGO_SIZE, LOGO_SIZE),
                        );
                        let text_left = logo_rect.right() + 6.0;
                        if let Some(logo_url) = crate::api::live::channel_logo_url(c, server_url) {
                            // The backing plate behind the logo — most
                            // channel logos are transparent-background PNGs,
                            // and a dark-ink logo on this app's dark theme
                            // (or a light-ink one in light theme) was
                            // otherwise nearly invisible. Painted before the
                            // image so its own transparency composites on top.
                            ui.painter().rect_filled(logo_rect, 3.0, logo_bg);
                            // Plain `paint_at(ui, logo_rect)` was ruled out
                            // (confirmed via egui's own source) since it
                            // skips `calc_size` and always stretches into
                            // whatever rect it's given, ignoring aspect
                            // ratio — that's what was squashing non-square
                            // logos into ovals. `ui.put(rect, image)` fixed
                            // that (it does respect aspect ratio) but turned
                            // out to have its own real bug: `Ui::put` calls
                            // `allocate_new_ui`, which — confirmed via source
                            // — advances the *parent* Ui's layout cursor a
                            // second time (`placer.advance_after_rects`),
                            // conflicting with this loop's own manual
                            // per-row cursor management via
                            // `allocate_exact_size` above and drifting every
                            // row further down the list, worse for rows
                            // further down (the exact "channel column isn't
                            // aligned to the grid" bug reported after that
                            // round). Fixed by computing the aspect-correct
                            // "contain" sub-rect ourselves — the same
                            // scale-to-fit math `ImageFit::Fraction` does
                            // internally — and handing that to `paint_at`
                            // instead, which never touches Ui layout state
                            // at all, matching every other call in this
                            // row's otherwise fully Painter-driven layout.
                            let image = egui::Image::new(logo_url).rounding(3.0).show_loading_spinner(true);
                            let natural_size =
                                image.load_for_size(ui.ctx(), logo_rect.size()).ok().and_then(|t| t.size());
                            let paint_rect = match natural_size {
                                Some(size) if size.x > 0.0 && size.y > 0.0 => {
                                    let scale = (logo_rect.width() / size.x).min(logo_rect.height() / size.y);
                                    egui::Rect::from_center_size(logo_rect.center(), size * scale)
                                }
                                _ => logo_rect,
                            };
                            image.paint_at(ui, paint_rect);
                        } else {
                            // Blends into the cell's own background now that
                            // each row is its own bordered box, rather than
                            // the surrounding panel's — `panel_fill` would
                            // otherwise show up as a mismatched patch inside
                            // the cell's border.
                            ui.painter().rect_filled(logo_rect, 3.0, cell_fill);
                        }

                        let star = if c.favorited { " ★" } else { "" };
                        // A long channel name previously overran the cell
                        // and visibly overlapped the timeline next to it —
                        // `Painter::text` has no width limit or clipping of
                        // its own. Single-line ellipsis truncation, not
                        // wrapping: matches every other channel-name
                        // rendering in this app (`selectable_truncated_
                        // label`), and this cell's fixed `ROW_HEIGHT`
                        // leaves no headroom for a genuine second line
                        // without reopening the channel-column/timeline
                        // vertical-alignment bugs this file has already
                        // been through more than once. Same `into_galley`
                        // truncation technique the timeline's own program
                        // titles already use, just vertically centered
                        // (`Painter::galley` anchors top-left, unlike
                        // `Painter::text`'s `Align2`).
                        let label_galley = egui::WidgetText::from(format!(
                            "{}  {}{}",
                            c.number, c.name, star
                        ))
                        .into_galley(
                            ui,
                            Some(egui::TextWrapMode::Truncate),
                            (cell_rect.right() - text_left - 4.0).max(0.0),
                            egui::TextStyle::Body.resolve(ui.style()),
                        );
                        let label_pos = egui::pos2(
                            text_left,
                            cell_rect.center().y - label_galley.size().y / 2.0,
                        );
                        ui.painter()
                            .galley(label_pos, label_galley, ui.visuals().text_color());
                    }
                });

                if !guide_ready {
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        ui.label("Loading guide…");
                    });
                    return None;
                }

                let mut timeline_scroll = egui::ScrollArea::horizontal().id_salt("live_guide_timeline");
                if let Some(x) = state.pending_scroll_x.take() {
                    timeline_scroll = timeline_scroll.scroll_offset(egui::vec2(x, 0.0));
                } else if !state.scrolled_to_now {
                    let target_x = x_for_time(hour_align(now)).max(0.0);
                    timeline_scroll = timeline_scroll.scroll_offset(egui::vec2(target_x, 0.0));
                    state.scrolled_to_now = true;
                }
                let output = timeline_scroll.show(ui, |ui| {
                    let (response, painter) =
                        ui.allocate_painter(egui::vec2(timeline_width, body_height), egui::Sense::hover());
                    let origin_pos = response.rect.min;

                    // The horizontal `ScrollArea` clips its content `Ui` to
                    // the currently-visible viewport (confirmed by reading
                    // `ScrollArea::Prepared::begin`'s `set_clip_rect` call),
                    // so this is the screen-space x of the first visible
                    // instant in the grid — used below to keep a long
                    // block's text readable once scrolled into its middle.
                    let visible_left = ui.clip_rect().left();

                    // Ruler ticks are now painted separately, into the
                    // sticky header strip reserved above (see `ruler_slot`)
                    // — this body no longer draws its own copy of them.

                    for (row_idx, chan) in rows.iter().enumerate() {
                        let row_top = origin_pos.y + row_idx as f32 * ROW_HEIGHT;
                        let programs: Vec<GuideProgram> = match programs_by_channel.get(chan.number.as_str()) {
                            Some(ps) if !ps.is_empty() => ps.iter().map(|p| (*p).clone()).collect(),
                            // Starts at "now", not `origin` — `origin` can
                            // now reach back a full day via the optional
                            // history merge, and a channel with *no* real
                            // data of its own shouldn't grow a full day of
                            // fake past placeholder blocks implying history
                            // that was never actually captured.
                            _ => synthetic_blocks(chan, now, window_end - now),
                        };

                        for (prog_idx, prog) in programs.iter().enumerate() {
                            let x0 = origin_pos.x + x_for_time(prog.start).max(0.0);
                            let x1 = origin_pos.x + x_for_time(prog.stop);
                            // Symmetric inset on all four sides — half of
                            // `CELL_SPACING` each, so two adjacent cells
                            // (in the same row, or the row above/below)
                            // combine to the full 5px gap, not just this
                            // one side's half of it.
                            let inset = CELL_SPACING / 2.0;
                            // Guards the *inset* result, not just the raw
                            // `x1 <= x0` — a real bug found via a crash
                            // report: a short-duration program (under
                            // ~75s of on-screen width at this zoom level)
                            // could pass a plain `x1 <= x0` check yet still
                            // end up with `x1 - inset < x0 + inset` once
                            // the border inset was subtracted, producing an
                            // inverted `block_rect` (`right() < left()`)
                            // that `f32::clamp` below then panicked on
                            // (`min > max`). Skipping anything too narrow
                            // to show a properly inset cell at all avoids
                            // the inversion outright, not just this one
                            // symptom of it.
                            if x1 - x0 <= CELL_SPACING {
                                continue;
                            }
                            let block_rect = egui::Rect::from_min_max(
                                egui::pos2(x0 + inset, row_top + inset),
                                egui::pos2(x1 - inset, row_top + ROW_HEIGHT - inset),
                            );

                            let is_past = prog.stop <= now;
                            let is_current = prog.start <= now && now < prog.stop;

                            let status = prog.program_id.as_deref().and_then(|id| recorded.get(id));
                            let recorded_file_id = match status {
                                Some(ProgramStatus::Recorded { file_id }) => Some(file_id.as_str()),
                                _ => None,
                            };
                            let is_recording_now = matches!(status, Some(ProgramStatus::Recording));
                            let is_scheduled = active_job_slots
                                .contains(&(chan.number.clone(), prog.start));
                            let has_pass = prog
                                .series_id
                                .as_deref()
                                .map(|sid| passed_series.contains(sid))
                                .unwrap_or(false);
                            // "Existing" covers both a finished recording
                            // and one still being written (a real file
                            // exists either way) — "scheduled" is a future
                            // job that hasn't started recording yet.
                            let is_existing_recording = recorded_file_id.is_some() || is_recording_now;
                            let is_scheduled_new = is_scheduled && !is_existing_recording;
                            let is_search_highlight = matches!(
                                &current_match,
                                Some(SearchMatch::Program { channel, start, .. })
                                    if channel == &chan.number && *start == prog.start
                            );

                            // Interacted with before painting (not after, as
                            // this used to be structured) so the hover
                            // response can feed back into this block's own
                            // fill color. Every cell gets a response — past,
                            // non-recorded slots use `Sense::hover()` alone
                            // (mouseover highlight without implying they're
                            // clickable), everything else also senses clicks.
                            let interactive = !is_past || recorded_file_id.is_some();
                            let id = ui.make_persistent_id(("guide_block", row_idx, prog_idx));
                            let sense = if interactive { egui::Sense::click() } else { egui::Sense::hover() };
                            let resp = ui.interact(block_rect, id, sense);
                            let hover_t = ui
                                .ctx()
                                .animate_bool_with_time(id.with("hover"), resp.hovered(), 0.1);

                            let mut fill = if is_past {
                                ui.visuals().faint_bg_color
                            } else {
                                ui.visuals().extreme_bg_color
                            };
                            if is_current {
                                fill = ui.visuals().selection.bg_fill.gamma_multiply(0.35);
                            }
                            if hover_t > 0.0 {
                                fill = mix(fill, ui.visuals().widgets.hovered.bg_fill, hover_t * 0.6);
                            }
                            // Recording/scheduled status is conveyed entirely
                            // by the circle/bullseye indicator on the title
                            // line (see below), not by the border — every
                            // block gets the same rounded hairline regardless
                            // of status.
                            let mut stroke = egui::Stroke::new(
                                1.5_f32,
                                ui.visuals().widgets.noninteractive.bg_stroke.color,
                            );
                            // The active search match gets a distinct gold
                            // highlight, painted last so it wins over both
                            // the "current program" tint and any hover blend
                            // — the whole point is to be unambiguous after a
                            // jump, even if the cell also happens to be
                            // hovered or airing now.
                            if is_search_highlight {
                                fill = mix(fill, SEARCH_HIGHLIGHT, 0.55);
                                stroke = egui::Stroke::new(2.5_f32, SEARCH_HIGHLIGHT);
                            }

                            painter.rect(block_rect, 6.0, fill, stroke);

                            let text_color = if is_past {
                                ui.visuals().weak_text_color()
                            } else {
                                ui.visuals().text_color()
                            };
                            const RECORDING_RED: egui::Color32 = egui::Color32::from_rgb(220, 50, 50);
                            const NEW_AMBER: egui::Color32 = egui::Color32::from_rgb(255, 190, 60);
                            let small_font = egui::FontId::proportional(CELL_FONT_SIZE);

                            // Painted directly (`circle_filled`/
                            // `circle_stroke`), not as a text glyph — found
                            // via screenshot that "●"/"◎" render as
                            // fixed-color emoji-style glyphs on this font
                            // stack (egui's bundled emoji font ignores the
                            // requested `TextFormat` color entirely), which
                            // is why an earlier attempt at this rendered as
                            // a red-and-white icon instead of a plain red
                            // shape. A manually painted circle has no such
                            // ambiguity and matches "red circle"/"red
                            // bullseye" exactly.
                            // A program that started well before the
                            // current scroll position (a movie, a
                            // multi-hour placeholder block) otherwise has
                            // its title anchored to the block's true start
                            // — off-screen to the left, so nothing readable
                            // shows once scrolled into the block's middle.
                            // Once the true start is more than an hour
                            // before the first visible instant, anchor the
                            // text to the visible edge instead, the same
                            // "sticky label" idiom timeline/gantt UIs use
                            // for wide bars — clamped to the block's own
                            // bounds so it never text overflows past either
                            // edge.
                            let text_left_edge = if visible_left - x0 > 60.0 * PIXELS_PER_MIN {
                                visible_left.clamp(block_rect.left(), block_rect.right())
                            } else {
                                block_rect.left()
                            };
                            let mut title_left = text_left_edge + 3.0;
                            if is_existing_recording || is_scheduled_new {
                                let indicator_center =
                                    egui::pos2(title_left + 4.0, block_rect.top() + 2.0 + small_font.size / 2.0);
                                if is_existing_recording {
                                    painter.circle_filled(indicator_center, 4.0, RECORDING_RED);
                                } else {
                                    // Bullseye: a ring plus a small filled
                                    // center dot.
                                    painter.circle_stroke(indicator_center, 4.0, egui::Stroke::new(1.3_f32, RECORDING_RED));
                                    painter.circle_filled(indicator_center, 1.4, RECORDING_RED);
                                }
                                title_left += 11.0;
                            }

                            // A `LayoutJob` (not a plain string) since the
                            // pass star/NEW tag need their own colors
                            // distinct from the title — built as one job
                            // rather than separate painter calls so
                            // truncation (`TextWrapMode::Truncate`, via
                            // `into_galley` below) accounts for all of it
                            // at once instead of clipping the title without
                            // knowing the tag needs room too.
                            let mut title_job = egui::text::LayoutJob::default();
                            if has_pass && !is_existing_recording && !is_scheduled_new {
                                title_job.append(
                                    "★ ",
                                    0.0,
                                    egui::TextFormat { font_id: small_font.clone(), color: text_color, ..Default::default() },
                                );
                            }
                            title_job.append(
                                &prog.title,
                                0.0,
                                egui::TextFormat { font_id: small_font.clone(), color: text_color, ..Default::default() },
                            );
                            if prog.is_new {
                                title_job.append(
                                    " NEW",
                                    0.0,
                                    egui::TextFormat { font_id: small_font.clone(), color: NEW_AMBER, ..Default::default() },
                                );
                            }
                            let title_galley = egui::WidgetText::from(title_job).into_galley(
                                ui,
                                Some(egui::TextWrapMode::Truncate),
                                (block_rect.right() - title_left - 1.0).max(0.0),
                                small_font.clone(),
                            );
                            let title_pos = egui::pos2(title_left, block_rect.top() + 2.0);
                            painter.galley(title_pos, title_galley.clone(), text_color);

                            // Second line: as much of the description as
                            // fits, single-line-truncated the same way —
                            // only painted if it actually fits within the
                            // block's remaining height, so a very short
                            // slot doesn't get an overlapping second line.
                            if let Some(desc) = prog.desc.as_deref().filter(|d| !d.trim().is_empty()) {
                                let line2_x = text_left_edge + 3.0;
                                let line2_y = title_pos.y + title_galley.size().y + 1.0;
                                if line2_y + small_font.size <= block_rect.bottom() {
                                    let desc_color = ui.visuals().weak_text_color();
                                    let desc_galley = egui::WidgetText::from(desc).into_galley(
                                        ui,
                                        Some(egui::TextWrapMode::Truncate),
                                        (block_rect.right() - line2_x - 1.0).max(0.0),
                                        small_font.clone(),
                                    );
                                    painter.galley(egui::pos2(line2_x, line2_y), desc_galley, desc_color);
                                }
                            }

                            // Anything clickable at all — a future/current
                            // slot, or a past slot that was actually
                            // recorded (nothing to do with a past slot that
                            // wasn't, so those stay inert). `resp`/`id` were
                            // already built above (before painting, so the
                            // hover state could feed back into the fill
                            // color) — positional, not content-based: the
                            // guide feed can legitimately list more than one
                            // programme for the same channel+start (a
                            // channel number shared by multiple raw
                            // source/station mappings the XMLTV export
                            // doesn't dedupe), which content-based ids
                            // collided on, confirmed as the cause of a
                            // "First use of widget ID ..." flicker on the
                            // affected rows. `(row_idx, prog_idx)` is unique
                            // by construction regardless of data quality.
                            if interactive {
                                if resp.clicked() {
                                    if is_past {
                                        if let Some(file_id) = recorded_file_id {
                                            action = LiveAction::OpenPastRecording {
                                                file_id: file_id.to_string(),
                                                has_pass,
                                            };
                                        }
                                    } else if is_current {
                                        let active_file_id = active_job_file_id
                                            .get(&(chan.number.clone(), prog.start))
                                            .cloned();
                                        if let Some(file_id) = active_file_id {
                                            state.play_choice = Some(PlayChoiceState {
                                                channel: chan.clone(),
                                                file_id,
                                                title: prog.title.clone(),
                                            });
                                        } else {
                                            action = LiveAction::Play(chan.clone());
                                        }
                                    } else {
                                        let (pad_start, pad_end) = state.default_padding.unwrap_or((0, 60));
                                        let mut dialog = RecordDialogState::new(
                                            chan.clone(),
                                            (*prog).clone(),
                                            pad_start,
                                            pad_end,
                                        );
                                        dialog.existing_pass = prog
                                            .series_id
                                            .as_deref()
                                            .map(|sid| passed_series.contains(sid))
                                            .unwrap_or(false);
                                        if let Some(series_id) = prog.series_id.clone() {
                                            if !dialog.existing_pass {
                                                action = LiveAction::LookupSeriesAiring {
                                                    series_id,
                                                    channel: chan.number.clone(),
                                                    time: prog.start,
                                                };
                                            } else {
                                                dialog.airing = Loaded::Ready(
                                                    crate::api::guide::build_fallback_airing(prog),
                                                );
                                            }
                                        } else {
                                            dialog.airing =
                                                Loaded::Ready(crate::api::guide::build_fallback_airing(prog));
                                        }
                                        state.dialog = Some(dialog);
                                    }
                                }
                            }
                        }
                    }

                    // "Now" line, drawn last so it sits above program blocks.
                    let now_x = origin_pos.x + x_for_time(now);
                    painter.vline(
                        now_x,
                        origin_pos.y..=(origin_pos.y + body_height),
                        egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(220, 50, 50)),
                    );
                });
                Some(output.state.offset.x)
            })
            .inner
        })
        .inner;

    // Now that the timeline body has actually been shown (and reported its
    // real, post-interaction horizontal offset), paint the sticky ruler's
    // content into the strip reserved earlier — see `ruler_slot`'s doc
    // comment for why this has to happen in this order.
    if let (Some((ruler_rect, ruler_painter)), Some(offset_x)) = (ruler_slot, timeline_offset_x) {
        ruler_painter.rect_filled(ruler_rect, 0.0, ui.visuals().panel_fill);
        let mut t = origin - (origin % 1800);
        while t <= window_end {
            let x = ruler_rect.left() + x_for_time(t) - offset_x;
            if x >= ruler_rect.left() - 1.0 && x <= ruler_rect.right() {
                ruler_painter.vline(
                    x,
                    ruler_rect.top()..=ruler_rect.bottom(),
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
                ruler_painter.text(
                    egui::pos2(x + 3.0, ruler_rect.top() + 2.0),
                    egui::Align2::LEFT_TOP,
                    format_hour_tick(t),
                    egui::FontId::proportional(RULER_FONT_SIZE),
                    ui.visuals().weak_text_color(),
                );
            }
            t += 1800;
        }
        ruler_painter.hline(
            ruler_rect.x_range(),
            ruler_rect.bottom(),
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
    }

    // The middle of the grid's own display area (this `ui` is the
    // CentralPanel's content `Ui`, unshadowed since the top of this
    // function — its `max_rect()` is the grid area, not the whole window,
    // so a dialog centers relative to what's visible next to the sidebar).
    let grid_center = ui.max_rect().center();

    if let Some(dialog_state) = state.dialog.as_mut() {
        let ctx = ui.ctx().clone();
        match super::guide_dialog::show(&ctx, dialog_state, grid_center) {
            DialogAction::None => {}
            DialogAction::Close => state.dialog = None,
            other => action = LiveAction::Dialog(other),
        }
    }

    if let Some(choice) = &state.play_choice {
        let mut open = true;
        let mut chosen: Option<LiveAction> = None;
        egui::Window::new(&choice.title)
            .id(egui::Id::new(("live_play_choice", &choice.file_id)))
            .collapsible(false)
            .resizable(false)
            .default_pos(grid_center)
            .pivot(egui::Align2::CENTER_CENTER)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label("This is currently being recorded. What would you like to watch?");
                ui.horizontal(|ui| {
                    if ui.button("📶 Watch Live").clicked() {
                        chosen = Some(LiveAction::Play(choice.channel.clone()));
                    }
                    if ui.button("⏺ Play Recording").clicked() {
                        chosen = Some(LiveAction::PlayRecording {
                            file_id: choice.file_id.clone(),
                        });
                    }
                });
            });
        if let Some(chosen) = chosen {
            action = chosen;
            state.play_choice = None;
        } else if !open {
            state.play_choice = None;
        }
    }

    action
}
