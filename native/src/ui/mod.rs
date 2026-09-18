pub mod collections;
pub mod downloads_screen;
pub mod guide_dialog;
pub mod library;
pub mod live;
pub mod media_card;
pub mod movies;
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

/// Makes an entire already-drawn region (typically a thumbnail next to a
/// `selectable_truncated_label`) respond to clicks as one unit, instead of
/// only whichever inner widget happens to sense clicks on its own — by
/// default that's just the text label, since a plain thumbnail image has
/// no click sense at all. The old app's list rows were each a single
/// `<button>` wrapping both the logo and the title; this is the egui
/// equivalent, called with the `Response.rect` from the `ui.horizontal()`
/// that drew the row.
pub fn row_click(
    ui: &mut egui::Ui,
    row_rect: egui::Rect,
    id_source: impl std::hash::Hash,
) -> egui::Response {
    let id = ui.make_persistent_id(id_source);
    ui.interact(row_rect, id, egui::Sense::click())
}
