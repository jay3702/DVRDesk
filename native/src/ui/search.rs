//! Port of `Search.tsx` — fully client-side keyword search across shows,
//! episodes, movies, and videos, all loaded up front (no server-side search
//! endpoint exists). Search only triggers on submit, not live-as-you-type,
//! matching the old app.
//!
//! Also matches the TV guide (the same channels/guide data the Live screen
//! loads): programs airing now or later, shown as "On TV" rows that open
//! the guide scrolled to that program.
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
    /// Include "On TV" matches from the guide.
    pub include_guide: bool,
}

/// Where clicking a result goes.
#[derive(Debug, Clone)]
pub enum SearchTarget {
    Screen(Screen),
    /// The Live guide, scrolled to and highlighting this airing.
    Guide(crate::ui::live::GuideJump),
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
            include_guide: true,
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
    Guide,
}

impl ResultKind {
    fn rank(self) -> u8 {
        match self {
            ResultKind::Show => 0,
            ResultKind::Episode => 1,
            ResultKind::Movie => 2,
            ResultKind::Video => 3,
            ResultKind::Guide => 4,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ResultKind::Show => "TV Series",
            ResultKind::Episode => "TV Episode",
            ResultKind::Movie => "Movie",
            ResultKind::Video => "Video",
            ResultKind::Guide => "On TV",
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
    navigate_to: SearchTarget,
    /// "Ch 5 KHNL", for On TV rows — shown under the type.
    channel: Option<String>,
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

/// "On TV" matches: guide programs airing now or later whose title (or
/// description, by match type) contains the keyword. Future-only, the same
/// rule the guide's own search uses. `created_at` carries the air time, in
/// milliseconds like the library rows, for the date column.
fn guide_results(
    state: &SearchState,
    live: &crate::ui::live::LiveState,
    needle: &str,
) -> Vec<ResultRow> {
    let now = chrono::Utc::now().timestamp();
    let Loaded::Ready(guide) = &live.guide else {
        return Vec::new();
    };
    let channel_names: HashMap<&str, &str> = match &live.channels {
        Loaded::Ready(channels) => channels
            .iter()
            .map(|c| (c.number.as_str(), c.name.as_str()))
            .collect(),
        _ => HashMap::new(),
    };
    let mut rows: Vec<ResultRow> = guide
        .iter()
        .filter(|p| p.stop > now)
        .filter(|p| {
            let in_title = contains_ci(&p.title, needle);
            let in_desc = || p.desc.as_deref().is_some_and(|d| contains_ci(d, needle));
            match state.search_type {
                SearchType::Title | SearchType::SeriesName => in_title,
                SearchType::Summary => in_desc(),
                SearchType::Any => in_title || in_desc(),
            }
        })
        .map(|p| ResultRow {
            kind: ResultKind::Guide,
            title: p.title.clone(),
            episode_title: None,
            summary: p.desc.clone(),
            full_summary: None,
            created_at: p.start * 1000,
            updated_at: None,
            navigate_to: SearchTarget::Guide(crate::ui::live::GuideJump {
                channel: p.channel.clone(),
                start: p.start,
                title: p.title.clone(),
            }),
            channel: Some(match channel_names.get(p.channel.as_str()) {
                Some(name) => format!("Ch {} {name}", p.channel),
                None => format!("Ch {}", p.channel),
            }),
        })
        .collect();
    rows.sort_by_key(|r| r.created_at);
    rows
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
                    navigate_to: SearchTarget::Screen(Screen::TvShows {
                        show_id: Some(s.id.clone()),
                        episode_id: None,
                        filter: None,
                    }),
                    channel: None,
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
                navigate_to: SearchTarget::Screen(Screen::TvShows {
                    show_id: e.show_id.clone(),
                    episode_id: Some(e.id.clone()),
                    filter: None,
                }),
                channel: None,
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
                    navigate_to: SearchTarget::Screen(Screen::Movies {
                        movie_id: Some(m.id.clone()),
                    }),
                    channel: None,
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
                    navigate_to: SearchTarget::Screen(Screen::Library {
                        group_id: v.video_group_id.clone(),
                        video_id: Some(v.id.clone()),
                    }),
                    channel: None,
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
/// `live`: the Live screen's channels and guide, for "On TV" matches.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut SearchState,
    live: &crate::ui::live::LiveState,
) -> Option<SearchTarget> {
    let mut navigate: Option<SearchTarget> = None;

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
        ui.checkbox(&mut state.include_guide, "Include TV Guide")
            .on_hover_text("Programs airing now or later, from the guide");
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
    let guide_loading = state.include_guide && matches!(live.guide, Loaded::Idle | Loaded::Loading);
    if state.include_guide {
        results.extend(guide_results(state, live, state.submitted_keyword.trim()));
    }
    if guide_loading {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak("Loading the TV guide…");
        });
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
            // On TV rows show the channel on a second line under the type.
            let min_h = if r.channel.is_some() {
                single_line_height + ui.fonts(|f| f.row_height(&font_id))
            } else {
                single_line_height
            };
            (summary_h.max(full_h) + CELL_TOP_PAD + SUMMARY_BOTTOM_GAP).max(min_h)
        })
        .collect();

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        // Click-sensing cells are what turn on egui_extras' own hover
        // highlight for the whole row; a click anywhere on it opens the
        // result, same as its title button.
        .sense(egui::Sense::click())
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
                    if let Some(channel) = &r.channel {
                        ui.add(egui::Label::new(egui::RichText::new(channel).weak()).wrap());
                    }
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
                if row.response().clicked() {
                    navigate = Some(r.navigate_to.clone());
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::guide::GuideProgram;

    fn program(channel: &str, start_offset: i64, title: &str, desc: &str) -> GuideProgram {
        let now = chrono::Utc::now().timestamp();
        GuideProgram {
            channel: channel.to_string(),
            start: now + start_offset,
            stop: now + start_offset + 1800,
            title: title.to_string(),
            desc: Some(desc.to_string()),
            categories: Vec::new(),
            image: None,
            series_id: None,
            program_id: None,
            is_new: false,
        }
    }

    fn search(keyword: &str, search_type: SearchType, guide: Vec<GuideProgram>) -> Vec<ResultRow> {
        let state = SearchState {
            submitted_keyword: keyword.to_string(),
            search_type,
            ..SearchState::default()
        };
        let live = crate::ui::live::LiveState {
            guide: Loaded::Ready(guide),
            ..Default::default()
        };
        guide_results(&state, &live, keyword)
    }

    #[test]
    fn guide_matches_are_current_or_future_and_soonest_first() {
        let rows = search(
            "dateline",
            SearchType::Any,
            vec![
                program("5", 7200, "Dateline NBC", ""),
                program("5", -7200, "Dateline NBC", ""), // already over
                program("9", -600, "Dateline on ID", ""), // airing now
                program("2", 3600, "Jeopardy!", "Dateline champions"),
            ],
        );
        let got: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r.title.as_str(), r.channel.as_deref().unwrap()))
            .collect();
        assert_eq!(
            got,
            [
                ("Dateline on ID", "Ch 9"),
                ("Jeopardy!", "Ch 2"),
                ("Dateline NBC", "Ch 5")
            ]
        );
        assert!(matches!(
            &rows[0].navigate_to,
            SearchTarget::Guide(j) if j.channel == "9"
        ));
    }

    #[test]
    fn guide_match_type_picks_title_or_description() {
        let guide = || vec![program("2", 3600, "Jeopardy!", "Dateline champions")];
        assert!(search("dateline", SearchType::Title, guide()).is_empty());
        assert_eq!(search("dateline", SearchType::Summary, guide()).len(), 1);
    }
}
