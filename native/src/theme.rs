//! Custom `egui::Visuals` (dark + light), ported from the old Tauri/React
//! app's own actual palette (`../../src/themes.css` at the repo root — the
//! React app, not this crate) rather than relying on egui's stock gray/blue
//! defaults. Each starts from `egui::Visuals::dark()`/`::light()` so any
//! field not explicitly listed here keeps a sane egui default instead of
//! being left zeroed.
//!
//! Applied once, in `App::new()`, via `ctx.style_mut_of(Theme::Dark/Light,
//! |s| s.visuals = ...)` — confirmed via egui's own source that this sets
//! the two per-theme styles independently and doesn't fight the existing
//! per-frame `ctx.set_theme(settings.theme)` call, which only *selects*
//! between them.

use egui::{Color32, Rounding, Stroke, Visuals};

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

const ROUNDING: Rounding = Rounding::same(4.0);

pub fn dark_visuals() -> Visuals {
    let mut v = Visuals::dark();

    v.panel_fill = rgb(0x12121e); // --wc-bg
    v.widgets.noninteractive.bg_fill = rgb(0x12121e); // --wc-bg
    v.window_fill = rgb(0x1e1e2e); // --wc-bg-surface
    v.code_bg_color = rgb(0x111826); // --wc-bg-surface-2
    v.extreme_bg_color = rgb(0x1a1a28); // --wc-bg-input
    v.faint_bg_color = rgb(0x1e1e30); // --wc-bg-hover
    v.hyperlink_color = rgb(0x0078d4); // --wc-accent
    v.warn_fg_color = rgb(0xe07b00); // --wc-warn
    v.error_fg_color = rgb(0xe06c75); // --wc-error

    v.selection.bg_fill = rgb(0x1a3a5c); // --wc-bg-active
    v.selection.stroke = Stroke::new(1.0_f32, rgb(0x0078d4)); // --wc-accent

    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, rgb(0x2a2a3a)); // --wc-border
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, rgb(0xd4d4d4)); // --wc-text
    v.widgets.noninteractive.rounding = ROUNDING;

    v.widgets.inactive.bg_fill = rgb(0x2a2f42); // --wc-bg-btn-secondary
    v.widgets.inactive.weak_bg_fill = rgb(0x2a2f42);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, rgb(0xd4d4d4)); // --wc-text
    v.widgets.inactive.rounding = ROUNDING;

    v.widgets.hovered.bg_fill = rgb(0x38415f); // --wc-bg-btn-secondary-hover
    v.widgets.hovered.weak_bg_fill = rgb(0x38415f);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, rgb(0x0078d4)); // --wc-border-focus
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, rgb(0xe0e0e0)); // --wc-text-heading
    v.widgets.hovered.rounding = ROUNDING;

    v.widgets.active.bg_fill = rgb(0x1a3a5c); // --wc-bg-active
    v.widgets.active.weak_bg_fill = rgb(0x1a3a5c);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, rgb(0x60b0f0)); // --wc-text-active
    v.widgets.active.rounding = ROUNDING;

    v.widgets.open.bg_fill = rgb(0x1a3a5c);
    v.widgets.open.weak_bg_fill = rgb(0x1a3a5c);
    v.widgets.open.rounding = ROUNDING;

    v
}

pub fn light_visuals() -> Visuals {
    let mut v = Visuals::light();

    v.panel_fill = rgb(0xf0f2f5); // --wc-bg
    v.widgets.noninteractive.bg_fill = rgb(0xf0f2f5); // --wc-bg
    v.window_fill = rgb(0xffffff); // --wc-bg-surface
    v.code_bg_color = rgb(0xeef1f8); // --wc-bg-surface-2
    v.extreme_bg_color = rgb(0xffffff); // --wc-bg-input
    v.faint_bg_color = rgb(0xe6e9f0); // --wc-bg-hover
    v.hyperlink_color = rgb(0x0078d4); // --wc-accent
    v.warn_fg_color = rgb(0xc86a00); // --wc-warn
    v.error_fg_color = rgb(0xc62828); // --wc-error

    v.selection.bg_fill = rgb(0xcde4ff); // --wc-bg-active
    v.selection.stroke = Stroke::new(1.0_f32, rgb(0x0078d4)); // --wc-accent

    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, rgb(0xcdd1dd)); // --wc-border
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, rgb(0x1a1a2e)); // --wc-text
    v.widgets.noninteractive.rounding = ROUNDING;

    v.widgets.inactive.bg_fill = rgb(0xdde1ed); // --wc-bg-btn-secondary
    v.widgets.inactive.weak_bg_fill = rgb(0xdde1ed);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, rgb(0x1a1a2e)); // --wc-text
    v.widgets.inactive.rounding = ROUNDING;

    v.widgets.hovered.bg_fill = rgb(0xccd0e0); // --wc-bg-btn-secondary-hover
    v.widgets.hovered.weak_bg_fill = rgb(0xccd0e0);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, rgb(0x0078d4)); // --wc-border-focus
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, rgb(0x0d0d1a)); // --wc-text-heading
    v.widgets.hovered.rounding = ROUNDING;

    v.widgets.active.bg_fill = rgb(0xcde4ff); // --wc-bg-active
    v.widgets.active.weak_bg_fill = rgb(0xcde4ff);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, rgb(0x0060b0)); // --wc-text-active
    v.widgets.active.rounding = ROUNDING;

    v.widgets.open.bg_fill = rgb(0xcde4ff);
    v.widgets.open.weak_bg_fill = rgb(0xcde4ff);
    v.widgets.open.rounding = ROUNDING;

    v
}
