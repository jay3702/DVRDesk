//! Full-screen playback overlay. Matches the old app's design exactly:
//! `VideoPlayer` rendered at the App root next to `<Routes>`, on top of
//! whatever page is active underneath, not as a route/Screen of its own —
//! see the plan's §3 state/navigation model.
//!
//! Remaining work not yet in here: the real mpv event thread (still
//! per-frame polling), the old app's manual-seek override tracking for
//! commercial auto-skip (meaningful now that real seeking exists, but not
//! ported yet).

use std::time::{Duration, Instant};

use crate::player::captions::{CaptionKind, CaptionMode, CaptionTrack};
use crate::player::Player;
use crate::player::{commercial_skip, keybindings};
use crate::state::now_playing::NowPlaying;
use crate::state::settings::{KeybindingsConfig, SkipIntervalsConfig};

/// How long the pointer has to sit still before a live channel's controls
/// auto-hide.
const CONTROLS_HIDE_DELAY: Duration = Duration::from_secs(3);

pub struct PlayerOverlayAction {
    /// Caller tears down playback (stop the player, clear `now_playing`)
    /// since this module doesn't own that state.
    pub close: bool,
    /// `Some` the frame the user picks a different caption option — caller
    /// applies it (this module only renders the picker; App owns
    /// caption_mode and the tick that actually calls `Player::set_sid`).
    pub new_caption_mode: Option<CaptionMode>,
    /// `Some` the frame the user toggles commercial auto-skip.
    pub new_skip_ads: Option<bool>,
    /// `Some` the frame the user toggles the stats-for-nerds panel.
    pub new_show_stats: Option<bool>,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    player: &Player,
    now_playing: &NowPlaying,
    caption_tracks: &[CaptionTrack],
    caption_mode: CaptionMode,
    skip_ads: bool,
    showing_skip_toast: bool,
    skip_intervals: SkipIntervalsConfig,
    player_keybindings: &KeybindingsConfig,
    diagnostics_enabled: bool,
    show_stats: bool,
    controls_active_since: &mut Instant,
) -> PlayerOverlayAction {
    let mut close_clicked = false;
    let mut new_caption_mode = None;
    let mut new_skip_ads = None;
    let mut new_show_stats = None;
    let screen_rect = ctx.screen_rect();
    let ad_blocks = commercial_skip::ad_blocks(&now_playing.commercials);

    // Any pointer activity resets the idle clock — checked globally (not
    // scoped to a particular widget) since the whole point is "the user is
    // still around," not "the user is touching this specific control."
    let pointer_active =
        ctx.input(|i| i.pointer.is_moving() || i.pointer.any_click() || i.pointer.any_pressed());
    if pointer_active {
        *controls_active_since = Instant::now();
    }
    // Applies to every kind of playback now (live and recorded alike) —
    // originally scoped to live only, extended on request.
    let controls_visible = controls_active_since.elapsed() < CONTROLS_HIDE_DELAY;

    // Keyboard shortcuts, checked once per frame regardless of which widget
    // has focus — matches the old app's onPlayerKeyDown, which listened
    // globally while the player was open rather than requiring the video
    // element itself to be focused.
    ctx.input(|input| {
        let pos = player.position_secs().unwrap_or(0.0);
        let dur = player.duration_secs().unwrap_or(f64::MAX);
        if keybindings::any_pressed(input, &player_keybindings.skip_forward) {
            player.seek((pos + skip_intervals.skip_forward as f64).min(dur));
        }
        if keybindings::any_pressed(input, &player_keybindings.skip_back) {
            player.seek((pos - skip_intervals.skip_back as f64).max(0.0));
        }
        if keybindings::any_pressed(input, &player_keybindings.fast_forward) {
            player.seek((pos + skip_intervals.fast_forward as f64).min(dur));
        }
        if keybindings::any_pressed(input, &player_keybindings.fast_reverse) {
            player.seek((pos - skip_intervals.fast_reverse as f64).max(0.0));
        }
        if keybindings::any_pressed(input, &player_keybindings.play_pause) {
            player.set_pause(!player.paused());
        }
        if keybindings::any_pressed(input, &player_keybindings.close) {
            close_clicked = true;
        }
    });

    let has_py_captions = caption_tracks
        .iter()
        .any(|t| t.kind == CaptionKind::PyCaptions);
    let has_broadcast = caption_tracks
        .iter()
        .any(|t| t.kind == CaptionKind::Broadcast);

    egui::Area::new(egui::Id::new("player_overlay"))
        .fixed_pos(screen_rect.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // No margin once controls are hidden — a live channel's video
            // should fill the frame edge to edge, not just have empty space
            // where the header/scrubber used to be.
            egui::Frame::default()
                .fill(egui::Color32::BLACK)
                .inner_margin(if controls_visible { 8.0 } else { 0.0 })
                .show(ui, |ui| {
                    ui.set_min_size(screen_rect.size());
                    ui.set_max_size(screen_rect.size());

                    if controls_visible {
                        ui.horizontal(|ui| {
                            ui.heading(
                                egui::RichText::new(&now_playing.title).color(egui::Color32::WHITE),
                            );

                            if ui
                                .button(format!("◀ {}s", skip_intervals.skip_back))
                                .clicked()
                            {
                                let pos = player.position_secs().unwrap_or(0.0);
                                player.seek((pos - skip_intervals.skip_back as f64).max(0.0));
                            }
                            if ui
                                .button(if player.paused() {
                                    "▶ Resume"
                                } else {
                                    "⏸ Pause"
                                })
                                .clicked()
                            {
                                player.set_pause(!player.paused());
                            }
                            if ui
                                .button(format!("{}s ▶", skip_intervals.skip_forward))
                                .clicked()
                            {
                                let pos = player.position_secs().unwrap_or(0.0);
                                let dur = player.duration_secs().unwrap_or(f64::MAX);
                                player.seek((pos + skip_intervals.skip_forward as f64).min(dur));
                            }

                            // Matches the old app's dropdown exactly: only shown
                            // once at least one caption track is actually
                            // available, same "Off / Broadcast / Py-Captions"
                            // options.
                            if has_py_captions || has_broadcast {
                                let current_label = match caption_mode {
                                    CaptionMode::Off => "CC: Off",
                                    CaptionMode::PyCaptions => "CC: Py-Captions",
                                    CaptionMode::Broadcast => "CC: Broadcast",
                                };
                                egui::ComboBox::from_id_salt("caption_mode")
                                    .selected_text(current_label)
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(
                                                caption_mode == CaptionMode::Off,
                                                "Off",
                                            )
                                            .clicked()
                                        {
                                            new_caption_mode = Some(CaptionMode::Off);
                                        }
                                        if has_broadcast
                                            && ui
                                                .selectable_label(
                                                    caption_mode == CaptionMode::Broadcast,
                                                    "Broadcast",
                                                )
                                                .clicked()
                                        {
                                            new_caption_mode = Some(CaptionMode::Broadcast);
                                        }
                                        if has_py_captions
                                            && ui
                                                .selectable_label(
                                                    caption_mode == CaptionMode::PyCaptions,
                                                    "Py-Captions",
                                                )
                                                .clicked()
                                        {
                                            new_caption_mode = Some(CaptionMode::PyCaptions);
                                        }
                                    });
                            }

                            if !ad_blocks.is_empty() {
                                let label = if skip_ads {
                                    "⏭ Skip Ads: On"
                                } else {
                                    "⏭ Skip Ads: Off"
                                };
                                if ui.button(label).clicked() {
                                    new_skip_ads = Some(!skip_ads);
                                }
                            }

                            if diagnostics_enabled {
                                let label = if show_stats { "Hide Stats" } else { "Stats" };
                                if ui.button(label).clicked() {
                                    new_show_stats = Some(!show_stats);
                                }
                                if ui.button("Copy Report").clicked() {
                                    ctx.copy_text(build_report(player, now_playing));
                                }
                            }

                            // Plain text, not an icon glyph — this
                            // codebase has repeatedly found egui's bundled
                            // fonts don't cover every Unicode symbol that
                            // looks plausible (✕/▲/▼ all needed swapping
                            // out earlier), and there's no already-proven-
                            // safe glyph for "fullscreen" to reach for.
                            let is_fullscreen =
                                ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
                            let fullscreen_label = if is_fullscreen {
                                "Exit Fullscreen"
                            } else {
                                "Fullscreen"
                            };
                            if ui.button(fullscreen_label).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                                    !is_fullscreen,
                                ));
                            }

                            if ui.button("✖ Close").clicked() {
                                close_clicked = true;
                            }
                        });

                        let pos = player.position_secs().unwrap_or(0.0);
                        let dur = player.duration_secs().unwrap_or(0.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  /  {}",
                                    fmt_secs(pos),
                                    fmt_secs(dur)
                                ))
                                .color(egui::Color32::LIGHT_GRAY),
                            );
                            if showing_skip_toast {
                                ui.label(
                                    egui::RichText::new("Skipping commercial…")
                                        .color(egui::Color32::YELLOW),
                                );
                            }
                        });

                        // Real interactive scrubber: click or drag anywhere on
                        // the bar to seek. Ad blocks (if any) are drawn as red
                        // segments directly on it rather than as a separate
                        // non-interactive bar, so there's one control instead
                        // of two competing for the same space.
                        if dur > 0.0 {
                            let (bar_rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 14.0),
                                egui::Sense::click_and_drag(),
                            );
                            let painter = ui.painter();
                            painter.rect_filled(bar_rect, 2.0, egui::Color32::from_gray(40));
                            for &(start, end) in &ad_blocks {
                                let left = bar_rect.left()
                                    + (start.max(0.0) / dur).min(1.0) as f32 * bar_rect.width();
                                let right = bar_rect.left()
                                    + (end.max(0.0) / dur).min(1.0) as f32 * bar_rect.width();
                                let seg = egui::Rect::from_min_max(
                                    egui::pos2(left, bar_rect.top()),
                                    egui::pos2(right.max(left + 1.0), bar_rect.bottom()),
                                );
                                painter.rect_filled(seg, 1.0, egui::Color32::from_rgb(180, 60, 40));
                            }
                            let playhead_x = bar_rect.left()
                                + (pos / dur).clamp(0.0, 1.0) as f32 * bar_rect.width();
                            painter.line_segment(
                                [
                                    egui::pos2(playhead_x, bar_rect.top() - 2.0),
                                    egui::pos2(playhead_x, bar_rect.bottom() + 2.0),
                                ],
                                egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
                            );

                            if response.clicked() || response.dragged() {
                                if let Some(interact_pos) = response.interact_pointer_pos() {
                                    let frac = ((interact_pos.x - bar_rect.left())
                                        / bar_rect.width())
                                    .clamp(0.0, 1.0)
                                        as f64;
                                    player.seek(frac * dur);
                                }
                            }
                            ui.add_space(4.0);
                        }
                    } // controls_visible

                    let available = ui.available_size();
                    let (rect, _response) = ui.allocate_exact_size(available, egui::Sense::hover());
                    ui.painter()
                        .add(crate::player::paint_callback(player, rect, ctx));
                });
        });

    // Rendered as its own floating Area, on top of everything (including the
    // main overlay's own Order::Foreground content, i.e. the video) rather
    // than as a sibling in the main overlay's vertical layout — it used to
    // live inline there, which meant `ui.available_size()` for the video
    // rect was computed *after* the stats panel had already consumed
    // vertical space, visibly pushing the video down instead of floating
    // over it.
    if diagnostics_enabled && show_stats {
        egui::Area::new(egui::Id::new("player_stats_overlay"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 56.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(egui::Color32::from_black_alpha(220))
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_max_width(340.0);
                        ui.label(
                            egui::RichText::new("Stats for nerds")
                                .strong()
                                .color(egui::Color32::WHITE),
                        );
                        stat_row(
                            ui,
                            "State",
                            if player.paused() { "paused" } else { "playing" },
                        );
                        stat_row(
                            ui,
                            "Dropped frames",
                            &player
                                .dropped_frames()
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "n/a".to_string()),
                        );
                        stat_row(
                            ui,
                            "A/V sync",
                            &player
                                .avsync_secs()
                                .map(|v| format!("{v:.3}s"))
                                .unwrap_or_else(|| "n/a".to_string()),
                        );
                        stat_row(
                            ui,
                            "Buffer ahead",
                            &player
                                .buffer_ahead_secs()
                                .map(|v| format!("{v:.2}s"))
                                .unwrap_or_else(|| "n/a".to_string()),
                        );
                        stat_row(
                            ui,
                            "Bitrate (v/a)",
                            &format!(
                                "{} / {}",
                                fmt_bitrate(player.video_bitrate_bps()),
                                fmt_bitrate(player.audio_bitrate_bps())
                            ),
                        );
                        stat_row(
                            ui,
                            "Caption tracks",
                            &if caption_tracks.is_empty() {
                                "none detected".to_string()
                            } else {
                                caption_tracks
                                    .iter()
                                    .map(|t| format!("sid={} {:?}", t.sid, t.kind))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            },
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "Manifest: {}",
                                now_playing.manifest_url.as_deref().unwrap_or("n/a")
                            ))
                            .small()
                            .color(egui::Color32::DARK_GRAY),
                        );
                    });
            });
    }

    ctx.request_repaint();
    PlayerOverlayAction {
        close: close_clicked,
        new_caption_mode,
        new_skip_ads,
        new_show_stats,
    }
}

fn fmt_secs(total: f64) -> String {
    let total = total.max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn fmt_bitrate(bps: Option<f64>) -> String {
    match bps {
        Some(b) if b > 0.0 => format!("{:.2} Mbps", b / 1_000_000.0),
        _ => "n/a".to_string(),
    }
}

fn stat_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{label}:"))
                .weak()
                .color(egui::Color32::GRAY),
        );
        ui.label(egui::RichText::new(value).color(egui::Color32::WHITE));
    });
}

/// Plain-text diagnostic report, copied to the clipboard — the mpv-native
/// equivalent of the old app's "Copy Report" button. Field set differs from
/// the old app's (no hls.js ABR "level"/bandwidth-estimate concept applies
/// to mpv) rather than being a literal port.
fn build_report(player: &Player, now_playing: &NowPlaying) -> String {
    let pos = player.position_secs().unwrap_or(0.0);
    let dur = player.duration_secs().unwrap_or(0.0);
    format!(
        "DVRDesk (native) Playback Report\n\
         Time: {}\n\
         Title: {}\n\
         ID: {}\n\
         Manifest/stream URL: {}\n\
         Playback state: {}\n\
         Position: {:.2}s / {:.2}s\n\
         Dropped frames: {}\n\
         A/V sync: {}\n\
         Buffer ahead: {}\n\
         Video bitrate: {}\n\
         Audio bitrate: {}\n",
        chrono::Local::now().to_rfc3339(),
        now_playing.title,
        now_playing.id,
        now_playing.manifest_url.as_deref().unwrap_or("n/a"),
        if player.paused() { "paused" } else { "playing" },
        pos,
        dur,
        player
            .dropped_frames()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        player
            .avsync_secs()
            .map(|v| format!("{v:.3}s"))
            .unwrap_or_else(|| "n/a".to_string()),
        player
            .buffer_ahead_secs()
            .map(|v| format!("{v:.2}s"))
            .unwrap_or_else(|| "n/a".to_string()),
        fmt_bitrate(player.video_bitrate_bps()),
        fmt_bitrate(player.audio_bitrate_bps()),
    )
}
