//! Port of `MediaCard.tsx` — the card used for TV Shows' episode grid and
//! Movies' list. Renders 16:9 art, title/subtitle, a badge row (tags,
//! content rating, favorited/delayed/cancelled/corrupted, recording
//! status), a recorded-time + duration line, and a progress bar — laid out
//! by the caller inside an `egui::Grid` (via `columns_for_width()` to pick
//! a column count) for a wrapping-grid effect matching the old app's
//! `.media-grid` (`grid-template-columns: repeat(auto-fill, minmax(180px,
//! 1fr))`).
//!
//! `ui.horizontal_wrapped()` was tried first and produced a "staircase"
//! cascade instead of aligned rows — each card's cell apparently reported
//! its size back to the wrapping layout before its actual dynamic-height
//! content (badges row, truncatable labels) had settled, throwing off
//! where the wrap layout placed the next item. `egui::Grid` doesn't have
//! this problem: column/row sizes are derived from all cells up front, not
//! accumulated incrementally.
//!
//! Not ported: the play/watch-toggle hover buttons overlaid on the art —
//! single click selects and double click plays, same interaction the
//! plain-row versions of these lists already used, so this is additive
//! (richer content) rather than a new interaction model.

use chrono::{Local, TimeZone};

use crate::api::types::Recording;

pub const CARD_WIDTH: f32 = 200.0;
const GRID_SPACING: f32 = 14.0;

/// Picks a column count so `CARD_WIDTH`-wide cards (plus spacing) fill the
/// available width without overflowing — the `egui::Grid` analogue of the
/// old app's CSS `repeat(auto-fill, minmax(180px, 1fr))`.
pub fn columns_for_width(available_width: f32) -> usize {
    (((available_width + GRID_SPACING) / (CARD_WIDTH + GRID_SPACING)).floor() as usize).max(1)
}

pub struct MediaCardAction {
    pub clicked: bool,
    pub double_clicked: bool,
    /// The download icon was clicked — only ever `true` for a completed
    /// recording (there's nothing to download from one still being
    /// written). Whether this actually starts, resumes, or no-ops is the
    /// caller's `Downloads::start()`'s call to make, not this module's —
    /// it doesn't know about download state at all, so a click always
    /// fires regardless of whether one's already running or finished.
    pub download_clicked: bool,
}

fn fmt_time(ts_millis: i64) -> String {
    let Some(dt) = Local.timestamp_millis_opt(ts_millis).single() else {
        return String::new();
    };
    dt.format("%-I:%M %p").to_string()
}

fn fmt_duration(total_secs: f64) -> String {
    let total = total_secs.max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

fn badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::default()
        .fill(color)
        .inner_margin(egui::Margin::symmetric(4.0, 1.0))
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(9.0)
                    .color(egui::Color32::WHITE),
            );
        });
}

/// `title`/`subtitle` are caller-supplied since TV Shows (`S1E4`, episode
/// title) and Movies (plain title, no subtitle) label cards differently —
/// everything else (badges, meta, progress) is computed from `rec` itself.
pub fn show(
    ui: &mut egui::Ui,
    server_url: &str,
    rec: &Recording,
    title: &str,
    subtitle: Option<&str>,
    selected: bool,
) -> MediaCardAction {
    let id = ui.make_persistent_id(("media_card", &rec.id));
    let mut art_rect = egui::Rect::NOTHING;

    let outer = ui.allocate_ui(egui::vec2(CARD_WIDTH, 0.0), |ui| {
        let fill = if selected {
            ui.visuals().selection.bg_fill.linear_multiply(0.3)
        } else {
            ui.visuals().extreme_bg_color
        };
        egui::Frame::default()
            .fill(fill)
            .stroke(egui::Stroke::new(
                1.0_f32,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ))
            .rounding(8.0)
            .show(ui, |ui| {
                ui.set_width(CARD_WIDTH);
                ui.vertical(|ui| {
                    let art_size = egui::vec2(CARD_WIDTH, CARD_WIDTH * 9.0 / 16.0);
                    match rec.thumbnail_url.as_deref().or(rec.image_url.as_deref()) {
                        Some(raw) => {
                            let url = super::thumb::resolve(server_url, raw);
                            let response = ui.add(
                                egui::Image::new(url)
                                    .fit_to_exact_size(art_size)
                                    // `fit_to_exact_size` alone still
                                    // preserves the source image's own
                                    // aspect ratio (letterboxed within the
                                    // box) — since thumbnails come back at
                                    // whatever aspect Channels DVR happens
                                    // to generate per-recording, that was
                                    // rendering every card's art at a
                                    // different actual height, breaking
                                    // grid alignment across a row. Forcing
                                    // it to fill the exact box (mild
                                    // stretch, not a crop) keeps every
                                    // card the same size regardless.
                                    .maintain_aspect_ratio(false)
                                    .show_loading_spinner(true),
                            );
                            art_rect = response.rect;
                        }
                        None => {
                            let (rect, _) = ui.allocate_exact_size(art_size, egui::Sense::hover());
                            ui.painter()
                                .rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "▶",
                                egui::FontId::proportional(24.0),
                                ui.visuals().weak_text_color(),
                            );
                            art_rect = rect;
                        }
                    }

                    egui::Frame::default()
                        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                        .show(ui, |ui| {
                            ui.set_width(CARD_WIDTH - 16.0);
                            // Truncated to one line with an ellipsis, matching
                            // the old app's CSS (`white-space: nowrap;
                            // text-overflow: ellipsis`) — without this, cards
                            // with longer titles/subtitles wrap to two lines
                            // and grow taller than their neighbors, breaking
                            // grid alignment across a row of otherwise
                            // same-width cards.
                            ui.add(
                                egui::Label::new(egui::RichText::new(title).strong().size(13.0))
                                    .truncate(),
                            );
                            if let Some(sub) = subtitle {
                                if !sub.is_empty() {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(sub).small().weak())
                                            .truncate(),
                                    );
                                }
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                                for tag in &rec.tags {
                                    badge(ui, tag, egui::Color32::from_gray(60));
                                }
                                if let Some(rating) = &rec.content_rating {
                                    badge(ui, rating, egui::Color32::from_gray(60));
                                }
                                if rec.favorited {
                                    badge(ui, "Favorited", egui::Color32::from_rgb(23, 106, 42));
                                }
                                if rec.delayed {
                                    badge(ui, "Delayed", egui::Color32::from_rgb(122, 26, 26));
                                }
                                if rec.cancelled {
                                    badge(ui, "Cancelled", egui::Color32::from_rgb(122, 26, 26));
                                }
                                if rec.corrupted {
                                    badge(ui, "Interrupted", egui::Color32::from_rgb(122, 26, 26));
                                }
                                if !rec.completed {
                                    badge(ui, "Recording", egui::Color32::from_rgb(122, 26, 26));
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(fmt_time(rec.created_at))
                                        .size(10.0)
                                        .weak(),
                                );
                                let duration_label = if !rec.completed {
                                    "Recording".to_string()
                                } else {
                                    fmt_duration(rec.duration)
                                };
                                ui.label(egui::RichText::new(duration_label).size(11.0).weak());
                            });
                        });

                    let progress = if rec.duration > 0.0 && rec.playback_time > 0.0 {
                        (rec.playback_time / rec.duration).clamp(0.0, 1.0) as f32
                    } else {
                        0.0
                    };
                    if rec.watched || progress > 0.0 {
                        let (bar_rect, _) = ui
                            .allocate_exact_size(egui::vec2(CARD_WIDTH, 3.0), egui::Sense::hover());
                        let painter = ui.painter();
                        painter.rect_filled(bar_rect, 0.0, ui.visuals().extreme_bg_color);
                        let frac = if rec.watched { 1.0 } else { progress };
                        let fill = egui::Rect::from_min_size(
                            bar_rect.min,
                            egui::vec2(bar_rect.width() * frac, bar_rect.height()),
                        );
                        let fill_color = if rec.watched {
                            egui::Color32::from_rgb(45, 145, 68)
                        } else {
                            egui::Color32::from_rgb(0, 120, 212)
                        };
                        painter.rect_filled(fill, 0.0, fill_color);
                    }
                });
            });
    });

    let response = ui.interact(outer.response.rect, id, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    // Registered *after* the whole-card interact above, so this smaller
    // hotspot wins the pointer for the pixels it overlaps — egui resolves
    // hover/click by last-registered-wins at a given screen position, the
    // same "declare the more specific region after the broader one" idiom
    // this codebase's hover-play overlay in `recording_detail.rs` already
    // relies on for its own art-region overlay. Only offered for a
    // completed recording — there's nothing to download from one still
    // being written (the same `rec.completed` gate the "Recording" badge
    // above already uses).
    let mut download_clicked = false;
    if rec.completed {
        let icon_rect = egui::Rect::from_min_size(
            art_rect.right_bottom() - egui::vec2(28.0, 28.0),
            egui::vec2(24.0, 24.0),
        );
        let dl_id = ui.make_persistent_id(("media_card_download", &rec.id));
        let dl_response = ui.interact(icon_rect, dl_id, egui::Sense::click());
        let dl_response = dl_response.on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.painter().circle_filled(
            icon_rect.center(),
            13.0,
            egui::Color32::from_black_alpha(190),
        );
        ui.painter().text(
            icon_rect.center(),
            egui::Align2::CENTER_CENTER,
            "⬇",
            egui::FontId::proportional(14.0),
            egui::Color32::WHITE,
        );
        download_clicked = dl_response.clicked();
    }

    MediaCardAction {
        clicked: response.clicked(),
        double_clicked: response.double_clicked(),
        download_clicked,
    }
}
