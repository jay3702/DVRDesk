//! Port of `RecentRecordings.tsx` — the simplest page, chosen first in the
//! plan specifically to validate list/selection/detail/incremental-render
//! patterns before the more complex screens reuse them.

use chrono::{DateTime, Local, TimeZone};

use super::Loaded;
use crate::api::types::{Channel, Recording};

const INITIAL_VISIBLE_DAY_GROUPS: usize = 3;
const VISIBLE_DAY_GROUPS_STEP: usize = 2;

pub struct RecentState {
    pub items: Loaded<Vec<Recording>>,
    /// Fetched purely to build a channel/network logo lookup for list
    /// rows — Recent doesn't otherwise need channel data. A separate copy
    /// from `App.live.channels` rather than a shared cache, matching this
    /// codebase's existing pattern of each screen owning its own fetch
    /// (see Search's doc comment for the same tradeoff, made explicitly).
    pub channels: Loaded<Vec<Channel>>,
    pub selected_id: Option<String>,
    pub visible_day_groups: usize,
    /// Which day the ⬆/⬇ "jump to day" control last moved to — an index
    /// into the day groups computed fresh every frame, not organic scroll
    /// position (reading that back out of a `ScrollArea` mid-scroll isn't
    /// something egui exposes cleanly), so each click moves exactly one day
    /// from wherever the last click left off.
    pub current_day_idx: usize,
    /// One-shot: set by an ⬆/⬇ click, consumed (and cleared) the same frame
    /// by calling `Response::scroll_to_me` on that day's heading — the
    /// standard egui mechanism for "make this widget visible," used here
    /// instead of manually computing a scroll offset (unlike the guide grid,
    /// which paints its own rows via `Painter` and has no real per-row
    /// widget to ask this of).
    pub pending_scroll_to_day: bool,
    /// When `items` was last (re)fetched — drives a periodic background
    /// refresh while this screen is open, since nothing else tells this app
    /// a new recording exists server-side (confirmed: no push/webhook
    /// mechanism, and the fetch-once-per-session `Loaded::Idle` gate every
    /// other screen also uses means a recording made an hour into a long
    /// session would otherwise never appear until the app restarted).
    pub last_fetched: Option<std::time::Instant>,
}

impl Default for RecentState {
    fn default() -> Self {
        Self {
            items: Loaded::Idle,
            channels: Loaded::Idle,
            selected_id: None,
            visible_day_groups: INITIAL_VISIBLE_DAY_GROUPS,
            current_day_idx: 0,
            pending_scroll_to_day: false,
            last_fetched: None,
        }
    }
}

/// Play intent and detail-pane actions handed back to the caller — this
/// module doesn't own the API/async layer or the Player.
#[derive(Default)]
pub struct RecentAction {
    pub play: Option<Recording>,
    pub toggle_watched: Option<(String, bool)>,
    pub trash: Option<String>,
    pub download: Option<Recording>,
}

struct TimeGroup {
    label: String,
    items: Vec<Recording>,
}

struct DayGroup {
    label: String,
    time_groups: Vec<TimeGroup>,
}

fn local_dt(ts_millis: i64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt(ts_millis)
        .single()
        .unwrap_or_else(Local::now)
}

fn day_label(ts_millis: i64) -> String {
    local_dt(ts_millis).format("%A, %B %-d, %Y").to_string()
}

fn time_label(ts_millis: i64) -> String {
    local_dt(ts_millis).format("%-I:%M %p").to_string()
}

/// Recordings are assumed pre-sorted desc by date from the API (matches the
/// old code's own assumption), bucketed into day groups then time-of-day
/// sub-groups — identical HH:MM strings collapse into one sub-header,
/// modeling "everything that started recording at this exact time."
fn group_by_day_time(recordings: &[Recording]) -> Vec<DayGroup> {
    let mut days: Vec<DayGroup> = Vec::new();
    for rec in recordings {
        let d_label = day_label(rec.created_at);
        let t_label = time_label(rec.created_at);

        let need_new_day = !matches!(days.last(), Some(d) if d.label == d_label);
        if need_new_day {
            days.push(DayGroup {
                label: d_label,
                time_groups: Vec::new(),
            });
        }
        let day = days.last_mut().unwrap();

        let need_new_time = !matches!(day.time_groups.last(), Some(t) if t.label == t_label);
        if need_new_time {
            day.time_groups.push(TimeGroup {
                label: t_label,
                items: Vec::new(),
            });
        }
        day.time_groups.last_mut().unwrap().items.push(rec.clone());
    }
    days
}

/// Single-click selects, double-click (or the detail pane's Play button)
/// plays — matches the old page's interaction exactly. Keyboard nav relies
/// on egui's own Tab-focus system rather than custom arrow-key handling
/// (a first attempt at the latter fought with egui's built-in focus
/// handling in ways that couldn't be cleanly suppressed) — Tab/Shift+Tab
/// already cycles through these rows in order, and a focused row's
/// `Response::clicked()` already returns true on Enter/Space natively, so
/// selecting via keyboard needs no extra code beyond the `.clicked()`
/// check already here for mouse clicks.
pub fn show(ui: &mut egui::Ui, state: &mut RecentState, server_url: &str) -> RecentAction {
    let mut action = RecentAction::default();

    match &state.items {
        Loaded::Idle | Loaded::Loading => {
            ui.label("Loading…");
        }
        Loaded::Err(e) => {
            ui.colored_label(egui::Color32::RED, e);
        }
        Loaded::Ready(recordings) => {
            if recordings.is_empty() {
                ui.label("No recordings.");
                return action;
            }

            let groups = group_by_day_time(recordings);
            let selected_id = state.selected_id.clone();
            let channel_logos = if let Loaded::Ready(channels) = &state.channels {
                crate::api::live::build_channel_logo_map(channels, server_url)
            } else {
                std::collections::HashMap::new()
            };

            egui::SidePanel::left("recent_list")
                .resizable(true)
                .default_width(360.0)
                .show_inside(ui, |ui| {
                    // `horizontal_wrapped`, not `horizontal` — see the
                    // matching comment in `movies.rs` for why.
                    ui.horizontal_wrapped(|ui| {
                        ui.heading("Recent Recordings");
                        ui.separator();
                        ui.label("Day:");
                        if ui.button("⬆").on_hover_text("Previous day").clicked() {
                            state.current_day_idx = state.current_day_idx.saturating_sub(1);
                            state.pending_scroll_to_day = true;
                        }
                        if ui
                            .add_enabled(
                                state.current_day_idx + 1 < groups.len(),
                                egui::Button::new("⬇"),
                            )
                            .on_hover_text("Next day")
                            .clicked()
                        {
                            state.current_day_idx += 1;
                            state.pending_scroll_to_day = true;
                        }
                    });
                    // The target day might not be incrementally revealed
                    // yet — reveal up through it so there's an actual
                    // heading for `scroll_to_me` to jump to below.
                    if state.visible_day_groups <= state.current_day_idx {
                        state.visible_day_groups = state.current_day_idx + 1;
                    }
                    ui.separator();

                    // `auto_shrink`'s default is `[true, true]` — the
                    // ScrollArea (and thus this SidePanel's own reported
                    // content rect, which `SidePanel` stores as *the*
                    // panel width for next frame) shrinks to fit content's
                    // natural width instead of filling the panel's
                    // allocated width. Left un-set, that silently
                    // overwrites the panel's actual width with "whatever
                    // the widest row happens to need" every single frame —
                    // this is what made the divider seem to clip content
                    // and snap back instead of staying wherever it was
                    // dragged to.
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (day_idx, day) in
                                groups.iter().take(state.visible_day_groups).enumerate()
                            {
                                // `.wrap()`, not `.truncate()` — the day
                                // heading should stay fully readable, just
                                // wrap to a second line rather than push the
                                // panel wider (same feedback-loop concern as
                                // the row labels below).
                                let heading_resp = ui.add(
                                    egui::Label::new(egui::RichText::new(&day.label).heading())
                                        .wrap(),
                                );
                                if state.pending_scroll_to_day && day_idx == state.current_day_idx {
                                    heading_resp.scroll_to_me(Some(egui::Align::TOP));
                                }
                                for tg in &day.time_groups {
                                    ui.label(egui::RichText::new(&tg.label).weak());
                                    for rec in &tg.items {
                                        let is_selected =
                                            selected_id.as_deref() == Some(rec.id.as_str());
                                        let label = match &rec.episode_title {
                                            Some(ep) => format!("{} — {}", rec.title, ep),
                                            None => rec.title.clone(),
                                        };
                                        let logo = crate::api::live::logo_for_channel_key(
                                            rec.channel.as_deref(),
                                            &channel_logos,
                                        );
                                        let row = ui.horizontal(|ui| {
                                            super::thumb::show(
                                                ui,
                                                server_url,
                                                logo.as_deref(),
                                                egui::vec2(64.0, 36.0),
                                            );
                                            super::selectable_truncated_label(
                                                ui,
                                                is_selected,
                                                &label,
                                            );
                                        });
                                        let resp = super::row_click(
                                            ui,
                                            row.response.rect,
                                            ("recent_row", &rec.id),
                                        );
                                        if resp.double_clicked() {
                                            action.play = Some(rec.clone());
                                        } else if resp.clicked() {
                                            state.selected_id = Some(rec.id.clone());
                                            resp.request_focus();
                                        }
                                    }
                                }
                                ui.separator();
                            }
                            if state.visible_day_groups < groups.len() {
                                if ui.button("Load more").clicked() {
                                    state.visible_day_groups += VISIBLE_DAY_GROUPS_STEP;
                                }
                            }
                        });
                    state.pending_scroll_to_day = false;
                });

            match state
                .selected_id
                .as_deref()
                .and_then(|id| recordings.iter().find(|r| r.id == id))
            {
                Some(rec) => {
                    let detail = super::recording_detail::show(ui, rec, server_url);
                    if detail.play {
                        action.play = Some(rec.clone());
                    }
                    if let Some(watched) = detail.toggle_watched {
                        action.toggle_watched = Some((rec.id.clone(), watched));
                    }
                    if detail.download {
                        action.download = Some(rec.clone());
                    }
                    if detail.trash {
                        action.trash = Some(rec.id.clone());
                        state.selected_id = None;
                    }
                }
                None => {
                    ui.add_space(8.0);
                    ui.weak("Select a recording to see details.");
                }
            }
        }
    }

    action
}
