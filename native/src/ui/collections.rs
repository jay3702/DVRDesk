//! Channels DVR's "Library Collections" — a server-side, customizable
//! grouping of recordings (movies/shows), not to be confused with the old
//! "Channel Collections" filter this app briefly had in the Live guide grid
//! (channel groupings, removed — see `api/collections.rs`'s module doc
//! comment for the full distinction). Two-level list, mirroring
//! `library.rs`'s group→videos shape: a collection list on the left, that
//! collection's shows in the center once one is selected.

use super::Loaded;
use crate::api::collections::LibraryCollection;
use crate::api::types::Show;
use crate::screen::Screen;

pub struct CollectionsState {
    pub collections: Loaded<Vec<LibraryCollection>>,
    pub shows: Loaded<Vec<Show>>,
    /// Which collection `shows` actually belongs to — same staleness guard
    /// `LibraryState::videos_group_id` uses, so a fetch in flight for a
    /// newly-selected collection isn't mistaken for stale data belonging to
    /// the previous one.
    pub shows_collection_id: Option<String>,
}

impl Default for CollectionsState {
    fn default() -> Self {
        Self {
            collections: Loaded::Idle,
            shows: Loaded::Idle,
            shows_collection_id: None,
        }
    }
}

#[derive(Default)]
pub struct CollectionsAction {
    pub new_screen: Option<Screen>,
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut CollectionsState,
    collection_id: Option<&str>,
    server_url: &str,
) -> CollectionsAction {
    let mut action = CollectionsAction::default();

    let collections = match &state.collections {
        Loaded::Idle | Loaded::Loading => {
            ui.label("Loading collections…");
            return action;
        }
        Loaded::Err(e) => {
            ui.colored_label(egui::Color32::RED, e);
            return action;
        }
        Loaded::Ready(c) => c,
    };

    egui::SidePanel::left("collections_list")
        .resizable(true)
        .default_width(280.0)
        .show_inside(ui, |ui| {
            ui.heading("Collections");
            ui.separator();

            if collections.is_empty() {
                ui.weak("No collections configured on this server.");
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for c in collections {
                        let is_selected = collection_id == Some(c.id.as_str());
                        let label = format!("{} ({})", c.name, c.content_count);
                        if super::selectable_truncated_label(ui, is_selected, &label).clicked() {
                            action.new_screen = Some(Screen::Collections {
                                collection_id: Some(c.id.clone()),
                                show_id: None,
                            });
                        }
                    }
                });
        });

    let Some(cid) = collection_id else {
        ui.weak("Select a collection to see what's in it.");
        return action;
    };
    let Some(collection) = collections.iter().find(|c| c.id == cid) else {
        ui.weak("Select a collection to see what's in it.");
        return action;
    };

    if collection.collection_type != "shows" {
        ui.add_space(8.0);
        ui.weak(format!(
            "This collection type (\"{}\") isn't supported yet — only show collections are \
             browsable here so far.",
            collection.collection_type
        ));
        return action;
    }

    let shows = match &state.shows {
        _ if state.shows_collection_id.as_deref() != Some(cid) => {
            ui.label("Loading…");
            return action;
        }
        Loaded::Idle | Loaded::Loading => {
            ui.label("Loading…");
            return action;
        }
        Loaded::Err(e) => {
            ui.colored_label(egui::Color32::RED, e);
            return action;
        }
        Loaded::Ready(s) => s,
    };

    if shows.is_empty() {
        ui.weak("This collection has no shows.");
        return action;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for s in shows {
            let label = format!("{} ({})", s.name, s.episode_count);
            let row = ui.horizontal(|ui| {
                super::thumb::show(
                    ui,
                    server_url,
                    s.image_url.as_deref(),
                    egui::vec2(48.0, 64.0),
                );
                super::selectable_truncated_label(ui, false, &label);
            });
            let clicked =
                super::row_click(ui, row.response.rect, ("collection_show_row", &s.id)).clicked();
            if clicked {
                action.new_screen = Some(Screen::TvShows {
                    show_id: Some(s.id.clone()),
                    episode_id: None,
                    filter: None,
                });
            }
        }
    });

    action
}
