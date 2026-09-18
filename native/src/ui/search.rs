//! Port of `Search.tsx` — fully client-side keyword search across shows,
//! episodes, movies, and videos, all loaded up front (no server-side search
//! endpoint exists). Search only triggers on submit, not live-as-you-type,
//! matching the old app.
//!
//! Results render as a real table (`Type`/`Created Date`/`Modified Date`/
//! `Title`/`Episode Title`/`Summary`/`Full Summary`) rather than a plain
//! list of rows — a genuinely different shape from every other screen's
//! list-plus-detail-pane pattern, since search results span four different
//! item kinds at once with no single shared detail view to click into.

use std::collections::HashMap;

use chrono::{Local, TimeZone};

use super::Loaded;
use crate::api::types::{Recording, Show, Video};
use crate::screen::Screen;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchType {
    Any,
    Title,
    Summary,
    SeriesName,
}

pub struct SearchState {
    pub shows: Loaded<Vec<Show>>,
    pub episodes: Loaded<Vec<Recording>>,
    pub movies: Loaded<Vec<Recording>>,
    pub videos: Loaded<Vec<Video>>,
    pub keyword_draft: String,
    pub submitted_keyword: String,
    pub search_type: SearchType,
    pub show_watched: bool,
    pub show_partial: bool,
    pub show_unwatched: bool,
    pub include_series: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            shows: Loaded::Idle,
            episodes: Loaded::Idle,
            movies: Loaded::Idle,
            videos: Loaded::Idle,
            keyword_draft: String::new(),
            submitted_keyword: String::new(),
            search_type: SearchType::Any,
            show_watched: true,
            show_partial: true,
            show_unwatched: true,
            include_series: true,
        }
    }
}

/// `Recording` and `Video` both carry `playback_time` but not every one of
/// them has a reliable `duration` to compute an exact fraction from, so
/// "partial" is decided the same way the card/detail progress bars
/// elsewhere in this app already treat it: any resumable position at all,
/// short of the `watched` flag itself being set.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WatchedState {
    Watched,
    Partial,
    Unwatched,
}

fn watched_state(watched: bool, playback_time: f64) -> WatchedState {
    if watched {
        WatchedState::Watched
    } else if playback_time > 0.0 {
        WatchedState::Partial
    } else {
        WatchedState::Unwatched
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultKind {
    Show,
    Episode,
    Movie,
    Video,
}

impl ResultKind {
    fn rank(self) -> u8 {
        match self {
            ResultKind::Show => 0,
            ResultKind::Episode => 1,
            ResultKind::Movie => 2,
            ResultKind::Video => 3,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ResultKind::Show => "TV Series",
            ResultKind::Episode => "TV Episode",
            ResultKind::Movie => "Movie",
            ResultKind::Video => "Video",
        }
    }
}

struct ResultRow {
    kind: ResultKind,
    title: String,
    episode_title: Option<String>,
    summary: Option<String>,
    full_summary: Option<String>,
    created_at: i64,
    /// `None` where the source type has no such field at all (`Video`) —
    /// shown as "—" rather than falling back to `created_at`, since that
    /// would silently claim knowledge this app doesn't actually have.
    updated_at: Option<i64>,
    navigate_to: Screen,
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn watched_filter_ok(state: &SearchState, ws: WatchedState) -> bool {
    match ws {
        WatchedState::Watched => state.show_watched,
        WatchedState::Partial => state.show_partial,
        WatchedState::Unwatched => state.show_unwatched,
    }
}

fn compute_results(state: &SearchState) -> Vec<ResultRow> {
    let needle = state.submitted_keyword.trim();
    if needle.is_empty() {
        return Vec::new();
    }

    let shows: &[Show] = match &state.shows {
        Loaded::Ready(s) => s,
        _ => &[],
    };
    let episodes: &[Recording] = match &state.episodes {
        Loaded::Ready(e) => e,
        _ => &[],
    };
    let movies: &[Recording] = match &state.movies {
        Loaded::Ready(m) => m,
        _ => &[],
    };
    let videos: &[Video] = match &state.videos {
        Loaded::Ready(v) => v,
        _ => &[],
    };

    let show_name_by_id: HashMap<&str, &str> = shows
        .iter()
        .map(|s| (s.id.as_str(), s.name.as_str()))
        .collect();
    let matches_show_name = |show_id: Option<&str>| -> bool {
        show_id
            .and_then(|id| show_name_by_id.get(id))
            .is_some_and(|name| contains_ci(name, needle))
    };

    let mut rows = Vec::new();

    if matches!(state.search_type, SearchType::Any | SearchType::SeriesName) {
        for s in shows {
            let hit = match state.search_type {
                SearchType::SeriesName => contains_ci(&s.name, needle),
                _ => {
                    contains_ci(&s.name, needle)
                        || s.summary
                            .as_deref()
                            .is_some_and(|sum| contains_ci(sum, needle))
                }
            };
            if hit {
                rows.push(ResultRow {
                    kind: ResultKind::Show,
                    title: s.name.clone(),
                    episode_title: None,
                    summary: s.summary.clone(),
                    full_summary: None,
                    created_at: s.last_recorded_at,
                    updated_at: Some(s.updated_at),
                    navigate_to: Screen::TvShows {
                        show_id: Some(s.id.clone()),
                        episode_id: None,
                        filter: None,
                    },
                });
            }
        }
    }

    for e in episodes {
        if !watched_filter_ok(state, watched_state(e.watched, e.playback_time)) {
            continue;
        }
        let hit = match state.search_type {
            SearchType::Title => {
                contains_ci(&e.title, needle)
                    || e.episode_title
                        .as_deref()
                        .is_some_and(|t| contains_ci(t, needle))
            }
            SearchType::Summary => {
                e.summary.as_deref().is_some_and(|s| contains_ci(s, needle))
                    || e.full_summary
                        .as_deref()
                        .is_some_and(|s| contains_ci(s, needle))
            }
            SearchType::SeriesName => matches_show_name(e.show_id.as_deref()),
            SearchType::Any => {
                contains_ci(&e.title, needle)
                    || e.episode_title
                        .as_deref()
                        .is_some_and(|t| contains_ci(t, needle))
                    || e.summary.as_deref().is_some_and(|s| contains_ci(s, needle))
                    || matches_show_name(e.show_id.as_deref())
            }
        };
        if hit {
            rows.push(ResultRow {
                kind: ResultKind::Episode,
                title: e.title.clone(),
                episode_title: e.episode_title.clone(),
                summary: e.summary.clone(),
                full_summary: e.full_summary.clone(),
                created_at: e.created_at,
                updated_at: Some(e.updated_at),
                navigate_to: Screen::TvShows {
                    show_id: e.show_id.clone(),
                    episode_id: Some(e.id.clone()),
                    filter: None,
                },
            });
        }
    }

    // Movies and videos are never matched by Series Name, same as the old app.
    if !matches!(state.search_type, SearchType::SeriesName) {
        for m in movies {
            if !watched_filter_ok(state, watched_state(m.watched, m.playback_time)) {
                continue;
            }
            let hit = match state.search_type {
                SearchType::Title => contains_ci(&m.title, needle),
                SearchType::Summary => {
                    m.summary.as_deref().is_some_and(|s| contains_ci(s, needle))
                        || m.full_summary
                            .as_deref()
                            .is_some_and(|s| contains_ci(s, needle))
                }
                SearchType::Any => {
                    contains_ci(&m.title, needle)
                        || m.summary.as_deref().is_some_and(|s| contains_ci(s, needle))
                }
                SearchType::SeriesName => false,
            };
            if hit {
                rows.push(ResultRow {
                    kind: ResultKind::Movie,
                    title: m.title.clone(),
                    episode_title: None,
                    summary: m.summary.clone(),
                    full_summary: m.full_summary.clone(),
                    created_at: m.created_at,
                    updated_at: Some(m.updated_at),
                    navigate_to: Screen::Movies {
                        movie_id: Some(m.id.clone()),
                    },
                });
            }
        }

        for v in videos {
            if !watched_filter_ok(state, watched_state(v.watched, v.playback_time)) {
                continue;
            }
            let vt = v.video_title.as_deref().unwrap_or(&v.title);
            let hit = match state.search_type {
                SearchType::Summary => false, // no summary field on Video
                _ => contains_ci(vt, needle) || contains_ci(&v.title, needle),
            };
            if hit {
                rows.push(ResultRow {
                    kind: ResultKind::Video,
                    title: vt.to_string(),
                    episode_title: None,
                    summary: None,
                    full_summary: None,
                    created_at: v.created_at,
                    updated_at: None,
                    navigate_to: Screen::Library {
                        group_id: v.video_group_id.clone(),
                        video_id: Some(v.id.clone()),
                    },
                });
            }
        }
    }

    rows.sort_by(|a, b| {
        a.kind
            .rank()
            .cmp(&b.kind.rank())
            .then(b.created_at.cmp(&a.created_at))
    });
    rows
}

/// Returns `Some(screen)` the frame a result row is clicked — clicking
/// deep-links into the owning screen rather than playing directly, matching
/// the old app exactly.
pub fn show(ui: &mut egui::Ui, state: &mut SearchState) -> Option<Screen> {
    let mut navigate: Option<Screen> = None;

    ui.heading("Search");
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.label("Keyword:");
        let resp = ui.text_edit_singleline(&mut state.keyword_draft);
        let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button("Search").clicked() || enter_pressed {
            state.submitted_keyword = state.keyword_draft.clone();
        }
    });
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.label("Match:");
        ui.selectable_value(&mut state.search_type, SearchType::Any, "Any");
        ui.selectable_value(&mut state.search_type, SearchType::Title, "Title");
        ui.selectable_value(&mut state.search_type, SearchType::Summary, "Summary");
        ui.selectable_value(
            &mut state.search_type,
            SearchType::SeriesName,
            "Series Name",
        );
    });
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.checkbox(&mut state.show_unwatched, "Not watched");
        ui.checkbox(&mut state.show_partial, "Partially watched");
        ui.checkbox(&mut state.show_watched, "Watched");
        ui.checkbox(&mut state.include_series, "Include Series");
    });
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);

    if state.submitted_keyword.trim().is_empty() {
        ui.weak("Enter a keyword and press Search.");
        return None;
    }

    let loading = matches!(state.shows, Loaded::Idle | Loaded::Loading)
        || matches!(state.episodes, Loaded::Idle | Loaded::Loading)
        || matches!(state.movies, Loaded::Idle | Loaded::Loading)
        || matches!(state.videos, Loaded::Idle | Loaded::Loading);
    if loading {
        ui.label("Loading search index…");
        return None;
    }

    let mut results = compute_results(state);
    if !state.include_series {
        results.retain(|r| r.kind != ResultKind::Show);
    }

    if results.is_empty() {
        ui.weak("No results.");
        return None;
    }

    let count_label = if results.len() == 1 {
        "1 result".to_string()
    } else {
        format!("{} results", results.len())
    };
    ui.label(egui::RichText::new(count_label).strong());
    ui.add_space(8.0);

    use egui_extras::{Column, TableBuilder};

    // Summary/Full Summary get a *fixed*, non-resizable width rather than
    // the other five columns' free resizing — a real per-row dynamic
    // height (below) needs to know each cell's wrap width in order to
    // measure how many lines it needs, and that width isn't knowable
    // until `TableBuilder` has already resolved it for this exact frame
    // (which happens inside `.body()`, too late to feed back into the row
    // heights `.body()` itself needs up front) — the same chicken-and-egg
    // this codebase already hit once with `egui::Image` sizing. Pinning
    // these two columns' widths removes the unknown, at the cost of the
    // user no longer being able to drag them wider/narrower.
    const SUMMARY_COL_WIDTH: f32 = 320.0;
    const FULL_SUMMARY_COL_WIDTH: f32 = 320.0;
    const CELL_TOP_PAD: f32 = 6.0;
    const SUMMARY_BOTTOM_GAP: f32 = 6.0;

    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let single_line_height =
        ui.fonts(|f| f.row_height(&font_id)) + CELL_TOP_PAD + SUMMARY_BOTTOM_GAP;
    let measure = |text: Option<&str>, wrap_width: f32| -> f32 {
        match text {
            Some(t) if !t.is_empty() => ui.fonts(|f| {
                f.layout(
                    t.to_string(),
                    font_id.clone(),
                    egui::Color32::PLACEHOLDER,
                    wrap_width,
                )
                .size()
                .y
            }),
            _ => 0.0,
        }
    };
    // One real measurement pass per row, using the same font/wrap-width
    // pairing the cells themselves render with below — not a guess.
    let row_heights: Vec<f32> = results
        .iter()
        .map(|r| {
            let summary_h = measure(r.summary.as_deref(), SUMMARY_COL_WIDTH);
            let full_h = measure(r.full_summary.as_deref(), FULL_SUMMARY_COL_WIDTH);
            (summary_h.max(full_h) + CELL_TOP_PAD + SUMMARY_BOTTOM_GAP).max(single_line_height)
        })
        .collect();

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .column(Column::initial(90.0).at_least(60.0).clip(true)) // Type
        .column(Column::initial(150.0).at_least(100.0).clip(true)) // Created Date
        .column(Column::initial(150.0).at_least(100.0).clip(true)) // Modified Date
        .column(Column::initial(200.0).at_least(80.0).clip(true)) // Title
        .column(Column::initial(200.0).at_least(80.0).clip(true)) // Episode Title
        .column(Column::exact(SUMMARY_COL_WIDTH).resizable(false)) // Summary
        .column(Column::exact(FULL_SUMMARY_COL_WIDTH).resizable(false)) // Full Summary
        .header(22.0, |mut header| {
            for label in [
                "Type",
                "Created Date",
                "Modified Date",
                "Title",
                "Episode Title",
                "Summary",
                "Full Summary",
            ] {
                header.col(|ui| {
                    ui.label(egui::RichText::new(label).strong());
                });
            }
        })
        .body(|body| {
            body.heterogeneous_rows(row_heights.into_iter(), |mut row| {
                let r = &results[row.index()];
                // Every cell explicitly opts into `Label::wrap()` — a
                // clipped `Column` (every one of them, above) makes
                // `egui_extras` force the cell `Ui`'s *default* wrap mode
                // to `Truncate` (confirmed by reading `StripLayout::cell`),
                // so a plain `ui.label(text)` would keep truncating to one
                // line no matter what; a `Label`'s own explicit wrap mode
                // overrides that ui-level default.
                row.col(|ui| {
                    ui.add(egui::Label::new(r.kind.label()).wrap());
                });
                row.col(|ui| {
                    ui.add(egui::Label::new(fmt_date(r.created_at)).wrap());
                });
                row.col(|ui| {
                    ui.add(
                        egui::Label::new(
                            r.updated_at
                                .map(fmt_date)
                                .unwrap_or_else(|| "—".to_string()),
                        )
                        .wrap(),
                    );
                });
                row.col(|ui| {
                    // A real `Button`, not a `Label` with `Sense::click()`
                    // — this is the row's only actual navigation control,
                    // and a plain label gave no visual signal it was
                    // clickable at all. `.wrap()` keeps the same
                    // wrap-not-truncate behavior every other cell in this
                    // table already relies on (a clipped `Column` forces
                    // its `Ui`'s *default* wrap mode to `Truncate`, but a
                    // widget's own explicit wrap mode overrides that).
                    let resp = ui
                        .add(egui::Button::new(&r.title).wrap())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if resp.clicked() {
                        navigate = Some(r.navigate_to.clone());
                    }
                });
                row.col(|ui| {
                    ui.add(egui::Label::new(r.episode_title.as_deref().unwrap_or("")).wrap());
                });
                row.col(|ui| {
                    // Shrinks the *clip* rect a few px short of the row's
                    // true bottom edge — not just visual padding, but the
                    // actual paint boundary — so a wrapped cell whose text
                    // happens to fill (or nearly fill) the row's fixed
                    // height doesn't leave its last line's descenders
                    // sitting right on the row divider, reading as if it
                    // overlaps the row below. `set_max_height`/spacing
                    // alone wouldn't do this: `Label` wraps on *width*
                    // only, so it paints every line its text produces
                    // regardless of how much vertical space is "allocated"
                    // around it — only the clip rect actually cuts it off.
                    let mut clip = ui.clip_rect();
                    clip.max.y -= SUMMARY_BOTTOM_GAP;
                    ui.shrink_clip_rect(clip);
                    ui.add(egui::Label::new(r.summary.as_deref().unwrap_or("")).wrap());
                });
                row.col(|ui| {
                    let mut clip = ui.clip_rect();
                    clip.max.y -= SUMMARY_BOTTOM_GAP;
                    ui.shrink_clip_rect(clip);
                    ui.add(egui::Label::new(r.full_summary.as_deref().unwrap_or("")).wrap());
                });
            });
        });

    navigate
}

fn fmt_date(ts_millis: i64) -> String {
    let Some(dt) = Local.timestamp_millis_opt(ts_millis).single() else {
        return String::new();
    };
    format!("{} {}", dt.format("%b %-d, %Y"), dt.format("%-I:%M %p"))
}
