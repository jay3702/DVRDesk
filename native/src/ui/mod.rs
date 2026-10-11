pub mod collections;
pub mod downloads_screen;
pub mod file_details;
pub mod guide_dialog;
pub mod library;
pub mod live;
pub mod media_card;
pub mod movies;
pub mod mpv_needed;
pub mod player_overlay;
pub mod recent;
pub mod recording_detail;
pub mod search;
pub mod settings;
pub mod thumb;
pub mod tv_shows;

/// Shared across every screen's fetch state — the Rust analogue of a
/// per-page `useState` tracking loading/error/ready, since egui has no
/// framework-level equivalent of React Suspense/query hooks.
pub enum Loaded<T> {
    Idle,
    Loading,
    Ready(T),
    Err(String),
}

/// Like `ui.selectable_label()`, but truncates its text to fit whatever
/// width is actually available, with an ellipsis, instead of wrapping to a
/// second line — matching the old (React/CSS `text-overflow: ellipsis`)
/// app's list-row behavior, and letting a resizable `SidePanel` genuinely
/// shrink down to a small width instead of stopping at "however wide the
/// longest label wants to be."
///
/// `ui.selectable_label()` itself can't do this: `SelectableLabel::ui()`
/// always lays its text out with `into_galley(ui, None, wrap_width, ...)`,
/// and a `None` wrap mode falls back to *wrapping* (a second line), not
/// truncating. Text that wraps instead of truncating is exactly what fed
/// the `SidePanel` width feedback loop described in `recent.rs` — a row
/// just slightly too wide to fit on one line reports itself as wanting
/// more horizontal room instead of clipping to what's available, and that
/// "wanted" width gets stored as the panel's own width for next frame.
/// Forcing `TextWrapMode::Truncate` explicitly (which `WidgetText::
/// into_galley` supports directly — `SelectableLabel` just never exposes
/// that choice) guarantees the rendered galley never exceeds the width it
/// was given, so content can never ask the panel to be wider than it
/// currently is. This mirrors `SelectableLabel::ui()`'s own implementation
/// almost line-for-line, only swapping that one argument.
pub fn selectable_truncated_label(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    let button_padding = ui.spacing().button_padding;
    let total_extra = button_padding + button_padding;
    let wrap_width = (ui.available_width() - total_extra.x).max(0.0);

    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        wrap_width,
        egui::TextStyle::Button,
    );

    let mut desired_size = total_extra + galley.size();
    desired_size.y = desired_size.y.max(ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    if ui.is_rect_visible(response.rect) {
        let text_pos = ui
            .layout()
            .align_size_within_rect(galley.size(), rect.shrink2(button_padding))
            .min;
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            let rect = rect.expand(visuals.expansion);
            ui.painter().rect(
                rect,
                visuals.rounding,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
            );
        }
        ui.painter().galley(text_pos, galley, visuals.text_color());
    }

    response
}

/// A small colored pill with white text — the status/tag badges on episode
/// cards and the recording detail page.
///
/// Laid out as one widget of a known size rather than a `Frame` around a
/// label: a `Frame` is placed at the cursor on the current line
/// (`available_rect_before_wrap`), so in a `horizontal_wrapped` row it
/// never moves to the next line, and badges past the edge were squeezed or
/// cut off. `allocate_exact_size` is what lets the row wrap.
pub fn badge(
    ui: &mut egui::Ui,
    text: impl Into<egui::RichText>,
    color: egui::Color32,
    padding: egui::Vec2,
) -> egui::Response {
    let galley = egui::WidgetText::from(text.into().color(egui::Color32::WHITE)).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + 2.0 * padding, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 4.0, color);
        ui.painter()
            .galley(rect.min + padding, galley, egui::Color32::WHITE);
    }
    response
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

/// Ascending/descending toggle pair, matching the old app's ▲/▼ button
/// style next to each sort dropdown — rendered as ⬆/⬇ rather than the
/// literal ▲/▼ triangles, which egui's bundled default font doesn't have
/// glyphs for (confirmed by screenshot: they rendered as tofu boxes, same
/// as `✓`/`✕` elsewhere in the app — ⬆/⬇/✔/✖ are covered, swapped in
/// throughout instead of pulling in a whole new font for four glyphs).
/// Shared by every screen with sort controls (TV Shows, Movies, Videos)
/// rather than duplicated per module.
pub fn order_buttons(ui: &mut egui::Ui, order: &mut SortOrder) -> bool {
    let mut changed = false;
    if ui.selectable_label(*order == SortOrder::Asc, "⬆").clicked() {
        *order = SortOrder::Asc;
        changed = true;
    }
    if ui
        .selectable_label(*order == SortOrder::Desc, "⬇")
        .clicked()
    {
        *order = SortOrder::Desc;
        changed = true;
    }
    changed
}

/// Reserves the paint slot for a list row's hover highlight. Call it just
/// before drawing the row and hand the result to `row_click`, which fills
/// it in — so the highlight sits *behind* the row's thumbnail and text,
/// even though whether the row is hovered is only known once it's drawn.
pub fn row_background(ui: &egui::Ui) -> egui::layers::ShapeIdx {
    ui.painter().add(egui::Shape::Noop)
}

/// Makes an entire already-drawn region (typically a thumbnail next to a
/// `selectable_truncated_label`) respond to clicks as one unit, instead of
/// only whichever inner widget happens to sense clicks on its own — by
/// default that's just the text label, since a plain thumbnail image has
/// no click sense at all. The old app's list rows were each a single
/// `<button>` wrapping both the logo and the title; this is the egui
/// equivalent, called with the `Response.rect` from the `ui.horizontal()`
/// that drew the row.
///
/// Also highlights the whole row on mouseover, into `background` (from
/// `row_background`). The label's own hover highlight never showed in
/// these rows: this click area is registered on top of it, so the label
/// itself is never the hovered widget.
pub fn row_click(
    ui: &mut egui::Ui,
    background: egui::layers::ShapeIdx,
    row_rect: egui::Rect,
    id_source: impl std::hash::Hash,
) -> egui::Response {
    let id = ui.make_persistent_id(id_source);
    let response = ui.interact(row_rect, id, egui::Sense::click());
    if response.hovered() {
        let visuals = &ui.visuals().widgets.hovered;
        ui.painter().set(
            background,
            egui::epaint::RectShape::filled(
                row_rect.expand(2.0),
                visuals.rounding,
                visuals.weak_bg_fill,
            ),
        );
    }
    response
}

/// Whether winit will pick its Wayland backend — it does whenever
/// `WAYLAND_DISPLAY`/`WAYLAND_SOCKET` is set.
pub fn wayland_session() -> bool {
    static WAYLAND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *WAYLAND.get_or_init(|| {
        cfg!(target_os = "linux")
            && (std::env::var_os("WAYLAND_DISPLAY").is_some()
                || std::env::var_os("WAYLAND_SOCKET").is_some())
    })
}

/// Hint for controls that ask the window to stay on top. Wayland gives apps
/// no way to do that (winit's `set_window_level` is a no-op there), so on a
/// Wayland session this says how to do it from GNOME's window menu instead.
/// `None` elsewhere, where the request works.
pub fn always_on_top_hint() -> Option<&'static str> {
    wayland_session().then_some(
        "Wayland doesn't let apps keep a window on top. In GNOME, press Alt+Space \
         on the window and choose Always on Top.",
    )
}

#[cfg(test)]
mod tests {
    const WIDTH: f32 = 184.0; // a media card's content width
    const BADGES: [&str; 8] = [
        "HD",
        "Dolby Digital",
        "CC",
        "TV-PG",
        "Favorited",
        "Interrupted",
        "Corrupt",
        "Recording",
    ];

    /// Lays badges out the way a media card does: a card-width
    /// `allocate_ui`, then a `horizontal_wrapped` row.
    fn layout(add: impl Fn(&mut egui::Ui, &str) -> egui::Rect) -> Vec<egui::Rect> {
        let ctx = egui::Context::default();
        let mut rects = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.allocate_ui(egui::vec2(WIDTH, 0.0), |ui| {
                    ui.set_width(WIDTH);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                        for text in BADGES {
                            rects.push(add(ui, text));
                        }
                    });
                });
            });
        });
        rects
    }

    fn fits_and_wraps(rects: &[egui::Rect]) -> bool {
        let left = rects.iter().map(|r| r.left()).fold(f32::INFINITY, f32::min);
        let fits = rects.iter().all(|r| r.right() <= left + WIDTH + 0.5);
        let mut rows: Vec<i32> = rects.iter().map(|r| r.top().round() as i32).collect();
        rows.dedup();
        fits && rows.len() > 1
    }

    #[test]
    fn badges_wrap_instead_of_overflowing() {
        let rects = layout(|ui, text| {
            super::badge(
                ui,
                egui::RichText::new(text).size(9.0),
                egui::Color32::GRAY,
                egui::vec2(4.0, 1.0),
            )
            .rect
        });
        assert!(fits_and_wraps(&rects), "{rects:?}");
    }
}
