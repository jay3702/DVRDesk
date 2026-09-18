//! The Downloads screen — everything running, queued, paused, or finished,
//! with a way to pause/resume/remove any of it. Downloads themselves are
//! started from a card/detail view elsewhere (`media_card.rs`,
//! `recording_detail.rs`); this is purely a management view, matching
//! Clicker's own `ui_downloads.rs` in spirit — nothing here can change what
//! the DVR holds, only what's kept on this machine.

use crate::downloads::{Downloads, Status};

#[derive(Default)]
pub struct DownloadsScreenAction {
    pub pause: Option<String>,
    /// Needs the stored record (in particular its `url`) to re-fetch.
    pub resume: Option<(String, crate::downloads::DownloadRecord)>,
    pub remove: Option<String>,
    pub clear_finished: bool,
    /// A finished download's id — "▶ Play" or a double-click on its row.
    /// Carries only the id, not a `Recording`: this module only ever has
    /// the lightweight `DownloadRecord` snapshot, not the full server
    /// object playback needs for resume/watched tracking, so the caller
    /// fetches that itself (same as the guide's "Play Recording" action).
    pub play: Option<String>,
}

pub fn show(ui: &mut egui::Ui, server_url: &str, downloads: &Downloads) -> DownloadsScreenAction {
    let mut action = DownloadsScreenAction::default();
    let entries = downloads.entries();

    ui.horizontal(|ui| {
        ui.heading("Downloads");
        if entries.iter().any(|(_, s, _)| s.is_finished()) && ui.button("Clear finished").clicked()
        {
            action.clear_finished = true;
        }
    });
    ui.separator();

    if entries.is_empty() {
        ui.add_space(16.0);
        ui.weak("No downloads yet — download a recording from its card or detail view and it'll appear here.");
        return action;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (id, status, record) in &entries {
                let is_done = matches!(status, Status::Done(_));
                ui.horizontal(|ui| {
                    // Thumbnail + title/status/progress only — kept in its own
                    // inner `Ui` so `row_click` below can be scoped to exactly
                    // this rect, not the button row that follows it in the same
                    // outer horizontal. Registering a click-catcher over the
                    // buttons' own rect would steal their clicks (a later
                    // `ui.interact()` wins any overlap) — this sidesteps that
                    // by never overlapping them in the first place, the same
                    // structure `library.rs`'s row+play-button already uses.
                    let row = ui.horizontal(|ui| {
                        super::thumb::show(
                            ui,
                            server_url,
                            record.thumbnail_url.as_deref(),
                            egui::vec2(96.0, 54.0),
                        );

                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&record.title).strong());
                            if let Some(sub) = &record.subtitle {
                                ui.label(egui::RichText::new(sub).small().weak());
                            }

                            let (detail, color) = match status {
                                Status::Queued => {
                                    ("Waiting".to_string(), ui.visuals().weak_text_color())
                                }
                                Status::Active(frac) => {
                                    let text = if *frac >= 0.0 {
                                        format!("{:.0}%", frac * 100.0)
                                    } else {
                                        "Starting…".to_string()
                                    };
                                    (text, egui::Color32::from_rgb(0, 120, 212))
                                }
                                Status::Paused(frac) => {
                                    let text = if *frac >= 0.0 {
                                        format!("Paused at {:.0}%", frac * 100.0)
                                    } else {
                                        "Paused — resumes where it stopped".to_string()
                                    };
                                    (text, egui::Color32::from_rgb(200, 140, 0))
                                }
                                Status::Done(_) => (
                                    "Downloaded".to_string(),
                                    egui::Color32::from_rgb(45, 145, 68),
                                ),
                                Status::Failed(e) => (format!("Failed: {e}"), egui::Color32::RED),
                            };
                            ui.colored_label(color, detail);

                            if let Status::Active(frac) = status {
                                if *frac >= 0.0 {
                                    let (bar_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(240.0, 4.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    painter.rect_filled(
                                        bar_rect,
                                        2.0,
                                        ui.visuals().extreme_bg_color,
                                    );
                                    let fill = egui::Rect::from_min_size(
                                        bar_rect.min,
                                        egui::vec2(
                                            bar_rect.width() * frac.clamp(0.0, 1.0),
                                            bar_rect.height(),
                                        ),
                                    );
                                    painter.rect_filled(
                                        fill,
                                        2.0,
                                        egui::Color32::from_rgb(0, 120, 212),
                                    );
                                }
                            }
                        });
                    });

                    if is_done {
                        let resp = super::row_click(ui, row.response.rect, ("download_row", id));
                        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.double_clicked() {
                            action.play = Some(id.clone());
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🗑 Remove").clicked() {
                            action.remove = Some(id.clone());
                        }
                        if matches!(status, Status::Active(_) | Status::Queued) {
                            if ui.button("⏸ Pause").clicked() {
                                action.pause = Some(id.clone());
                            }
                        } else if status.is_resumable() && ui.button("▶ Resume").clicked() {
                            action.resume = Some((id.clone(), record.clone()));
                        } else if is_done && ui.button("▶ Play").clicked() {
                            action.play = Some(id.clone());
                        }
                    });
                });
                ui.separator();
            }
        });

    action
}
