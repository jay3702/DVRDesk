//! Port of `Movies.tsx`. `movie_id` selection lives in `Screen::Movies`
//! (deep-linkable, per the plan's §3 model) — but the watched filter is
//! local state here, not deep-linked, matching the old app: unlike TV
//! Shows, Movies never put its filter in a URL search param either.
//!
//! Sort controls match TV Shows' show-list pattern exactly (same
//! `SortOrder`/`order_buttons` from `ui/mod.rs`), field set adapted to
//! what `Recording` actually has for a movie (no season/episode, so no
//! season/episode-shaped field here the way TV Shows' *episode* sort has).
//!
//! Not ported yet: the generic attribute-dump the old app's inline detail
//! view had — this reuses the shared `recording_detail` component instead,
//! same as Recent/TV Shows, rather than Movies' own bespoke detail layout.
//! List rows use the shared `media_card` component (thumbnail, badges,
//! progress bar), same as TV Shows' episode grid.

use super::{Loaded, SortOrder};
use crate::api::types::Recording;
use crate::screen::Screen;

const INITIAL_VISIBLE: usize = 120;
const VISIBLE_STEP: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovieSortField {
    Title,
    Id,
    DateAdded,
    DateUpdated,
}

impl MovieSortField {
    const ALL: [MovieSortField; 4] = [
        MovieSortField::Title,
        MovieSortField::Id,
        MovieSortField::DateAdded,
        MovieSortField::DateUpdated,
    ];

    fn label(self) -> &'static str {
        match self {
            MovieSortField::Title => "Title",
            MovieSortField::Id => "ID",
            MovieSortField::DateAdded => "Date Added",
            MovieSortField::DateUpdated => "Date Updated",
        }
    }

    fn default_order(self) -> SortOrder {
        match self {
            MovieSortField::Title => SortOrder::Asc,
            _ => SortOrder::Desc,
        }
    }
}

pub struct MoviesState {
    pub items: Loaded<Vec<Recording>>,
    pub visible: usize,
    pub unwatched_only: bool,
    pub sort: MovieSortField,
    pub sort_order: SortOrder,
}

impl Default for MoviesState {
    fn default() -> Self {
        Self {
            items: Loaded::Idle,
            visible: INITIAL_VISIBLE,
            unwatched_only: false,
            sort: MovieSortField::DateAdded,
            sort_order: MovieSortField::DateAdded.default_order(),
        }
    }
}

#[derive(Default)]
pub struct MoviesAction {
    pub new_screen: Option<Screen>,
    pub play: Option<Recording>,
    pub toggle_watched: Option<(String, bool)>,
    pub trash: Option<String>,
    pub download: Option<Recording>,
}

/// Keyboard nav relies on egui's own Tab-focus system — see `ui::recent`'s
/// `show()` doc comment for why a custom arrow-key scheme was reverted.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut MoviesState,
    movie_id: Option<&str>,
    server_url: &str,
) -> MoviesAction {
    let mut action = MoviesAction::default();

    let movies = match &state.items {
        Loaded::Idle | Loaded::Loading => {
            ui.label("Loading movies…");
            return action;
        }
        Loaded::Err(e) => {
            ui.colored_label(egui::Color32::RED, e);
            return action;
        }
        Loaded::Ready(m) => m,
    };

    egui::SidePanel::left("movies_list")
        .resizable(true)
        // Wider than the other list panels' 300-360 default — this one's
        // toolbar row packs title + filter + sort together (per request),
        // and needs the extra room to actually fit on one line rather than
        // wrapping to a second immediately at the old default.
        .default_width(420.0)
        .show_inside(ui, |ui| {
            // `horizontal_wrapped`, not `horizontal` — this row now packs
            // title + filter + sort into one line, which the panel's own
            // default width can't always fit; wrapping to a second line
            // when it doesn't avoids feeding the row's full natural width
            // back into the `SidePanel`'s stored width (the exact
            // resize-creep bug root-caused earlier this session for list
            // rows, here for a toolbar row instead).
            ui.horizontal_wrapped(|ui| {
                ui.heading("Movies");
                ui.separator();
                ui.label("Filter:");
                if ui.selectable_label(!state.unwatched_only, "All").clicked() {
                    state.unwatched_only = false;
                }
                if ui
                    .selectable_label(state.unwatched_only, "Unwatched")
                    .clicked()
                {
                    state.unwatched_only = true;
                }
                ui.separator();
                egui::ComboBox::from_id_salt("movie_sort_field")
                    .selected_text(state.sort.label())
                    .show_ui(ui, |ui| {
                        for field in MovieSortField::ALL {
                            if ui
                                .selectable_label(state.sort == field, field.label())
                                .clicked()
                                && state.sort != field
                            {
                                state.sort = field;
                                state.sort_order = field.default_order();
                                state.visible = INITIAL_VISIBLE;
                            }
                        }
                    });
                if super::order_buttons(ui, &mut state.sort_order) {
                    state.visible = INITIAL_VISIBLE;
                }
            });
            ui.separator();

            let mut visible_movies: Vec<&Recording> = movies
                .iter()
                .filter(|m| !state.unwatched_only || !m.watched)
                .collect();
            match state.sort {
                MovieSortField::Title => visible_movies
                    .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
                MovieSortField::Id => visible_movies.sort_by(|a, b| a.id.cmp(&b.id)),
                MovieSortField::DateAdded => visible_movies.sort_by_key(|m| m.created_at),
                MovieSortField::DateUpdated => visible_movies.sort_by_key(|m| m.updated_at),
            }
            if state.sort_order == SortOrder::Desc {
                visible_movies.reverse();
            }

            // See the matching comment in `recent.rs` — this one happened
            // to look correct by coincidence (the card grid's own natural
            // width tracks upward with more available space too, since
            // more columns fit), but it's the same underlying bug: without
            // this, the panel's stored width comes from the grid's actual
            // content bounds, not from wherever the divider was dragged to.
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let cols = super::media_card::columns_for_width(ui.available_width());
                    egui::Grid::new("movies_grid")
                        .num_columns(cols)
                        .spacing([14.0, 14.0])
                        .show(ui, |ui| {
                            for (i, m) in visible_movies.iter().take(state.visible).enumerate() {
                                let is_selected = movie_id == Some(m.id.as_str());
                                let card_action = super::media_card::show(
                                    ui,
                                    server_url,
                                    m,
                                    &m.title,
                                    None,
                                    is_selected,
                                );
                                if card_action.double_clicked {
                                    action.play = Some((*m).clone());
                                } else if card_action.clicked {
                                    action.new_screen = Some(Screen::Movies {
                                        movie_id: Some(m.id.clone()),
                                    });
                                }
                                if card_action.download_clicked {
                                    action.download = Some((*m).clone());
                                }
                                if (i + 1) % cols == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                    if state.visible < visible_movies.len() && ui.button("Load more").clicked() {
                        state.visible += VISIBLE_STEP;
                    }
                });
        });

    match movie_id.and_then(|id| movies.iter().find(|m| m.id == id)) {
        Some(m) => {
            let detail = super::recording_detail::show(ui, m, server_url);
            if detail.play {
                action.play = Some(m.clone());
            }
            if let Some(watched) = detail.toggle_watched {
                action.toggle_watched = Some((m.id.clone(), watched));
            }
            if detail.trash {
                action.trash = Some(m.id.clone());
            }
            if detail.download {
                action.download = Some(m.clone());
            }
        }
        None => {
            ui.add_space(8.0);
            ui.weak("Select a movie to see details.");
        }
    }

    action
}
