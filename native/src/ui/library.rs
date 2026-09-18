//! Port of `Library.tsx` ("Videos" in the sidebar) — DVR's custom video
//! groups feature. Simpler than TV Shows/Movies: no watched-toggle, no
//! trash, no summary field — none of those exist for library videos at
//! all (confirmed against the real API), so this doesn't reuse the shared
//! `recording_detail` component, which assumes all of that.
//!
//! Sort controls at both levels (groups, then videos within a group),
//! mirroring TV Shows' show-list/episode-list dual pattern — same shared
//! `SortOrder`/`order_buttons` from `ui/mod.rs`, field sets adapted to
//! what `VideoGroup`/`Video` actually have (no `updated_at` on either, no
//! season/episode shape at all).

use chrono::{Local, TimeZone};

use super::{Loaded, SortOrder};
use crate::api::types::{Video, VideoGroup};
use crate::screen::Screen;

const INITIAL_VISIBLE_GROUPS: usize = 90;
const VISIBLE_GROUPS_STEP: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupSortField {
    Title,
    Id,
    DateAdded,
    VideoCount,
}

impl GroupSortField {
    const ALL: [GroupSortField; 4] = [
        GroupSortField::Title,
        GroupSortField::Id,
        GroupSortField::DateAdded,
        GroupSortField::VideoCount,
    ];

    fn label(self) -> &'static str {
        match self {
            GroupSortField::Title => "Title",
            GroupSortField::Id => "ID",
            GroupSortField::DateAdded => "Date Added",
            GroupSortField::VideoCount => "Video Count",
        }
    }

    fn default_order(self) -> SortOrder {
        match self {
            GroupSortField::Title => SortOrder::Asc,
            _ => SortOrder::Desc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoSortField {
    Title,
    Id,
    DateAdded,
}

impl VideoSortField {
    const ALL: [VideoSortField; 3] = [
        VideoSortField::Title,
        VideoSortField::Id,
        VideoSortField::DateAdded,
    ];

    fn label(self) -> &'static str {
        match self {
            VideoSortField::Title => "Title",
            VideoSortField::Id => "ID",
            VideoSortField::DateAdded => "Date Added",
        }
    }

    fn default_order(self) -> SortOrder {
        match self {
            VideoSortField::Title => SortOrder::Asc,
            _ => SortOrder::Desc,
        }
    }
}

pub struct LibraryState {
    pub groups: Loaded<Vec<VideoGroup>>,
    pub videos: Loaded<Vec<Video>>,
    pub videos_group_id: Option<String>,
    pub visible_groups: usize,
    pub group_sort: GroupSortField,
    pub group_sort_order: SortOrder,
    pub video_sort: VideoSortField,
    pub video_sort_order: SortOrder,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            groups: Loaded::Idle,
            videos: Loaded::Idle,
            videos_group_id: None,
            visible_groups: INITIAL_VISIBLE_GROUPS,
            group_sort: GroupSortField::Title,
            group_sort_order: GroupSortField::Title.default_order(),
            video_sort: VideoSortField::Title,
            video_sort_order: VideoSortField::Title.default_order(),
        }
    }
}

#[derive(Default)]
pub struct LibraryAction {
    pub new_screen: Option<Screen>,
    pub play: Option<Video>,
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut LibraryState,
    group_id: Option<&str>,
    video_id: Option<&str>,
    server_url: &str,
) -> LibraryAction {
    let mut action = LibraryAction::default();

    let groups = match &state.groups {
        Loaded::Idle | Loaded::Loading => {
            ui.label("Loading video groups…");
            return action;
        }
        Loaded::Err(e) => {
            ui.colored_label(egui::Color32::RED, e);
            return action;
        }
        Loaded::Ready(g) => g,
    };

    egui::SidePanel::left("library_groups")
        .resizable(true)
        .default_width(300.0)
        .show_inside(ui, |ui| {
            // `horizontal_wrapped`, not `horizontal` — see the matching
            // comment in `movies.rs` for why.
            ui.horizontal_wrapped(|ui| {
                ui.heading("Videos");
                ui.separator();
                egui::ComboBox::from_id_salt("group_sort_field")
                    .selected_text(state.group_sort.label())
                    .show_ui(ui, |ui| {
                        for field in GroupSortField::ALL {
                            if ui
                                .selectable_label(state.group_sort == field, field.label())
                                .clicked()
                                && state.group_sort != field
                            {
                                state.group_sort = field;
                                state.group_sort_order = field.default_order();
                                state.visible_groups = INITIAL_VISIBLE_GROUPS;
                            }
                        }
                    });
                if super::order_buttons(ui, &mut state.group_sort_order) {
                    state.visible_groups = INITIAL_VISIBLE_GROUPS;
                }
            });
            ui.separator();

            let mut sorted_groups: Vec<&VideoGroup> = groups.iter().collect();
            match state.group_sort {
                GroupSortField::Title => {
                    sorted_groups.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
                GroupSortField::Id => sorted_groups.sort_by(|a, b| a.id.cmp(&b.id)),
                GroupSortField::DateAdded => sorted_groups.sort_by_key(|g| g.created_at),
                GroupSortField::VideoCount => sorted_groups.sort_by_key(|g| g.video_count),
            }
            if state.group_sort_order == SortOrder::Desc {
                sorted_groups.reverse();
            }

            // See the matching comment in `recent.rs` — without this, the
            // SidePanel's stored width silently gets overwritten by the
            // group list's natural content width every frame instead of
            // wherever the user actually drags the divider to.
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for g in sorted_groups.iter().take(state.visible_groups) {
                        let is_selected = group_id == Some(g.id.as_str());
                        let label = format!("{} ({})", g.name, g.video_count);
                        if super::selectable_truncated_label(ui, is_selected, &label).clicked() {
                            action.new_screen = Some(Screen::Library {
                                group_id: Some(g.id.clone()),
                                video_id: None,
                            });
                        }
                    }
                    if state.visible_groups < sorted_groups.len()
                        && ui.button("Load more").clicked()
                    {
                        state.visible_groups += VISIBLE_GROUPS_STEP;
                    }
                });
        });

    let Some(gid) = group_id else {
        ui.weak("Select a group to see its videos.");
        return action;
    };

    let videos = match &state.videos {
        _ if state.videos_group_id.as_deref() != Some(gid) => {
            ui.label("Loading videos…");
            return action;
        }
        Loaded::Idle | Loaded::Loading => {
            ui.label("Loading videos…");
            return action;
        }
        Loaded::Err(e) => {
            ui.colored_label(egui::Color32::RED, e);
            return action;
        }
        Loaded::Ready(v) => v,
    };

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("video_sort_field")
            .selected_text(state.video_sort.label())
            .show_ui(ui, |ui| {
                for field in VideoSortField::ALL {
                    if ui
                        .selectable_label(state.video_sort == field, field.label())
                        .clicked()
                    {
                        state.video_sort = field;
                        state.video_sort_order = field.default_order();
                    }
                }
            });
        super::order_buttons(ui, &mut state.video_sort_order);
    });
    ui.separator();

    let mut sorted_videos: Vec<&Video> = videos.iter().collect();
    match state.video_sort {
        VideoSortField::Title => sorted_videos.sort_by(|a, b| {
            let at = a.video_title.as_deref().unwrap_or(&a.title).to_lowercase();
            let bt = b.video_title.as_deref().unwrap_or(&b.title).to_lowercase();
            at.cmp(&bt)
        }),
        VideoSortField::Id => sorted_videos.sort_by(|a, b| a.id.cmp(&b.id)),
        VideoSortField::DateAdded => sorted_videos.sort_by_key(|v| v.created_at),
    }
    if state.video_sort_order == SortOrder::Desc {
        sorted_videos.reverse();
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for v in sorted_videos {
            let is_selected = video_id == Some(v.id.as_str());
            let label = v.video_title.clone().unwrap_or_else(|| v.title.clone());
            let label = if v.watched {
                format!("✔ {label}")
            } else {
                label
            };
            ui.horizontal(|ui| {
                let row = ui.horizontal(|ui| {
                    super::thumb::show(
                        ui,
                        server_url,
                        v.thumbnail_url.as_deref().or(v.image_url.as_deref()),
                        egui::vec2(64.0, 36.0),
                    );
                    super::selectable_truncated_label(ui, is_selected, &label);
                });
                let resp = super::row_click(ui, row.response.rect, ("library_video_row", &v.id));
                if resp.clicked() {
                    action.new_screen = Some(Screen::Library {
                        group_id: Some(gid.to_string()),
                        video_id: Some(v.id.clone()),
                    });
                }
                if ui.small_button("▶").clicked() {
                    action.play = Some(v.clone());
                }
            });
        }
    });

    if let Some(v) = video_id.and_then(|id| videos.iter().find(|v| v.id == id)) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        super::thumb::show(
            ui,
            server_url,
            v.thumbnail_url.as_deref().or(v.image_url.as_deref()),
            egui::vec2(320.0, 180.0),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading(v.video_title.as_deref().unwrap_or(&v.title));
            if ui.button("▶ Play").clicked() {
                action.play = Some(v.clone());
            }
        });
        let recorded = Local
            .timestamp_millis_opt(v.created_at)
            .single()
            .map(|dt| dt.format("%A, %B %-d, %Y").to_string())
            .unwrap_or_default();
        ui.label(egui::RichText::new(format!("Added: {recorded}")).weak());
    }

    action
}
