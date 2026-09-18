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
                            super::row_click(ui, row.response.rect, ("tv_show_row", &s.id))
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
        let detail = super::recording_detail::show(ui, ep, server_url);
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
                                let card_action = super::media_card::show(
                                    ui, server_url, ep, &title, subtitle, false,
                                );
                                if card_action.double_clicked {
                                    action.play = Some((*ep).clone());
                                } else if card_action.clicked {
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
