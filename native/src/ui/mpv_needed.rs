//! The "mpv is needed" dialog, shown at startup and on Play when libmpv
//! couldn't be loaded. Before this, a missing libmpv made Play silently do
//! nothing. On Windows it offers to download libmpv (`mpv_install`); on
//! Linux it names the package to install.

#[cfg(windows)]
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MpvNeededState {
    pub open: bool,
    #[cfg(windows)]
    pub install: Option<Arc<Mutex<crate::mpv_install::InstallState>>>,
}

#[derive(Default)]
pub struct MpvNeededAction {
    /// Caller spawns `mpv_install::install` with this state.
    #[cfg(windows)]
    pub start_download: bool,
    pub restart: bool,
}

pub fn show(ctx: &egui::Context, state: &mut MpvNeededState, load_error: &str) -> MpvNeededAction {
    let mut action = MpvNeededAction::default();
    if !state.open {
        return action;
    }
    let mut open = true;
    egui::Window::new("mpv is needed to play video")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .default_width(420.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            body(ui, state, &mut action);
            ui.add_space(6.0);
            egui::CollapsingHeader::new("Details").show(ui, |ui| {
                ui.label(egui::RichText::new(load_error).small().weak());
            });
        });
    if !open {
        state.open = false;
    }
    action
}

#[cfg(windows)]
fn body(ui: &mut egui::Ui, state: &mut MpvNeededState, action: &mut MpvNeededAction) {
    use crate::mpv_install::InstallState;

    ui.label(
        "DVRDesk plays video with mpv, which isn't installed on this PC. DVRDesk can \
         download it for you (about 30 MB) from the mpv builds published at \
         github.com/shinchiro/mpv-winbuild-cmake.",
    );
    ui.add_space(8.0);
    let current = state.install.as_ref().map(|s| s.lock().unwrap().clone());
    match current {
        None | Some(InstallState::Failed(_)) => {
            if let Some(InstallState::Failed(e)) = &current {
                ui.colored_label(egui::Color32::RED, e);
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                let label = if current.is_some() { "Try Again" } else { "Download mpv" };
                if ui.button(label).clicked() {
                    action.start_download = true;
                }
                if ui.button("Not Now").clicked() {
                    state.open = false;
                }
            });
        }
        Some(InstallState::Starting) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Finding the latest mpv…");
            });
        }
        Some(InstallState::Downloading { done, total }) => {
            let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(fraction).text(format!(
                "Downloading… {:.1} of {:.1} MB",
                done as f64 / 1_048_576.0,
                total as f64 / 1_048_576.0
            )));
        }
        Some(InstallState::Extracting) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Unpacking…");
            });
        }
        Some(InstallState::Done { vulkan_missing }) => {
            ui.colored_label(
                egui::Color32::GREEN,
                "mpv is installed. Restart DVRDesk to start using it.",
            );
            if vulkan_missing {
                ui.colored_label(
                    egui::Color32::from_rgb(224, 123, 0),
                    "This PC's graphics driver doesn't include Vulkan, which mpv needs. If video \
                     still doesn't play after restarting, update your graphics driver.",
                );
            }
            if ui.button("Restart DVRDesk").clicked() {
                action.restart = true;
            }
        }
    }
}

#[cfg(not(windows))]
fn body(ui: &mut egui::Ui, state: &mut MpvNeededState, _action: &mut MpvNeededAction) {
    ui.label("DVRDesk plays video with libmpv, which isn't installed. Install it with your package manager, then restart DVRDesk:");
    ui.add_space(4.0);
    for (distro, cmd) in [
        ("Debian / Ubuntu", "sudo apt install libmpv2"),
        ("Fedora", "sudo dnf install mpv-libs"),
        ("Arch", "sudo pacman -S mpv"),
    ] {
        ui.horizontal(|ui| {
            ui.label(format!("{distro}:"));
            ui.code(cmd);
        });
    }
    ui.add_space(6.0);
    if ui.button("Close").clicked() {
        state.open = false;
    }
}
