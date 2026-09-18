//! Thumbnail/poster/logo rendering, built on egui's own image-loader
//! pipeline (`egui_extras::install_image_loaders`, wired once in
//! `App::new`) rather than a hand-rolled fetch/cache/texture pipeline —
//! egui already does the HTTP fetch, decode, GPU upload, and in-memory
//! cache-by-URI for us. Deferred from earlier phases specifically to reuse
//! this instead of building it manually.
//!
//! Channels DVR returns image fields inconsistently: some (`Show.image_url`
//! in particular) are already full absolute URLs, others (`thumbnail_url`)
//! are paths relative to the server root — confirmed against the old app's
//! `MediaCard.tsx`, which explicitly checks for an `http` prefix before
//! deciding whether to prepend the server base. `resolve` handles both.

pub fn resolve(server_url: &str, raw: &str) -> String {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("{}{}", server_url.trim_end_matches('/'), raw)
    }
}

/// Renders a rounded thumbnail at exactly `size` if `raw` is a non-empty
/// URL, otherwise renders nothing (no placeholder box) so text-only rows
/// without art don't get an awkward empty gap. Load failures fall back to
/// egui's own built-in error glyph rather than anything custom.
pub fn show(ui: &mut egui::Ui, server_url: &str, raw: Option<&str>, size: egui::Vec2) {
    let Some(raw) = raw else { return };
    if raw.is_empty() {
        return;
    }
    let url = resolve(server_url, raw);
    ui.add(
        egui::Image::new(url)
            .fit_to_exact_size(size)
            .rounding(4.0)
            .show_loading_spinner(true),
    );
}

/// Renders scaled to `max_width` with the image's natural aspect ratio
/// (height not fixed) — for poster-style art like a TV show's series image,
/// which the old app's CSS only ever constrained by width (`width: 160px`,
/// no fixed height), unlike the exact-box thumbnails `show()` renders for
/// list rows.
pub fn show_max_width(ui: &mut egui::Ui, server_url: &str, raw: Option<&str>, max_width: f32) {
    let Some(raw) = raw else { return };
    if raw.is_empty() {
        return;
    }
    let url = resolve(server_url, raw);
    ui.add(
        // `fit_to_original_size` (fit by the image's own natural size,
        // shrunk only if it exceeds `max_size`) instead of the default
        // `ImageFit::Fraction` — Fraction scales against the *available*
        // size in the `Ui`, and inside a `horizontal()` layout the
        // available height at the point this is placed is whatever the
        // row's height happens to be so far (often just a line of text),
        // which was making the image render tiny regardless of
        // `max_width`.
        egui::Image::new(url)
            .fit_to_original_size(1.0)
            .max_width(max_width)
            .rounding(8.0)
            .show_loading_spinner(true),
    );
}
