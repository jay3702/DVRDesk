//! Full-screen playback overlay. Matches the old app's design exactly:
//! `VideoPlayer` rendered at the App root next to `<Routes>`, on top of
//! whatever page is active underneath, not as a route/Screen of its own —
//! see the plan's §3 state/navigation model.
//!
//! In picture-in-picture mode the same controls and video render into a
//! separate always-on-top native window instead, leaving the main window
//! free to browse. That works because eframe's glow backend draws every
//! viewport with one shared GL context (one surface per window), so mpv's
//! render context — created against the root window — can paint into the
//! PIP window's framebuffer unchanged.
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

/// Playback speeds offered for recordings and videos.
const SPEEDS: [f64; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// PIP window size when nothing has been shown docked yet (16:9) —
/// otherwise it opens at the docked picture's size (`pip_open_size`).
const PIP_DEFAULT_SIZE: [f32; 2] = [640.0, 360.0];
const PIP_MIN_SIZE: [f32; 2] = [240.0, 135.0];

/// The PIP window's opening size: the picture as it was shown docked — the
/// docked video area minus mpv's letterbox bars — or the default before
/// anything has been shown docked.
fn pip_open_size(docked: Option<egui::Vec2>, aspect: Option<f32>) -> egui::Vec2 {
    let Some(area) = docked.filter(|a| a.x >= 1.0 && a.y >= 1.0) else {
        return PIP_DEFAULT_SIZE.into();
    };
    let picture = match aspect {
        Some(aspect) if area.x / area.y > aspect => egui::vec2(area.y * aspect, area.y),
        Some(aspect) => egui::vec2(area.x, area.x / aspect),
        None => area,
    };
    picture.max(PIP_MIN_SIZE.into())
}

fn speed_label(speed: f64) -> String {
    format!("{speed}x")
}

#[derive(Default)]
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
    /// `Some` the frame the user picks a different playback speed.
    pub new_speed: Option<f64>,
    /// `Some` the frame the user toggles the stats-for-nerds panel.
    pub new_show_stats: Option<bool>,
    /// The user clicked Save Frame — caller picks the output path (it owns
    /// the configured cache directory) and calls `Player::save_video_frame`.
    pub save_frame: bool,
    /// `Some` the frame the user pops the video out into its own window
    /// (`true`) or docks it back (`false`, also sent when the PIP window's
    /// own close button is used).
    pub new_pip: Option<bool>,
}

/// Everything the overlay renders from — bundled so one body can be drawn
/// either docked over the main window or inside the PIP window.
struct View<'a> {
    player: &'a Player,
    now_playing: &'a NowPlaying,
    caption_tracks: &'a [CaptionTrack],
    caption_mode: CaptionMode,
    skip_ads: bool,
    speed: f64,
    showing_skip_toast: bool,
    skip_intervals: SkipIntervalsConfig,
    player_keybindings: &'a KeybindingsConfig,
    diagnostics_enabled: bool,
    show_stats: bool,
    pip: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    player: &Player,
    now_playing: &NowPlaying,
    caption_tracks: &[CaptionTrack],
    caption_mode: CaptionMode,
    skip_ads: bool,
    speed: f64,
    showing_skip_toast: bool,
    skip_intervals: SkipIntervalsConfig,
    player_keybindings: &KeybindingsConfig,
    diagnostics_enabled: bool,
    show_stats: bool,
    pip: bool,
    controls_active_since: &mut Instant,
) -> PlayerOverlayAction {
    let mut view = View {
        player,
        now_playing,
        caption_tracks,
        caption_mode,
        skip_ads,
        speed,
        showing_skip_toast,
        skip_intervals,
        player_keybindings,
        diagnostics_enabled,
        show_stats,
        pip,
    };
    let mut action = PlayerOverlayAction::default();

    let docked_size_id = egui::Id::new("player_docked_video_size");
    let pip_open_size_id = egui::Id::new("player_pip_open_size");
    if pip {
        // Opens at the size the picture had while docked, worked out once
        // when the window opens and then held, since a changed builder size
        // would resize the window again.
        let open_size = ctx.data_mut(|d| {
            d.get_temp(pip_open_size_id).unwrap_or_else(|| {
                let size = pip_open_size(d.get_temp(docked_size_id), player.video_aspect());
                d.insert_temp(pip_open_size_id, size);
                size
            })
        });
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("{} - DVRDesk", now_playing.title))
            .with_app_id("dvrdesk-native")
            .with_inner_size(open_size)
            .with_min_inner_size(PIP_MIN_SIZE)
            // No title bar or border: a border resize is driven by the
            // window manager, which draws a frame at its own size before
            // the app can correct it to the video's shape — visible as
            // flicker along the edges. The window resizes itself from
            // `pip_resize_handles` instead, and moves by dragging the video.
            .with_decorations(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop);
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("player_pip"),
            builder,
            |pip_ctx, class| {
                if class == egui::ViewportClass::Embedded {
                    // No native multi-window support on this backend —
                    // fall back to the docked overlay rather than a
                    // window that can't leave the main one.
                    view.pip = false;
                    action.new_pip = Some(false);
                    docked(pip_ctx, &view, &mut action, controls_active_since);
                    return;
                }
                snap_pip_to_aspect(pip_ctx, player);
                // The window's own close button docks the video back into
                // the main window; ending playback stays on ✖ Close.
                if pip_ctx.input(|i| i.viewport().close_requested()) {
                    action.new_pip = Some(false);
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(egui::Color32::BLACK))
                    .show(pip_ctx, |ui| {
                        let size = ui.available_size();
                        body(pip_ctx, ui, size, &view, &mut action, controls_active_since);
                    });
            },
        );
    } else {
        // Forget the last PIP window, so the next pop-out starts fresh.
        ctx.data_mut(|d| {
            d.remove::<egui::Vec2>(pip_open_size_id);
            d.remove::<PipSizing>(egui::Id::new("player_pip_sizing"));
        });
        docked(ctx, &view, &mut action, controls_active_since);
    }

    // With vsync, repainting every frame paces itself to the display. On
    // Wayland vsync is off (see main.rs), so that would spin a CPU core;
    // there mpv's new-frame callback requests repaints for the video, and
    // this only keeps the clock and control auto-hide ticking while paused.
    // The PIP window repaints along with the main one.
    if crate::ui::wayland_session() {
        ctx.request_repaint_after(Duration::from_millis(250));
    } else {
        ctx.request_repaint();
    }
    action
}

/// Resize bookkeeping for `snap_pip_to_aspect`, kept in egui's temp memory
/// since it only matters while the PIP window exists.
#[derive(Clone, Default)]
struct PipSizing {
    /// The window size seen last frame.
    last_size: Option<egui::Vec2>,
    /// The last size this code asked for — seeing it come back is our own
    /// correction landing, not the user resizing.
    requested: Option<egui::Vec2>,
    /// The last size that came from the user's resize (the window manager)
    /// rather than from us, and when. Drag direction is judged against it.
    last_external: Option<(egui::Vec2, Instant)>,
    snapped_aspect: Option<f32>,
}

/// Within this many points two window sizes count as the same — sizes are
/// rounded to whole physical pixels on the way through the window manager.
const SIZE_SLOP: f32 = 1.5;

/// External resizes closer together than this are one continuous drag.
const DRAG_GAP: Duration = Duration::from_millis(500);

fn same_size(a: egui::Vec2, b: egui::Vec2) -> bool {
    (a.x - b.x).abs() <= SIZE_SLOP && (a.y - b.y).abs() <= SIZE_SLOP
}

/// Keeps the PIP window at the video's aspect ratio: it adopts the video's
/// shape as soon as mpv knows it (and again whenever a video with a
/// different shape starts). The app's own resize handles already keep the
/// shape; this also corrects resizes it doesn't drive (keyboard resize from
/// the window menu, say) on the next frame, keeping the edge being dragged
/// — winit can't lock a window's aspect ratio.
fn snap_pip_to_aspect(ctx: &egui::Context, player: &Player) {
    let Some(aspect) = player.video_aspect() else {
        return;
    };
    let (maximized, fullscreen) = ctx.input(|i| {
        let vp = i.viewport();
        (
            vp.maximized.unwrap_or(false),
            vp.fullscreen.unwrap_or(false),
        )
    });
    if maximized || fullscreen {
        return;
    }
    // Not `viewport().inner_rect`: that needs the window's position, which
    // Wayland never reports, so it's always `None` there.
    let size = ctx.screen_rect().size();

    let id = egui::Id::new("player_pip_sizing");
    let mut state: PipSizing = ctx.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    let now = Instant::now();
    let prev = state.last_size.replace(size);

    let target = if state.snapped_aspect != Some(aspect) {
        state.snapped_aspect = Some(aspect);
        Some(egui::vec2(size.x, size.x / aspect))
    } else if prev.is_some_and(|p| same_size(p, size))
        || state.requested.is_some_and(|r| same_size(r, size))
    {
        // Unchanged, or our own correction arriving.
        None
    } else {
        // The user resized. During a drag the window manager sends sizes
        // computed from the drag's starting size plus the pointer's
        // movement, so the edge being dragged is found by comparing with
        // its previous size — not our corrected one, whose other edge it
        // never sees. A drag's first step compares with the size before it.
        let reference = match state.last_external {
            Some((ext, at)) if now.duration_since(at) < DRAG_GAP => ext,
            _ => prev.unwrap_or(size),
        };
        state.last_external = Some((size, now));
        let dw = (size.x - reference.x).abs() / reference.x.max(1.0);
        let dh = (size.y - reference.y).abs() / reference.y.max(1.0);
        Some(if dh > dw {
            egui::vec2(size.y * aspect, size.y)
        } else {
            egui::vec2(size.x, size.x / aspect)
        })
    };

    ctx.data_mut(|d| d.insert_temp(id, state));
    if let Some(target) = target {
        request_pip_size(ctx, target);
    }
}

/// Width of the invisible resize grab zones along the PIP window's edges.
const PIP_HANDLE: f32 = 8.0;

#[derive(Clone, Copy)]
enum PipEdge {
    Right,
    Bottom,
    Corner,
}

/// The window size closest to `suggested` that has the video's shape. An
/// edge drag follows that edge; a corner drag takes the nearest point on
/// the line of correctly shaped sizes. Scaled up as needed to stay at or
/// above the PIP window's minimum size.
fn fit_aspect(suggested: egui::Vec2, aspect: f32, edge: PipEdge) -> egui::Vec2 {
    let fitted = match edge {
        PipEdge::Right => egui::vec2(suggested.x, suggested.x / aspect),
        PipEdge::Bottom => egui::vec2(suggested.y * aspect, suggested.y),
        PipEdge::Corner => {
            // Sizes with the video's shape lie on (aspect, 1) * t.
            let t = (suggested.x * aspect + suggested.y) / (aspect * aspect + 1.0);
            egui::vec2(t * aspect, t)
        }
    };
    let scale = (PIP_MIN_SIZE[0] / fitted.x)
        .max(PIP_MIN_SIZE[1] / fitted.y)
        .max(1.0);
    fitted * scale
}

/// The PIP window's own resizing: the pointer's position during a drag is
/// only a suggestion, and the window is set to the nearest size with the
/// video's shape — so no frame is ever drawn at a wrong-shaped size. Right,
/// bottom and bottom-right only: growing from the left or top edge means
/// moving the window too, which Wayland doesn't let an app do.
fn pip_resize_handles(ctx: &egui::Context, ui: &mut egui::Ui, rect: egui::Rect, player: &Player) {
    let Some(aspect) = player.video_aspect() else {
        return;
    };
    let (maximized, fullscreen) = ctx.input(|i| {
        let vp = i.viewport();
        (
            vp.maximized.unwrap_or(false),
            vp.fullscreen.unwrap_or(false),
        )
    });
    if maximized || fullscreen {
        return;
    }

    let h = PIP_HANDLE;
    let zones = [
        (
            PipEdge::Right,
            egui::Rect::from_min_max(
                egui::pos2(rect.right() - h, rect.top()),
                egui::pos2(rect.right(), rect.bottom() - h),
            ),
            egui::CursorIcon::ResizeEast,
        ),
        (
            PipEdge::Bottom,
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - h),
                egui::pos2(rect.right() - h, rect.bottom()),
            ),
            egui::CursorIcon::ResizeSouth,
        ),
        (
            PipEdge::Corner,
            egui::Rect::from_min_max(
                rect.right_bottom() - egui::vec2(2.0 * h, 2.0 * h),
                rect.right_bottom(),
            ),
            egui::CursorIcon::ResizeSouthEast,
        ),
    ];
    let grab_id = egui::Id::new("player_pip_resize_grab");
    for (i, (edge, zone, cursor)) in zones.into_iter().enumerate() {
        let response = ui.interact(
            zone,
            egui::Id::new(("player_pip_resize", i)),
            egui::Sense::drag(),
        );
        if response.hovered() || response.dragged() {
            ctx.set_cursor_icon(cursor);
        }
        let Some(pointer) = response.interact_pointer_pos() else {
            continue;
        };
        // Window coordinates start at its top-left corner, which stays put
        // as it resizes; the grab offset keeps the edge under the pointer
        // where it was grabbed instead of jumping to the pointer itself.
        let pointer = pointer - rect.min;
        if response.drag_started() {
            let offset = rect.size() - pointer;
            ctx.data_mut(|d| d.insert_temp(grab_id, offset));
        }
        if response.dragged() {
            let offset: egui::Vec2 = ctx.data_mut(|d| d.get_temp(grab_id)).unwrap_or_default();
            let target = fit_aspect(pointer + offset, aspect, edge);
            request_pip_size(ctx, target);
        }
    }
}

/// Resizes the PIP window, recording the request so `snap_pip_to_aspect`
/// recognizes the resulting size as ours rather than a user resize.
fn request_pip_size(ctx: &egui::Context, target: egui::Vec2) {
    if same_size(target, ctx.screen_rect().size()) {
        return;
    }
    let id = egui::Id::new("player_pip_sizing");
    let mut state: PipSizing = ctx.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target));
    state.requested = Some(target);
    ctx.data_mut(|d| d.insert_temp(id, state));
}

/// The overlay covering the whole main window.
fn docked(
    ctx: &egui::Context,
    view: &View,
    action: &mut PlayerOverlayAction,
    controls_active_since: &mut Instant,
) {
    let screen_rect = ctx.screen_rect();
    egui::Area::new(egui::Id::new("player_overlay"))
        .fixed_pos(screen_rect.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            body(
                ctx,
                ui,
                screen_rect.size(),
                view,
                action,
                controls_active_since,
            );
        });
}

/// Controls, scrubber and video, filling `size`. `ctx` is whichever
/// viewport this is drawn in — input (shortcuts, pointer activity) is read
/// per viewport, so while popped out only keys pressed in the PIP window
/// drive playback, and typing in the main window's search box doesn't.
fn body(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    size: egui::Vec2,
    view: &View,
    action: &mut PlayerOverlayAction,
    controls_active_since: &mut Instant,
) {
    let ad_blocks = commercial_skip::ad_blocks(&view.now_playing.commercials);

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
    //
    // Only *reads* the keys inside `ctx.input`: that closure holds egui's
    // context lock, and mpv calls made under it could deadlock — mpv's
    // core can be waiting on a thread that's itself waiting for that lock.
    let player = view.player;
    let skip_intervals = view.skip_intervals;
    let kb = view.player_keybindings;
    let [skip_forward, skip_back, fast_forward, fast_reverse, play_pause, close] =
        ctx.input(|input| {
            [
                &kb.skip_forward,
                &kb.skip_back,
                &kb.fast_forward,
                &kb.fast_reverse,
                &kb.play_pause,
                &kb.close,
            ]
            .map(|keys| keybindings::any_pressed(input, keys))
        });
    if skip_forward || skip_back || fast_forward || fast_reverse {
        let pos = player.position_secs().unwrap_or(0.0);
        let dur = player.duration_secs().unwrap_or(f64::MAX);
        if skip_forward {
            player.seek((pos + skip_intervals.skip_forward as f64).min(dur));
        }
        if skip_back {
            player.seek((pos - skip_intervals.skip_back as f64).max(0.0));
        }
        if fast_forward {
            player.seek((pos + skip_intervals.fast_forward as f64).min(dur));
        }
        if fast_reverse {
            player.seek((pos - skip_intervals.fast_reverse as f64).max(0.0));
        }
    }
    if play_pause {
        player.set_pause(!player.paused());
    }
    if close {
        action.close = true;
    }

    let has_py_captions = view
        .caption_tracks
        .iter()
        .any(|t| t.kind == CaptionKind::PyCaptions);
    let has_broadcast = view
        .caption_tracks
        .iter()
        .any(|t| t.kind == CaptionKind::Broadcast);

    let mut controls = |ui: &mut egui::Ui| {
        // Wrapped so a small PIP window reflows the buttons onto
        // extra rows instead of clipping them off the right edge.
        ui.horizontal_wrapped(|ui| {
            // The PIP window already carries the title in its own
            // title bar.
            if !view.pip {
                ui.heading(
                    egui::RichText::new(&view.now_playing.title).color(egui::Color32::WHITE),
                );
            }

            if ui
                .button(format!("◀ {}s", view.skip_intervals.skip_back))
                .clicked()
            {
                let pos = view.player.position_secs().unwrap_or(0.0);
                view.player
                    .seek((pos - view.skip_intervals.skip_back as f64).max(0.0));
            }
            if ui
                .button(if view.player.paused() {
                    "▶ Resume"
                } else {
                    "⏸ Pause"
                })
                .clicked()
            {
                view.player.set_pause(!view.player.paused());
            }
            if ui
                .button(format!("{}s ▶", view.skip_intervals.skip_forward))
                .clicked()
            {
                let pos = view.player.position_secs().unwrap_or(0.0);
                let dur = view.player.duration_secs().unwrap_or(f64::MAX);
                view.player
                    .seek((pos + view.skip_intervals.skip_forward as f64).min(dur));
            }

            // Matches the old app's dropdown exactly: only shown
            // once at least one caption track is actually
            // available, same "Off / Broadcast / Py-Captions"
            // options.
            if has_py_captions || has_broadcast {
                let current_label = match view.caption_mode {
                    CaptionMode::Off => "CC: Off",
                    CaptionMode::PyCaptions => "CC: Py-Captions",
                    CaptionMode::Broadcast => "CC: Broadcast",
                };
                egui::ComboBox::from_id_salt("caption_mode")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(view.caption_mode == CaptionMode::Off, "Off")
                            .clicked()
                        {
                            action.new_caption_mode = Some(CaptionMode::Off);
                        }
                        if has_broadcast
                            && ui
                                .selectable_label(
                                    view.caption_mode == CaptionMode::Broadcast,
                                    "Broadcast",
                                )
                                .clicked()
                        {
                            action.new_caption_mode = Some(CaptionMode::Broadcast);
                        }
                        if has_py_captions
                            && ui
                                .selectable_label(
                                    view.caption_mode == CaptionMode::PyCaptions,
                                    "Py-Captions",
                                )
                                .clicked()
                        {
                            action.new_caption_mode = Some(CaptionMode::PyCaptions);
                        }
                    });
            }

            if !view.now_playing.live {
                egui::ComboBox::from_id_salt("playback_speed")
                    .selected_text(format!("Speed: {}", speed_label(view.speed)))
                    .show_ui(ui, |ui| {
                        for s in SPEEDS {
                            if ui
                                .selectable_label(view.speed == s, speed_label(s))
                                .clicked()
                            {
                                action.new_speed = Some(s);
                            }
                        }
                    });
            }

            if !ad_blocks.is_empty() {
                let label = if view.skip_ads {
                    "⏭ Skip Ads: On"
                } else {
                    "⏭ Skip Ads: Off"
                };
                if ui.button(label).clicked() {
                    action.new_skip_ads = Some(!view.skip_ads);
                }
            }

            if view.diagnostics_enabled {
                let label = if view.show_stats {
                    "Hide Stats"
                } else {
                    "Stats"
                };
                if ui.button(label).clicked() {
                    action.new_show_stats = Some(!view.show_stats);
                }
                if ui.button("Copy Report").clicked() {
                    ctx.copy_text(build_report(view.player, view.now_playing));
                }
                if ui.button("Save Frame").clicked() {
                    action.save_frame = true;
                }
            }

            // Plain text, not an icon glyph — this
            // codebase has repeatedly found egui's bundled
            // fonts don't cover every Unicode symbol that
            // looks plausible (✕/▲/▼ all needed swapping
            // out earlier), and there's no already-proven-
            // safe glyph for "fullscreen" to reach for.
            let is_fullscreen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
            let fullscreen_label = if is_fullscreen {
                "Exit Fullscreen"
            } else {
                "Fullscreen"
            };
            if ui.button(fullscreen_label).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen));
            }

            let pip_label = if view.pip { "Dock" } else { "Pop Out" };
            let pip_hint = match crate::ui::always_on_top_hint() {
                Some(hint) => {
                    format!("Picture-in-picture: play in a separate, resizable window.\n\n{hint}")
                }
                None => "Picture-in-picture: play in a separate, resizable window".to_string(),
            };
            if ui.button(pip_label).on_hover_text(pip_hint).clicked() {
                action.new_pip = Some(!view.pip);
            }

            if ui.button("✖ Close").clicked() {
                action.close = true;
            }
        });

        let pos = view.player.position_secs().unwrap_or(0.0);
        let dur = view.player.duration_secs().unwrap_or(0.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{}  /  {}", fmt_secs(pos), fmt_secs(dur)))
                    .color(egui::Color32::LIGHT_GRAY),
            );
            if view.showing_skip_toast {
                ui.label(egui::RichText::new("Skipping commercial…").color(egui::Color32::YELLOW));
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
                let left =
                    bar_rect.left() + (start.max(0.0) / dur).min(1.0) as f32 * bar_rect.width();
                let right =
                    bar_rect.left() + (end.max(0.0) / dur).min(1.0) as f32 * bar_rect.width();
                let seg = egui::Rect::from_min_max(
                    egui::pos2(left, bar_rect.top()),
                    egui::pos2(right.max(left + 1.0), bar_rect.bottom()),
                );
                painter.rect_filled(seg, 1.0, egui::Color32::from_rgb(180, 60, 40));
            }
            let playhead_x =
                bar_rect.left() + (pos / dur).clamp(0.0, 1.0) as f32 * bar_rect.width();
            painter.line_segment(
                [
                    egui::pos2(playhead_x, bar_rect.top() - 2.0),
                    egui::pos2(playhead_x, bar_rect.bottom() + 2.0),
                ],
                egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
            );

            if response.clicked() || response.dragged() {
                if let Some(interact_pos) = response.interact_pointer_pos() {
                    let frac = ((interact_pos.x - bar_rect.left()) / bar_rect.width())
                        .clamp(0.0, 1.0) as f64;
                    view.player.seek(frac * dur);
                }
            }
            ui.add_space(4.0);
        }
    };

    if view.pip {
        // The video fills the window and the controls float over its top
        // edge, so showing or hiding them never changes the video's size —
        // with the window held at the video's aspect ratio (see
        // `snap_pip_to_aspect`), the window is exactly the picture once
        // they fade.
        let (rect, video) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        ui.painter()
            .add(crate::player::paint_callback(view.player, rect, ctx));
        // Stand-ins for the missing title bar. `drag_started` only fires
        // once the pointer moves past egui's drag threshold, so a plain
        // double-click never hands the pointer to the window manager.
        if video.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if video.double_clicked() {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }
        if controls_visible {
            let mut overlay = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            egui::Frame::default()
                .fill(egui::Color32::from_black_alpha(170))
                .inner_margin(6.0)
                .show(&mut overlay, |ui| {
                    ui.set_width(ui.available_width());
                    controls(ui);
                });
        }
        // Last, so the handles win over the video's move-drag beneath them.
        pip_resize_handles(ctx, ui, rect, view.player);
    } else {
        // No margin once controls are hidden — a live channel's video
        // should fill the frame edge to edge, not just have empty space
        // where the header/scrubber used to be.
        egui::Frame::default()
            .fill(egui::Color32::BLACK)
            .inner_margin(if controls_visible { 8.0 } else { 0.0 })
            .show(ui, |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);

                if controls_visible {
                    controls(ui);
                }

                let available = ui.available_size();
                let (rect, _response) = ui.allocate_exact_size(available, egui::Sense::hover());
                ui.painter()
                    .add(crate::player::paint_callback(view.player, rect, ctx));
                // The PIP window opens at this size (see `show`).
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("player_docked_video_size"), rect.size())
                });
            });
    }

    // Rendered as its own floating Area, on top of everything (including the
    // main overlay's own Order::Foreground content, i.e. the video) rather
    // than as a sibling in the main overlay's vertical layout — it used to
    // live inline there, which meant `ui.available_size()` for the video
    // rect was computed *after* the stats panel had already consumed
    // vertical space, visibly pushing the video down instead of floating
    // over it.
    if view.diagnostics_enabled && view.show_stats {
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
                            if view.player.paused() {
                                "paused"
                            } else {
                                "playing"
                            },
                        );
                        stat_row(
                            ui,
                            "Dropped frames",
                            &view
                                .player
                                .dropped_frames()
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "n/a".to_string()),
                        );
                        stat_row(
                            ui,
                            "Decoder drops",
                            &view
                                .player
                                .decoder_dropped_frames()
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "n/a".to_string()),
                        );
                        stat_row(
                            ui,
                            "HW decoder",
                            &view
                                .player
                                .hwdec_current()
                                .unwrap_or_else(|| "n/a".to_string()),
                        );
                        stat_row(
                            ui,
                            "A/V sync",
                            &view
                                .player
                                .avsync_secs()
                                .map(|v| format!("{v:.3}s"))
                                .unwrap_or_else(|| "n/a".to_string()),
                        );
                        stat_row(
                            ui,
                            "Buffer ahead",
                            &view
                                .player
                                .buffer_ahead_secs()
                                .map(|v| format!("{v:.2}s"))
                                .unwrap_or_else(|| "n/a".to_string()),
                        );
                        stat_row(
                            ui,
                            "Bitrate (v/a)",
                            &format!(
                                "{} / {}",
                                fmt_bitrate(view.player.video_bitrate_bps()),
                                fmt_bitrate(view.player.audio_bitrate_bps())
                            ),
                        );
                        stat_row(
                            ui,
                            "Caption tracks",
                            &if view.caption_tracks.is_empty() {
                                "none detected".to_string()
                            } else {
                                view.caption_tracks
                                    .iter()
                                    .map(|t| format!("sid={} {:?}", t.sid, t.kind))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            },
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "Manifest: {}",
                                view.now_playing.manifest_url.as_deref().unwrap_or("n/a")
                            ))
                            .small()
                            .color(egui::Color32::DARK_GRAY),
                        );
                    });
            });
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
         Decoder drops: {}\n\
         HW decoder: {}\n\
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
            .decoder_dropped_frames()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        player.hwdec_current().unwrap_or_else(|| "n/a".to_string()),
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
