//! The "record this / create a pass" dialog opened by clicking a future
//! program in the Live guide grid. Kept as its own module rather than
//! folded into `live.rs`, matching this codebase's one-concern-per-file
//! convention (e.g. `recording_detail.rs` vs. the screens that use it).
//!
//! This module only renders and reports intent — it never touches the
//! network itself. `live.rs`/`app.rs` own the async dispatch (spawning
//! `api::guide::create_job`/`create_pass`, refreshing jobs/rules on
//! success) and feed the result back in via `RecordDialogState`'s fields,
//! the same `Loaded<T>`-driven pattern used everywhere else in this app.

use crate::api::guide::GuideProgram;
use crate::api::types::Channel;

use super::Loaded;

pub struct RecordDialogState {
    pub channel: Channel,
    pub program: GuideProgram,
    /// Whether a pass already exists for this program's series — hides the
    /// "Create Pass" button in favor of a note, since editing/deleting an
    /// existing pass isn't offered by this dialog (only creating one was
    /// asked for).
    pub existing_pass: bool,
    /// The native `Airing` JSON needed to actually create a job/pass —
    /// `Loading` for series content until `fetch_series_airings` resolves
    /// and the matching slot is found; filled in synchronously (no network
    /// round trip needed) for series-less content via
    /// `api::guide::build_fallback_airing`.
    pub airing: Loaded<serde_json::Value>,
    pub pad_start_min: i64,
    pub pad_end_min: i64,
    pub new_only: bool,
    pub busy: bool,
    pub message: Option<Result<String, String>>,
}

impl RecordDialogState {
    pub fn new(
        channel: Channel,
        program: GuideProgram,
        pad_start_secs: i64,
        pad_end_secs: i64,
    ) -> Self {
        Self {
            channel,
            program,
            existing_pass: false,
            airing: Loaded::Loading,
            pad_start_min: (pad_start_secs / 60).max(0),
            pad_end_min: (pad_end_secs / 60).max(0),
            new_only: true,
            busy: false,
            message: None,
        }
    }
}

pub enum DialogAction {
    None,
    Close,
    Record {
        name: String,
        time: i64,
        duration: i64,
        channels: Vec<String>,
        airing: serde_json::Value,
    },
    CreatePass {
        name: String,
        image: Option<String>,
        series_id: String,
        new_only: bool,
        pad_start: i64,
        pad_end: i64,
    },
}

/// `center` is the middle of the guide grid's own display area (not the
/// whole window — that would sit the dialog behind the sidebar's visual
/// center rather than the grid the user actually clicked in). Only applied
/// via `default_pos`, not every frame, so the window stays draggable once
/// open — and the `Id` is keyed on the specific program (channel + start),
/// not a fixed string, so a *different* program clicked later starts fresh
/// at the new center instead of reopening wherever a previous dialog was
/// last dragged to.
pub fn show(
    ctx: &egui::Context,
    state: &mut RecordDialogState,
    center: egui::Pos2,
) -> DialogAction {
    let mut action = DialogAction::None;
    let mut open = true;

    egui::Window::new(&state.program.title)
        .id(egui::Id::new((
            "guide_record_dialog",
            &state.program.channel,
            state.program.start,
        )))
        .collapsible(false)
        .resizable(false)
        .default_pos(center)
        .pivot(egui::Align2::CENTER_CENTER)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(format!("{}  {}", state.channel.number, state.channel.name,));
            let start = crate::ui::live::format_time(state.program.start);
            let stop = crate::ui::live::format_time(state.program.stop);
            ui.label(format!("{start} – {stop}"));

            if let Some(desc) = &state.program.desc {
                ui.add_space(4.0);
                ui.label(desc);
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Start padding (min):");
                ui.add(egui::DragValue::new(&mut state.pad_start_min).range(0..=60));
                ui.label("End padding (min):");
                ui.add(egui::DragValue::new(&mut state.pad_end_min).range(0..=180));
            });

            let airing_ready = matches!(state.airing, Loaded::Ready(_));
            let airing_failed = matches!(state.airing, Loaded::Err(_));

            ui.add_space(8.0);
            if airing_failed {
                if let Loaded::Err(e) = &state.airing {
                    ui.colored_label(
                        egui::Color32::RED,
                        format!("Couldn't look up this airing: {e}"),
                    );
                }
            }

            ui.horizontal(|ui| {
                let record_clicked = ui
                    .add_enabled(
                        airing_ready && !state.busy,
                        egui::Button::new("Record this episode"),
                    )
                    .clicked();
                if record_clicked {
                    if let Loaded::Ready(airing) = &state.airing {
                        let pad_start = state.pad_start_min * 60;
                        let pad_end = state.pad_end_min * 60;
                        let channels = vec![state.program.channel.clone()];
                        action = DialogAction::Record {
                            name: state.program.title.clone(),
                            time: state.program.start - pad_start,
                            duration: (state.program.stop - state.program.start)
                                + pad_start
                                + pad_end,
                            channels,
                            airing: airing.clone(),
                        };
                    }
                }

                if let Some(series_id) = &state.program.series_id {
                    if state.existing_pass {
                        ui.label("A pass already exists for this series.");
                    } else {
                        ui.checkbox(&mut state.new_only, "New episodes only");
                        if ui
                            .add_enabled(!state.busy, egui::Button::new("Create Pass"))
                            .clicked()
                        {
                            action = DialogAction::CreatePass {
                                name: state.program.title.clone(),
                                image: state.program.image.clone(),
                                series_id: series_id.clone(),
                                new_only: state.new_only,
                                pad_start: state.pad_start_min * 60,
                                pad_end: state.pad_end_min * 60,
                            };
                        }
                    }
                }
            });

            if state.busy {
                ui.add_space(6.0);
                ui.spinner();
            }
            if let Some(Ok(msg)) = &state.message {
                ui.add_space(6.0);
                ui.colored_label(egui::Color32::GREEN, msg);
            }
            if let Some(Err(msg)) = &state.message {
                ui.add_space(6.0);
                ui.colored_label(egui::Color32::RED, msg);
            }
        });

    if !open {
        action = DialogAction::Close;
    }
    action
}
