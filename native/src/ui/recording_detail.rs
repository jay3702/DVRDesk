//! Shared detail pane — used by RecentRecordings and TV Shows (Movies gets
//! its own richer inline view later, matching the old app's own design
//! where `RecordingDetail.tsx` wasn't used by Movies either).
//!
//! Port of the old app's `RecordingDetail.tsx`: the thumbnail itself is the
//! play control (a "hero" — full-width image with a play icon that appears
//! on hover/focus), not a separate button next to the title, with a
//! fallback "▶ Play" button when there's no thumbnail. Episode subtitle,
//! progress bar, and badge row are all computed internally from the
//! `Recording` now rather than being caller-supplied, matching the old
//! component's own design once `duration`/`content_rating`/`genres`/`tags`/
//! `delayed`/`cancelled`/`corrupted` were added to the type.

use chrono::{Local, TimeZone};

use crate::api::types::Recording;

#[derive(Default)]
pub struct RecordingDetailAction {
    pub play: bool,
    /// `Some(new_value)` the frame Mark Watched/Unwatched is clicked.
    pub toggle_watched: Option<bool>,
    pub trash: bool,
    /// Only ever `true` for a completed recording — nothing to download
    /// from one still being written.
    pub download: bool,
}

const HERO_MAX_WIDTH: f32 = 480.0;

fn fmt_datetime(ts_millis: i64) -> String {
    let Some(dt) = Local.timestamp_millis_opt(ts_millis).single() else {
        return String::new();
    };
    format!(
        "{} at {}",
        dt.format("%a, %b %-d, %Y"),
        dt.format("%-I:%M %p")
    )
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
        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .small()
                    .color(egui::Color32::WHITE),
            );
        });
}

pub fn show(ui: &mut egui::Ui, rec: &Recording, server_url: &str) -> RecordingDetailAction {
    // Left margin matters here, not just cosmetics: every caller renders
    // this immediately to the right of a resizable `SidePanel`, in the
    // panel's *sibling* space rather than inside the panel itself. egui
    // registers the panel's own resize-drag hit zone only after the
    // panel's content is laid out (deliberately, so the panel's own
    // `ScrollArea` can't steal it) — but that means content out here,
    // if it starts flush against the panel edge, registers even later
    // and wins the contest for that thin strip, making the divider
    // undraggable. A gap keeps this pane's interactive content (the hero
    // image in particular) clear of it.
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 12.0,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            // Scrolls rather than silently clipping — the hero image +
            // progress bar + badge row + summary + actions can add up to
            // more height than a modest window has available, and this
            // pane isn't nested in a scroll area of its own at any call
            // site.
            egui::ScrollArea::vertical()
                .id_salt("recording_detail_scroll")
                .show(ui, |ui| show_inner(ui, rec, server_url))
                .inner
        })
        .inner
}

fn show_inner(ui: &mut egui::Ui, rec: &Recording, server_url: &str) -> RecordingDetailAction {
    let mut action = RecordingDetailAction::default();

    // `image_url` (real production/promotional artwork from the Gracenote/
    // TMS guide feed) is preferred over `thumbnail_url` (a screenshot the
    // DVR itself captures from the recording, generated from a fixed point
    // near the very start of the file — often still mid-commercial for a
    // show with pre-roll, confirmed via `curl` against `preview.jpg` for a
    // real recording). Channels DVR's `preview.jpg` endpoint has no way to
    // request a different capture point (`?time=`/`?t=`/`?offset=` all
    // confirmed to return the identical, unchanged image), so the DVR's own
    // screenshot only serves as a fallback for the rare recording with no
    // guide artwork at all, not the primary choice.
    let thumb_url = rec.image_url.as_deref().or(rec.thumbnail_url.as_deref());
    match thumb_url {
        Some(raw) => {
            let url = super::thumb::resolve(server_url, raw);
            // Sized to the panel's actual available width (clamped to a
            // sane max) rather than a fixed constant — a fixed size wider
            // than the panel was allocating a rect that overflowed the
            // visible area, which could leave `ui.available_width()`
            // negative for everything laid out after it.
            let width = ui.available_width().max(160.0).min(HERO_MAX_WIDTH);
            // Sized to the image's own aspect ratio rather than a fixed
            // 16:9 — the Gracenote/TMS artwork this now prefers is
            // typically 4:3 (`tmsimg.fancybits.co`'s `w=720&h=540`
            // confirmed via `curl`), and stretching that into a 16:9 box
            // (what a fixed height plus a stretch-to-fill `paint_at` did
            // before) visibly distorted every poster. `load_for_size` gives
            // the real decoded texture size once it's loaded (`None` for
            // the first frame or two, in which case the previous fixed
            // 16:9 guess is used as a harmless fallback until the image
            // arrives, same idiom as the guide grid's channel-logo fix).
            // Clamped to a sane aspect range so a corrupt/degenerate image
            // can't produce a absurdly tall or thin box.
            let image = egui::Image::new(url)
                .rounding(8.0)
                .show_loading_spinner(true);
            let natural_size = image
                .load_for_size(ui.ctx(), egui::vec2(width, width))
                .ok()
                .and_then(|t| t.size());
            let height = match natural_size {
                Some(size) if size.x > 0.0 && size.y > 0.0 => {
                    (width * size.y / size.x).clamp(width * 0.4, width * 1.6)
                }
                _ => width * 9.0 / 16.0,
            };
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
            image.paint_at(ui, rect);
            if response.hovered() || response.has_focus() {
                let painter = ui.painter();
                painter.circle_filled(rect.center(), 28.0, egui::Color32::from_black_alpha(190));
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "▶",
                    egui::FontId::proportional(22.0),
                    egui::Color32::WHITE,
                );
            }
            if response.clicked() {
                action.play = true;
            }
        }
        None => {
            if ui
                .add(egui::Button::new("▶ Play").min_size(egui::vec2(120.0, 36.0)))
                .clicked()
            {
                action.play = true;
            }
        }
    }
    ui.add_space(10.0);

    ui.heading(&rec.title);

    if rec.duration > 0.0 && (rec.watched || rec.playback_time > 0.0) {
        let frac = if rec.watched {
            1.0
        } else {
            (rec.playback_time / rec.duration).clamp(0.0, 1.0) as f32
        };
        let bar_width = ui.available_width().max(0.0).min(320.0);
        let (bar_rect, _) =
            ui.allocate_exact_size(egui::vec2(bar_width, 4.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(bar_rect, 2.0, egui::Color32::from_gray(60));
        let fill = egui::Rect::from_min_size(
            bar_rect.min,
            egui::vec2(bar_rect.width() * frac, bar_rect.height()),
        );
        let fill_color = if rec.watched {
            egui::Color32::GRAY
        } else {
            egui::Color32::from_rgb(0, 120, 212)
        };
        painter.rect_filled(fill, 2.0, fill_color);
    }
    ui.add_space(4.0);

    if let Some(ep_title) = &rec.episode_title {
        let prefix = match (rec.season_number, rec.episode_number) {
            (Some(s), Some(e)) => format!("S{s}E{e} — "),
            _ => String::new(),
        };
        ui.label(
            egui::RichText::new(format!("{prefix}{ep_title}"))
                .italics()
                .weak(),
        );
    }

    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(fmt_datetime(rec.created_at)).weak());
        for tag in &rec.tags {
            badge(ui, tag, egui::Color32::from_gray(70));
        }
        if let Some(rating) = &rec.content_rating {
            badge(ui, rating, egui::Color32::from_gray(70));
        }
        if rec.favorited {
            badge(ui, "Favorited", egui::Color32::from_rgb(150, 110, 0));
        }
        if rec.delayed {
            badge(ui, "Delayed", egui::Color32::from_rgb(150, 40, 40));
        }
        if rec.cancelled {
            badge(ui, "Cancelled", egui::Color32::from_rgb(150, 40, 40));
        }
        if rec.corrupted {
            badge(ui, "Interrupted", egui::Color32::from_rgb(150, 40, 40));
        }
        if !rec.completed {
            badge(ui, "Recording", egui::Color32::from_rgb(0, 120, 60));
        } else if rec.duration > 0.0 {
            ui.label(egui::RichText::new(fmt_duration(rec.duration)).weak());
        }
    });
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);

    if let Some(summary) = rec.full_summary.as_ref().or(rec.summary.as_ref()) {
        ui.label(summary);
        ui.add_space(8.0);
    }

    if !rec.path.is_empty() {
        ui.label(
            egui::RichText::new(format!("Path: {}", rec.path))
                .small()
                .weak(),
        );
    }
    if !rec.genres.is_empty() {
        ui.label(egui::RichText::new(rec.genres.join(" · ")).small().weak());
    }
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        let watched_label = if rec.watched {
            "↺ Mark Unwatched"
        } else {
            "✔ Mark Watched"
        };
        if ui.button(watched_label).clicked() {
            action.toggle_watched = Some(!rec.watched);
        }
        if ui.button("🗑 Trash").clicked() {
            action.trash = true;
        }
        if rec.completed && ui.button("⬇ Download").clicked() {
            action.download = true;
        }
    });

    action
}
