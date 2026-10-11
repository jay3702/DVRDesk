//! Port of `TVShows.tsx`: resizable show list (left) → episode grid or
//! detail pane (right), depending on selection. Selection state
//! (`show_id`/`episode_id`/`filter`) lives in `Screen::TvShows`, not here —
//! matches the plan's §3 navigation model (deep-linkable state belongs on
//! the enum variant, not a local struct). This module only owns fetched
//! data, incremental-render counters, and sort state.
//!
//! Not ported yet: "Mark as Not Recorded" (needs a `DvrFile`/`RuleID`
//! lookup not built in this pass), and the "recorded date" fallback label
//! for episodes missing both season and episode numbers is simplified —
//! falls back to episode title or bare title rather than a formatted date.

use super::{Loaded, SortOrder};
use crate::api::types::{Recording, Show};
use crate::screen::Screen;

const INITIAL_VISIBLE_SHOWS: usize = 90;
const VISIBLE_SHOWS_STEP: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowSortField {
    Title,
    Id,
    DateAdded,
    DateUpdated,
    LastRecorded,
}

impl ShowSortField {
    const ALL: [ShowSortField; 5] = [
        ShowSortField::Title,
        ShowSortField::Id,
        ShowSortField::DateAdded,
        ShowSortField::DateUpdated,
        ShowSortField::LastRecorded,
    ];

    fn label(self) -> &'static str {
        match self {
            ShowSortField::Title => "Title",
            ShowSortField::Id => "ID",
            ShowSortField::DateAdded => "Date Added",
            ShowSortField::DateUpdated => "Date Updated",
            ShowSortField::LastRecorded => "Last Recorded",
        }
    }

    fn default_order(self) -> SortOrder {
        match self {
            ShowSortField::Title => SortOrder::Asc,
            _ => SortOrder::Desc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeSortField {
    SeasonEpisode,
    AirDate,
    DateAdded,
    DateUpdated,
    Title,
    Id,
}

impl EpisodeSortField {
    const ALL: [EpisodeSortField; 6] = [
        EpisodeSortField::SeasonEpisode,
        EpisodeSortField::AirDate,
        EpisodeSortField::DateAdded,
        EpisodeSortField::DateUpdated,
        EpisodeSortField::Title,
        EpisodeSortField::Id,
    ];

    fn label(self) -> &'static str {
        match self {
            EpisodeSortField::SeasonEpisode => "Season / Episode",
            EpisodeSortField::AirDate => "First Aired",
            EpisodeSortField::DateAdded => "Date Added",
            EpisodeSortField::DateUpdated => "Date Updated",
            EpisodeSortField::Title => "Title",
            EpisodeSortField::Id => "ID",
        }
    }

    fn default_order(self) -> SortOrder {
        match self {
            EpisodeSortField::SeasonEpisode
            | EpisodeSortField::AirDate
            | EpisodeSortField::Title => SortOrder::Asc,
            _ => SortOrder::Desc,
        }
    }
}

pub struct TvShowsState {
    pub shows: Loaded<Vec<Show>>,
    pub episodes: Loaded<Vec<Recording>>,
    /// Which show `episodes` actually belongs to — lets the render function
    /// tell "stale data for the previous show, a fetch is in flight" apart
    /// from "ready data for the currently-selected show."
    pub episodes_show_id: Option<String>,
    pub visible_shows: usize,
    pub show_sort: ShowSortField,
    pub show_sort_order: SortOrder,
    pub episode_sort: EpisodeSortField,
    pub episode_sort_order: SortOrder,
    /// Multi-selected episodes, for the bulk actions.
    pub selection: Selection,
}

/// Episode multi-select state — its own struct so the helpers that change
/// it can borrow it alongside the episode list.
#[derive(Default)]
pub struct Selection {
    pub checked: std::collections::HashSet<String>,
    /// The show `checked` belongs to; cleared when another is opened.
    show: Option<String>,
    /// Where a Shift-click range starts: the last card toggled.
    anchor: Option<String>,
    /// Episodes waiting on the Trash confirmation: `(id, label)`.
    confirm_trash: Option<Vec<(String, String)>>,
}

impl Default for TvShowsState {
    fn default() -> Self {
        Self {
            shows: Loaded::Idle,
            episodes: Loaded::Idle,
            episodes_show_id: None,
            visible_shows: INITIAL_VISIBLE_SHOWS,
            show_sort: ShowSortField::LastRecorded,
            show_sort_order: ShowSortField::LastRecorded.default_order(),
            episode_sort: EpisodeSortField::DateAdded,
            episode_sort_order: EpisodeSortField::DateAdded.default_order(),
            selection: Selection::default(),
        }
    }
}

#[derive(Default)]
pub struct NavAction {
    pub new_screen: Option<Screen>,
    pub play: Option<Recording>,
    /// `(id, new_watched_value)` — App applies the optimistic flip and
    /// fires the mutation; this module doesn't own the API/async layer.
    pub toggle_watched: Option<(String, bool)>,
    pub trash: Option<String>,
    pub download: Option<Recording>,
    /// Details / Debug Log clicked on the detail page, for this recording.
    pub file_info: Option<(super::recording_detail::FileInfo, Recording)>,
    /// Bulk Mark Watched / Unwatched: `(ids, watched)`.
    pub set_watched_many: Option<(Vec<String>, bool)>,
    /// Bulk Trash, already confirmed.
    pub trash_many: Option<Vec<String>>,
}

fn ep_label(rec: &Recording) -> String {
    match (rec.season_number, rec.episode_number) {
        (Some(s), Some(e)) => format!("S{s}E{e}"),
        (Some(s), None) => format!("S{s}"),
        (None, Some(e)) => format!("E{e}"),
        (None, None) => String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    state: &mut TvShowsState,
    show_id: Option<&str>,
    episode_id: Option<&str>,
    filter: Option<&str>,
    server_url: &str,
    file_checks: &crate::file_check::FileChecks,
) -> NavAction {
    let mut action = NavAction::default();

    let shows = match &state.shows {
        Loaded::Idle | Loaded::Loading => {
            ui.label("Loading shows…");
            return action;
        }
        Loaded::Err(e) => {
            ui.colored_label(egui::Color32::RED, e);
            return action;
        }
        Loaded::Ready(shows) => shows,
    };

    egui::SidePanel::left("tv_shows_list")
        .resizable(true)
        .default_width(320.0)
        .show_inside(ui, |ui| {
            // `horizontal_wrapped`, not `horizontal` — see the matching
            // comment in `movies.rs` for why (avoids feeding this row's
            // full natural width back into the `SidePanel`'s stored width
            // on a narrower panel).
            ui.horizontal_wrapped(|ui| {
                ui.heading("TV Shows");
                ui.separator();
                egui::ComboBox::from_id_salt("show_sort_field")
                    .selected_text(state.show_sort.label())
                    .show_ui(ui, |ui| {
                        for field in ShowSortField::ALL {
                            if ui
                                .selectable_label(state.show_sort == field, field.label())
                                .clicked()
                                && state.show_sort != field
                            {
                                state.show_sort = field;
                                state.show_sort_order = field.default_order();
                                state.visible_shows = INITIAL_VISIBLE_SHOWS;
                            }
                        }
                    });
                if super::order_buttons(ui, &mut state.show_sort_order) {
                    state.visible_shows = INITIAL_VISIBLE_SHOWS;
                }
            });

            let mut sorted_shows: Vec<&Show> = shows.iter().collect();
            match state.show_sort {
                ShowSortField::Title => {
                    sorted_shows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
                ShowSortField::Id => sorted_shows.sort_by(|a, b| a.id.cmp(&b.id)),
                ShowSortField::DateAdded => sorted_shows.sort_by_key(|s| s.created_at),
                ShowSortField::DateUpdated => sorted_shows.sort_by_key(|s| s.updated_at),
                ShowSortField::LastRecorded => sorted_shows.sort_by_key(|s| s.last_recorded_at),
            }
            if state.show_sort_order == SortOrder::Desc {
                sorted_shows.reverse();
            }

            // See the matching comment in `recent.rs` — without this, the
            // SidePanel's stored width silently gets overwritten by the
            // show list's natural content width every frame instead of
            // wherever the user actually drags the divider to.
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for s in sorted_shows.iter().take(state.visible_shows) {
                        let is_selected = show_id == Some(s.id.as_str());
                        let label = format!("{} ({})", s.name, s.episode_count);
                        let bg = super::row_background(ui);
                        let row = ui.horizontal(|ui| {
                            super::thumb::show(
                                ui,
                                server_url,
                                s.image_url.as_deref(),
                                egui::vec2(48.0, 64.0),
                            );
                            super::selectable_truncated_label(ui, is_selected, &label);
                        });
                        let clicked =
                            super::row_click(ui, bg, row.response.rect, ("tv_show_row", &s.id))
                                .clicked();
                        if clicked {
                            action.new_screen = Some(Screen::TvShows {
                                show_id: Some(s.id.clone()),
                                episode_id: None,
                                filter: filter.map(str::to_string),
                            });
                        }
                    }
                    if state.visible_shows < sorted_shows.len() && ui.button("Load more").clicked()
                    {
                        state.visible_shows += VISIBLE_SHOWS_STEP;
                    }
                });
        });

    let Some(sid) = show_id else {
        ui.weak("Select a show to see its episodes.");
        return action;
    };

    let episodes = match &state.episodes {
        _ if state.episodes_show_id.as_deref() != Some(sid) => {
            ui.label("Loading episodes…");
            return action;
        }
        Loaded::Idle | Loaded::Loading => {
            ui.label("Loading episodes…");
            return action;
        }
        Loaded::Err(e) => {
            ui.colored_label(egui::Color32::RED, e);
            return action;
        }
        Loaded::Ready(episodes) => episodes,
    };

    if state.selection.show.as_deref() != Some(sid) {
        state.selection = Selection {
            show: Some(sid.to_string()),
            ..Selection::default()
        };
    }
    // Trashed episodes drop out of `episodes`; drop them from the selection.
    state
        .selection
        .checked
        .retain(|id| episodes.iter().any(|e| &e.id == id));

    let unwatched_only = filter == Some("unwatched");
    ui.horizontal(|ui| {
        ui.label("Filter:");
        if ui.selectable_label(!unwatched_only, "All").clicked() {
            action.new_screen = Some(Screen::TvShows {
                show_id: Some(sid.to_string()),
                episode_id: episode_id.map(str::to_string),
                filter: None,
            });
        }
        if ui.selectable_label(unwatched_only, "Unwatched").clicked() {
            action.new_screen = Some(Screen::TvShows {
                show_id: Some(sid.to_string()),
                episode_id: episode_id.map(str::to_string),
                filter: Some("unwatched".to_string()),
            });
        }
        ui.separator();
        egui::ComboBox::from_id_salt("episode_sort_field")
            .selected_text(state.episode_sort.label())
            .show_ui(ui, |ui| {
                for field in EpisodeSortField::ALL {
                    if ui
                        .selectable_label(state.episode_sort == field, field.label())
                        .clicked()
                        && state.episode_sort != field
                    {
                        state.episode_sort = field;
                        state.episode_sort_order = field.default_order();
                    }
                }
            });
        super::order_buttons(ui, &mut state.episode_sort_order);
    });

    let mut visible: Vec<&Recording> = episodes
        .iter()
        .filter(|e| !unwatched_only || !e.watched)
        .collect();
    match state.episode_sort {
        EpisodeSortField::Title => visible.sort_by(|a, b| {
            let ak = a
                .episode_title
                .as_deref()
                .unwrap_or(&a.title)
                .to_lowercase();
            let bk = b
                .episode_title
                .as_deref()
                .unwrap_or(&b.title)
                .to_lowercase();
            ak.cmp(&bk)
        }),
        EpisodeSortField::Id => visible.sort_by(|a, b| a.id.cmp(&b.id)),
        EpisodeSortField::DateAdded => visible.sort_by_key(|e| e.created_at),
        EpisodeSortField::DateUpdated => visible.sort_by_key(|e| e.updated_at),
        EpisodeSortField::SeasonEpisode => {
            visible.sort_by_key(|e| (e.season_number.unwrap_or(0), e.episode_number.unwrap_or(0)))
        }
        EpisodeSortField::AirDate => visible.sort_by(|a, b| {
            let ad = a.original_air_date.as_deref().unwrap_or("");
            let bd = b.original_air_date.as_deref().unwrap_or("");
            match (ad.is_empty(), bd.is_empty()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => ad.cmp(bd),
            }
        }),
    }
    if state.episode_sort_order == SortOrder::Desc {
        visible.reverse();
    }

    let selected_episode = episode_id.and_then(|eid| visible.iter().find(|e| e.id == eid).copied());

    if let Some(ep) = selected_episode {
        if ui.button("◀ Back to episodes").clicked() {
            action.new_screen = Some(Screen::TvShows {
                show_id: Some(sid.to_string()),
                episode_id: None,
                filter: filter.map(str::to_string),
            });
        }
        let detail = super::recording_detail::show(ui, ep, server_url, file_checks);
        if detail.play {
            action.play = Some(ep.clone());
        }
        if let Some(watched) = detail.toggle_watched {
            action.toggle_watched = Some((ep.id.clone(), watched));
        }
        if detail.trash {
            action.trash = Some(ep.id.clone());
        }
        if detail.download {
            action.download = Some(ep.clone());
        }
        if let Some(kind) = detail.file_info {
            action.file_info = Some((kind, ep.clone()));
        }
    } else {
        // Left margin: this branch renders directly in the CentralPanel's
        // remaining space, not inside the `tv_shows_list` SidePanel, so its
        // content sits in the same "steals the resize handle" zone
        // `recording_detail::show` has to guard against — see its own doc
        // comment for the underlying egui mechanism.
        egui::Frame::default()
            .inner_margin(egui::Margin {
                left: 12.0,
                ..egui::Margin::ZERO
            })
            .show(ui, |ui| {
                if let Some(show) = shows.iter().find(|s| s.id == sid) {
                    if show.image_url.is_some() || show.summary.is_some() {
                        const IMAGE_COL_WIDTH: f32 = 160.0;
                        ui.horizontal(|ui| {
                            super::thumb::show_max_width(
                                ui,
                                server_url,
                                show.image_url.as_deref(),
                                IMAGE_COL_WIDTH,
                            );
                            if let Some(summary) = &show.summary {
                                // Constrained to the remaining width and
                                // wrapped explicitly — a plain
                                // `ui.label()` here doesn't wrap on its
                                // own inside a horizontal layout, it just
                                // runs off the edge of the panel.
                                let text_width = (ui.available_width() - 12.0).max(120.0);
                                ui.allocate_ui(egui::vec2(text_width, 0.0), |ui| {
                                    ui.add(egui::Label::new(summary).wrap());
                                });
                            }
                        });
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);
                    }
                }
                let selection = &mut state.selection;
                let file_statuses = scan_files(ui, &visible, server_url, file_checks, selection);
                selection_bar(ui, &visible, selection, &mut action);
                ui.add_space(6.0);

                let selecting = !selection.checked.is_empty();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let cols = super::media_card::columns_for_width(ui.available_width());
                    egui::Grid::new("episode_grid")
                        .num_columns(cols)
                        .spacing([14.0, 14.0])
                        .show(ui, |ui| {
                            for (i, ep) in visible.iter().enumerate() {
                                let label = ep_label(ep);
                                let title = if label.is_empty() {
                                    ep.title.clone()
                                } else {
                                    label
                                };
                                let subtitle = ep.episode_title.as_deref();
                                let extras = super::media_card::CardExtras {
                                    selectable: true,
                                    checked: selection.checked.contains(&ep.id),
                                    selecting,
                                    file_status: file_statuses.get(ep.id.as_str()).cloned(),
                                };
                                let card_action = super::media_card::show(
                                    ui, server_url, ep, &title, subtitle, false, &extras,
                                );
                                let modifiers = ui.input(|i| i.modifiers);
                                if card_action.check_clicked
                                    || (card_action.clicked && modifiers.command)
                                    || (card_action.clicked && selecting && !modifiers.shift)
                                {
                                    // While selecting, a plain click toggles
                                    // too, rather than leaving the grid.
                                    toggle_checked(selection, &ep.id);
                                } else if card_action.clicked && modifiers.shift {
                                    select_range(selection, &visible, &ep.id);
                                } else if card_action.double_clicked && !selecting {
                                    action.play = Some((*ep).clone());
                                } else if card_action.clicked && !selecting {
                                    action.new_screen = Some(Screen::TvShows {
                                        show_id: Some(sid.to_string()),
                                        episode_id: Some(ep.id.clone()),
                                        filter: filter.map(str::to_string),
                                    });
                                }
                                if card_action.download_clicked {
                                    action.download = Some((*ep).clone());
                                }
                                if (i + 1) % cols == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                });
            });
    }

    action
}

fn toggle_checked(state: &mut Selection, id: &str) {
    if !state.checked.remove(id) {
        state.checked.insert(id.to_string());
    }
    state.anchor = Some(id.to_string());
}

/// Shift-click: selects every episode between the anchor (the last card
/// toggled) and this one, in the grid's current order.
fn select_range(state: &mut Selection, visible: &[&Recording], id: &str) {
    let end = visible.iter().position(|e| e.id == id);
    let start = state
        .anchor
        .as_deref()
        .and_then(|a| visible.iter().position(|e| e.id == a))
        .or(end);
    if let (Some(start), Some(end)) = (start, end) {
        let (lo, hi) = (start.min(end), start.max(end));
        for e in &visible[lo..=hi] {
            state.checked.insert(e.id.clone());
        }
    }
    state.anchor = Some(id.to_string());
}

/// Checks every completed episode in the grid against the DVR (see
/// `crate::file_check`; results are cached, so this is cheap per frame)
/// and shows a status line: progress while checking, then how many files
/// are missing or empty, with a button to select those for cleanup.
/// Returns each episode's status for its card.
fn scan_files<'a>(
    ui: &mut egui::Ui,
    visible: &[&'a Recording],
    server_url: &str,
    file_checks: &crate::file_check::FileChecks,
    state: &mut Selection,
) -> std::collections::HashMap<&'a str, crate::file_check::FileStatus> {
    use crate::file_check::FileStatus;

    let statuses: std::collections::HashMap<&str, FileStatus> = visible
        .iter()
        .filter(|e| e.completed)
        .map(|e| (e.id.as_str(), file_checks.status(server_url, &e.id)))
        .collect();
    let checking = statuses
        .values()
        .filter(|s| **s == FileStatus::Checking)
        .count();
    let problems: Vec<&str> = visible
        .iter()
        .filter(|e| {
            e.too_short()
                || matches!(
                    statuses.get(e.id.as_str()),
                    Some(FileStatus::Missing | FileStatus::Empty)
                )
        })
        .map(|e| e.id.as_str())
        .collect();

    ui.horizontal(|ui| {
        if checking > 0 {
            ui.spinner();
            ui.label(format!(
                "Checking files on the DVR: {} of {} done",
                statuses.len() - checking,
                statuses.len()
            ));
        }
        if !problems.is_empty() {
            let missing = statuses
                .values()
                .filter(|s| **s == FileStatus::Missing)
                .count();
            let others = problems.len() - missing;
            let mut parts = Vec::new();
            if missing > 0 {
                parts.push(format!("{missing} missing from disk"));
            }
            if others > 0 {
                parts.push(format!("{others} empty or too short to play"));
            }
            ui.colored_label(ui.visuals().warn_fg_color, parts.join(", "));
            if ui
                .button("Select these")
                .on_hover_text("Select every episode that can't be played, for cleanup")
                .clicked()
            {
                state
                    .checked
                    .extend(problems.iter().map(|id| id.to_string()));
            }
        } else if checking == 0 && !statuses.is_empty() {
            ui.weak("All files present on the DVR.");
        }
    });
    statuses
}

/// "N selected" and the bulk actions, plus the Trash confirmation. Shows a
/// hint instead while nothing is selected.
fn selection_bar(
    ui: &mut egui::Ui,
    visible: &[&Recording],
    state: &mut Selection,
    action: &mut NavAction,
) {
    // Only what's on screen stays selected — with the Unwatched filter on,
    // marking episodes watched hides them, and they shouldn't stay counted.
    state
        .checked
        .retain(|id| visible.iter().any(|e| &e.id == id));
    if !state.checked.is_empty() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.checked.clear();
    }
    ui.horizontal_wrapped(|ui| {
        if state.checked.is_empty() {
            ui.weak("Ctrl-click or check episodes to select several.");
            if ui.small_button("Select all").clicked() {
                state.checked.extend(visible.iter().map(|e| e.id.clone()));
            }
            return;
        }
        // In grid order, so actions and the confirmation list read top-down.
        let selected: Vec<&Recording> = visible
            .iter()
            .filter(|e| state.checked.contains(&e.id))
            .copied()
            .collect();
        ui.strong(format!("{} selected", state.checked.len()));
        if ui.button("Select all").clicked() {
            state.checked.extend(visible.iter().map(|e| e.id.clone()));
        }
        let ids = || state.checked.iter().cloned().collect::<Vec<_>>();
        if ui.button("✔ Mark Watched").clicked() {
            action.set_watched_many = Some((ids(), true));
        }
        if ui.button("↺ Mark Unwatched").clicked() {
            action.set_watched_many = Some((ids(), false));
        }
        if ui.button("🗑 Trash…").clicked() {
            state.confirm_trash = Some(
                selected
                    .iter()
                    .map(|e| (e.id.clone(), episode_label(e)))
                    .collect(),
            );
        }
        if ui.button("Clear").on_hover_text("Esc").clicked() {
            state.checked.clear();
        }
    });

    let Some(pending) = &state.confirm_trash else {
        return;
    };
    let mut confirmed = false;
    let mut cancelled = false;
    egui::Window::new(format!("Trash {} recordings?", pending.len()))
        .id(egui::Id::new("tv_trash_confirm"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            ui.label("They'll be moved to the DVR's trash, where they can still be restored.");
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for (_, label) in pending {
                        ui.label(label);
                    }
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button(
                        egui::RichText::new(format!("Trash {}", pending.len()))
                            .color(ui.visuals().error_fg_color),
                    )
                    .clicked()
                {
                    confirmed = true;
                }
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
        });
    if confirmed {
        action.trash_many = Some(pending.iter().map(|(id, _)| id.clone()).collect());
        state.checked.clear();
        state.confirm_trash = None;
    } else if cancelled {
        state.confirm_trash = None;
    }
}

/// "S1E4 Episode Title", or whatever parts exist.
fn episode_label(rec: &Recording) -> String {
    let number = ep_label(rec);
    match (number.is_empty(), rec.episode_title.as_deref()) {
        (false, Some(t)) => format!("{number} {t}"),
        (false, None) => number,
        (true, Some(t)) => t.to_string(),
        (true, None) => rec.title.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str) -> Recording {
        serde_json::from_value(serde_json::json!({"id": id, "title": "T", "created_at": 0}))
            .unwrap()
    }

    #[test]
    fn toggle_then_shift_selects_range_in_grid_order() {
        let recs: Vec<Recording> = ["a", "b", "c", "d", "e"].iter().map(|i| rec(i)).collect();
        let visible: Vec<&Recording> = recs.iter().collect();
        let mut sel = Selection::default();

        toggle_checked(&mut sel, "b");
        select_range(&mut sel, &visible, "d");
        let mut got: Vec<_> = sel.checked.iter().cloned().collect();
        got.sort();
        assert_eq!(got, ["b", "c", "d"]);

        // Backwards from the new anchor (d) to a.
        select_range(&mut sel, &visible, "a");
        assert_eq!(sel.checked.len(), 4);

        toggle_checked(&mut sel, "c");
        assert!(!sel.checked.contains("c"));
    }

    #[test]
    fn too_short_means_completed_and_under_a_minute() {
        let mut r = rec("x");
        r.completed = true;
        r.duration = 1.3;
        assert!(r.too_short());
        r.duration = 3659.8;
        assert!(!r.too_short());
        r.duration = 0.0;
        r.completed = false; // still recording
        assert!(!r.too_short());
    }

    #[test]
    fn shift_without_anchor_selects_just_that_one() {
        let recs: Vec<Recording> = ["a", "b"].iter().map(|i| rec(i)).collect();
        let visible: Vec<&Recording> = recs.iter().collect();
        let mut sel = Selection::default();
        select_range(&mut sel, &visible, "b");
        assert_eq!(sel.checked.iter().collect::<Vec<_>>(), ["b"]);
    }
}
