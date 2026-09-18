//! Real Settings screen (Phase 8), replacing the Phase 3 single-server
//! placeholder: multi-server management (add/remove/edit/probe/switch
//! active), appearance (theme, always-on-top), player keybindings + skip
//! intervals, Live TV, diagnostics.
//!
//! Deliberately not ported yet: bug report composer, persistent client
//! error log, GitHub compatibility-matrix check, GitHub release/update
//! checker. Each needs its own bit of new infrastructure the plan already
//! scoped separately (`api/compat.rs`, `api/github.rs`, `error_log.rs`,
//! `bug_report.rs`) that hasn't been built yet — left for a dedicated pass
//! rather than folded in here.
//!
//! Also dropped, confirmed obsolete rather than just deferred: the old
//! app's Storage Share Path / SRT-sidecar subtitle setting (captions now
//! come from the recording's own embedded track via `stream.mpg`, see
//! `player/captions.rs`) and "Prefer remux stream" (that toggle only ever
//! applied to the HLS remux/transcode fork, which recordings no longer use
//! at all — see the plan's §5 findings).

use std::collections::{HashMap, HashSet};

use crate::deploy::{DeployStatus, Deploys};
use crate::state::settings::{
    AppSettings, DeployKind, DeployTarget, KeybindingsConfig, ServerOption, SkipIntervalsConfig,
};

pub struct SettingsState {
    draft_servers: Vec<ServerOption>,
    server_error: Option<String>,
    servers_saved: bool,
    /// Keyed by server id. Present with `true`/`false` once a probe
    /// completes; absent before the first probe.
    probe_results: HashMap<String, bool>,
    probe_in_flight: HashSet<String>,

    kb_text: KbText,
    skip_draft: SkipIntervalsConfig,
    player_saved: bool,

    history_url_draft: String,
    history_probe_in_flight: bool,
    history_probe_result: Option<Result<(), String>>,

    download_dir_draft: String,
    buffer_dir_draft: String,
    cache_dir_draft: String,
    live_buffer_gb_draft: u32,
    storage_error: Option<String>,
    storage_saved: bool,

    draft_deploy_targets: Vec<DeployTarget>,
    deploy_error: Option<String>,
    deploy_targets_saved: bool,
}

struct KbText {
    skip_forward: String,
    skip_back: String,
    fast_forward: String,
    fast_reverse: String,
    play_pause: String,
    close: String,
}

impl KbText {
    fn from_config(kb: &KeybindingsConfig) -> Self {
        Self {
            skip_forward: kb.skip_forward.join(", "),
            skip_back: kb.skip_back.join(", "),
            fast_forward: kb.fast_forward.join(", "),
            fast_reverse: kb.fast_reverse.join(", "),
            play_pause: kb.play_pause.join(", "),
            close: kb.close.join(", "),
        }
    }

    fn to_config(&self) -> KeybindingsConfig {
        let split = |s: &str| -> Vec<String> {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };
        KeybindingsConfig {
            skip_forward: split(&self.skip_forward),
            skip_back: split(&self.skip_back),
            fast_forward: split(&self.fast_forward),
            fast_reverse: split(&self.fast_reverse),
            play_pause: split(&self.play_pause),
            close: split(&self.close),
        }
    }
}

impl SettingsState {
    pub fn new(settings: &AppSettings) -> Self {
        Self {
            draft_servers: settings.servers.clone(),
            server_error: None,
            servers_saved: false,
            probe_results: HashMap::new(),
            probe_in_flight: HashSet::new(),
            kb_text: KbText::from_config(&settings.keybindings),
            skip_draft: settings.skip_intervals,
            player_saved: false,

            history_url_draft: settings.history_service_url.clone().unwrap_or_default(),
            history_probe_in_flight: false,
            history_probe_result: None,

            download_dir_draft: settings.download_dir.clone(),
            buffer_dir_draft: settings.buffer_dir.clone(),
            cache_dir_draft: settings.cache_dir.clone(),
            live_buffer_gb_draft: settings.live_buffer_gb,
            storage_error: None,
            storage_saved: false,

            draft_deploy_targets: settings.guide_deploy_targets.clone(),
            deploy_error: None,
            deploy_targets_saved: false,
        }
    }

    /// Called once a probe spawned via `SettingsAction::probe_server`
    /// completes.
    pub fn probe_completed(&mut self, server_id: &str, reachable: bool) {
        self.probe_in_flight.remove(server_id);
        self.probe_results.insert(server_id.to_string(), reachable);
    }

    pub fn history_probe_completed(&mut self, result: Result<(), String>) {
        self.history_probe_in_flight = false;
        self.history_probe_result = Some(result);
    }
}

#[derive(Default)]
pub struct SettingsAction {
    /// `(server_id, url)` — caller spawns the async reachability probe;
    /// the result comes back to `SettingsState::probe_completed`.
    pub probe_server: Option<(String, String)>,
    /// Set whenever any field on `settings` was mutated this frame — every
    /// other change (servers, keybindings, theme, toggles) is applied
    /// in-place directly on `settings` inside this module, so the caller's
    /// only remaining job is to persist it to disk when this is true.
    pub settings_changed: bool,
    /// The one exception to the above — see the doc comment where it's
    /// set (the server-list radio button) for why.
    pub switch_server: Option<String>,
    /// URL to probe for the optional history service's "Test" button —
    /// same reasoning as `probe_server`, needs `App`'s async runtime.
    pub probe_history_service: Option<String>,

    /// `(id, target)` for each deploy-wizard action clicked this frame —
    /// `App` forwards these straight into the matching `Deploys` method,
    /// which owns the actual async work; this module only owns the drafts
    /// and renders whatever `Deploys::status(id)` currently reports.
    pub test_deploy_target: Option<(String, DeployTarget)>,
    pub run_deploy_target: Option<(String, DeployTarget)>,
    pub check_deploy_status: Option<(String, DeployTarget)>,
    pub remove_deploy_target: Option<(String, DeployTarget)>,
    /// Set when "Reset Channel Genre Assignments" is clicked — needs
    /// `App`'s own `channel_genres` field, which this module has no handle
    /// on.
    pub reset_channel_genres: bool,
}

fn normalize_server_url(raw: &str) -> String {
    let mut url = raw.trim().to_string();
    if url.is_empty() {
        return url;
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("http://{url}");
    }
    while url.len() > 1 && url.ends_with('/') {
        url.pop();
    }
    url
}

/// Matches the old app's validation: scheme + host + explicit `:port`.
fn is_valid_server_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    match rest.rsplit_once(':') {
        Some((host, port)) => {
            !host.is_empty() && !port.is_empty() && port.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

fn new_server_id() -> String {
    format!(
        "srv_{}",
        chrono::Local::now()
            .timestamp_nanos_opt()
            .unwrap_or_default()
    )
}

fn new_deploy_id() -> String {
    format!(
        "dep_{}",
        chrono::Local::now()
            .timestamp_nanos_opt()
            .unwrap_or_default()
    )
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    settings: &mut AppSettings,
    deploys: &Deploys,
) -> SettingsAction {
    let mut action = SettingsAction::default();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Settings");
        ui.separator();

        ui.heading("Appearance");
        let mut theme = settings.theme;
        theme.radio_buttons(ui);
        if theme != settings.theme {
            settings.theme = theme;
            action.settings_changed = true;
        }
        if ui
            .checkbox(&mut settings.window_always_on_top, "Keep window on top when not maximized")
            .changed()
        {
            action.settings_changed = true;
        }
        ui.add_space(8.0);
        ui.separator();

        ui.heading("Servers");
        ui.label("Add as many Channels DVR servers as you like, then pick the active one below.");
        egui::Grid::new("servers_grid")
            .num_columns(5)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.strong("Active");
                ui.strong("Name");
                ui.strong("URL");
                ui.strong("Tailscale URL (optional)");
                ui.strong("");
                ui.end_row();

                let mut remove_idx: Option<usize> = None;
                for (i, server) in state.draft_servers.iter_mut().enumerate() {
                    let is_active = settings.active_server_id.as_deref() == Some(server.id.as_str());
                    if ui.radio(is_active, "").clicked() {
                        // Not applied directly here (unlike every other
                        // field on this screen) — switching the active
                        // server needs to reset every screen's cached
                        // data and stop any active playback too, which
                        // only `App` has a handle on.
                        action.switch_server = Some(server.id.clone());
                    }
                    ui.text_edit_singleline(&mut server.name);
                    ui.text_edit_singleline(&mut server.url);
                    let mut tailscale = server.tailscale_url.clone().unwrap_or_default();
                    if ui.text_edit_singleline(&mut tailscale).changed() {
                        server.tailscale_url = if tailscale.trim().is_empty() { None } else { Some(tailscale) };
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Test").clicked() {
                            state.probe_in_flight.insert(server.id.clone());
                            action.probe_server = Some((server.id.clone(), normalize_server_url(&server.url)));
                        }
                        if ui.button("Remove").clicked() {
                            remove_idx = Some(i);
                        }
                    });
                    ui.end_row();

                    if state.probe_in_flight.contains(&server.id) {
                        ui.label("");
                        ui.label("");
                        ui.colored_label(egui::Color32::GRAY, "Probing…");
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    } else if let Some(&reachable) = state.probe_results.get(&server.id) {
                        ui.label("");
                        ui.label("");
                        if reachable {
                            ui.colored_label(egui::Color32::GREEN, "Reachable");
                        } else {
                            ui.colored_label(egui::Color32::RED, "Not reachable");
                        }
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    }
                }
                if let Some(i) = remove_idx {
                    let removed = state.draft_servers.remove(i);
                    if settings.active_server_id.as_deref() == Some(removed.id.as_str()) {
                        settings.active_server_id = state.draft_servers.first().map(|s| s.id.clone());
                        action.settings_changed = true;
                    }
                }
            });

        ui.horizontal(|ui| {
            if ui.button("Add Server").clicked() {
                state.draft_servers.push(ServerOption {
                    id: new_server_id(),
                    name: String::new(),
                    url: String::new(),
                    tailscale_url: None,
                });
                state.servers_saved = false;
            }
            if ui.button(if state.servers_saved { "✔ Saved" } else { "Save Servers" }).clicked() {
                state.server_error = None;
                let mut validated = Vec::new();
                let mut err = None;
                for s in &state.draft_servers {
                    let name = s.name.trim();
                    let url = normalize_server_url(&s.url);
                    if name.is_empty() && url.is_empty() {
                        continue;
                    }
                    if name.is_empty() || url.is_empty() {
                        err = Some("Each server row must include both Name and URL.".to_string());
                        break;
                    }
                    if !is_valid_server_url(&url) {
                        err = Some(format!("Invalid URL for \"{name}\". Include scheme and port, e.g. http://192.168.1.4:8089"));
                        break;
                    }
                    let tailscale_url = s.tailscale_url.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|t| {
                        let n = normalize_server_url(t);
                        n
                    });
                    if let Some(ts) = &tailscale_url {
                        if !is_valid_server_url(ts) {
                            err = Some(format!("Invalid Tailscale URL for \"{name}\"."));
                            break;
                        }
                    }
                    validated.push(ServerOption { id: s.id.clone(), name: name.to_string(), url, tailscale_url });
                }
                if err.is_none() && validated.is_empty() {
                    err = Some("Add at least one server with Name and URL.".to_string());
                }
                match err {
                    Some(e) => state.server_error = Some(e),
                    None => {
                        if settings.active_server_id.is_none()
                            || !validated.iter().any(|s| Some(&s.id) == settings.active_server_id.as_ref())
                        {
                            settings.active_server_id = validated.first().map(|s| s.id.clone());
                        }
                        state.draft_servers = validated.clone();
                        settings.servers = validated;
                        action.settings_changed = true;
                        state.servers_saved = true;
                    }
                }
            }
        });
        if let Some(err) = &state.server_error {
            ui.colored_label(egui::Color32::RED, err);
        }
        ui.add_space(8.0);
        ui.separator();

        ui.heading("Diagnostics");
        if ui
            .checkbox(&mut settings.diagnostics_enabled, "Enable diagnostics tools in player (Stats/Copy Report)")
            .changed()
        {
            action.settings_changed = true;
        }
        ui.add_space(8.0);
        ui.separator();

        ui.heading("Player Keybindings & Skip Intervals");
        ui.label("Key names like ArrowRight, l, Shift+ArrowLeft, Space, Escape. Separate multiple keys with commas.");
        egui::Grid::new("keybindings_grid")
            .num_columns(3)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.strong("Action");
                ui.strong("Key(s)");
                ui.strong("Skip (s)");
                ui.end_row();

                ui.label("Skip Forward");
                ui.text_edit_singleline(&mut state.kb_text.skip_forward);
                ui.add(egui::DragValue::new(&mut state.skip_draft.skip_forward).range(1..=600));
                ui.end_row();

                ui.label("Skip Back");
                ui.text_edit_singleline(&mut state.kb_text.skip_back);
                ui.add(egui::DragValue::new(&mut state.skip_draft.skip_back).range(1..=600));
                ui.end_row();

                ui.label("Fast Forward");
                ui.text_edit_singleline(&mut state.kb_text.fast_forward);
                ui.add(egui::DragValue::new(&mut state.skip_draft.fast_forward).range(1..=600));
                ui.end_row();

                ui.label("Fast Reverse");
                ui.text_edit_singleline(&mut state.kb_text.fast_reverse);
                ui.add(egui::DragValue::new(&mut state.skip_draft.fast_reverse).range(1..=600));
                ui.end_row();

                ui.label("Play / Pause");
                ui.text_edit_singleline(&mut state.kb_text.play_pause);
                ui.label("");
                ui.end_row();

                ui.label("Close Player");
                ui.text_edit_singleline(&mut state.kb_text.close);
                ui.label("");
                ui.end_row();
            });
        if ui.button(if state.player_saved { "✔ Saved" } else { "Save Player Settings" }).clicked() {
            settings.keybindings = state.kb_text.to_config();
            settings.skip_intervals = state.skip_draft;
            action.settings_changed = true;
            state.player_saved = true;
        }
        ui.add_space(8.0);
        ui.separator();

        ui.heading("Live TV");
        if ui
            .checkbox(&mut settings.show_hidden_live_channels, "Show hidden channels")
            .changed()
        {
            action.settings_changed = true;
        }
        ui.add_space(4.0);
        ui.label(
            "The guide grid's Movies/Sports/Drama/News/Kids filters assign each channel a genre \
             once, the first time something clearly matching airs, and never change it \
             automatically afterward — so a channel that got the wrong genre from an unusual \
             program the first time it was ever seen stays wrong until reset here.",
        );
        if ui.button("Reset Channel Genre Assignments").clicked() {
            action.reset_channel_genres = true;
        }
        ui.add_space(8.0);
        ui.separator();

        ui.heading("Storage");
        ui.label(
            "Where downloaded recordings and the live buffer are kept, and where this app's own \
             cache and log file go. Leave any of these blank for the default location beside this \
             app's own data. A changed path takes effect after restarting DVRDesk.",
        );
        egui::Grid::new("storage_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Downloads folder:");
            ui.text_edit_singleline(&mut state.download_dir_draft);
            ui.end_row();

            ui.label("Live buffer folder:");
            ui.text_edit_singleline(&mut state.buffer_dir_draft);
            ui.end_row();

            ui.label("Cache/log folder:");
            ui.text_edit_singleline(&mut state.cache_dir_draft);
            ui.end_row();

            ui.label("Live buffer size cap:");
            ui.add(egui::DragValue::new(&mut state.live_buffer_gb_draft).range(0..=500).suffix(" GB"));
            ui.end_row();
        });
        ui.label(
            "0 GB disables the live buffer entirely — live TV plays exactly as it does today, \
             with no pause/rewind.",
        );
        if ui.button(if state.storage_saved { "✔ Saved" } else { "Save Storage Settings" }).clicked() {
            state.storage_error = None;
            let mut candidate = settings.clone();
            candidate.download_dir = state.download_dir_draft.clone();
            candidate.buffer_dir = state.buffer_dir_draft.clone();
            candidate.cache_dir = state.cache_dir_draft.clone();
            candidate.live_buffer_gb = state.live_buffer_gb_draft;

            let checks = [
                ("Downloads folder", candidate.download_path()),
                ("Live buffer folder", candidate.buffer_path()),
                ("Cache/log folder", candidate.cache_path()),
            ];
            let mut err = None;
            for (label, path) in &checks {
                if let Err(e) = crate::state::settings::writable(path) {
                    err = Some(format!("Can't write to {label} ({}): {e}", path.display()));
                    break;
                }
            }
            match err {
                Some(e) => state.storage_error = Some(e),
                None => {
                    *settings = candidate;
                    action.settings_changed = true;
                    state.storage_saved = true;
                }
            }
        }
        if let Some(e) = &state.storage_error {
            ui.colored_label(egui::Color32::RED, e);
        }
        ui.add_space(8.0);
        ui.separator();

        ui.heading("Programming History (optional)");
        ui.label(
            "Points at a separately-run guide-history-service instance, which continuously \
             captures Channels DVR's guide data so the Live grid can show the previous day's \
             programming and let you jump to what was recorded — the DVR's own guide API never \
             keeps anything once it airs. Leave this blank if you haven't set one up; the guide \
             works exactly as it does today either way.",
        );
        ui.horizontal(|ui| {
            ui.label("Service URL:");
            ui.text_edit_singleline(&mut state.history_url_draft);
            if ui.button("Test").clicked() {
                state.history_probe_in_flight = true;
                state.history_probe_result = None;
                action.probe_history_service = Some(normalize_server_url(&state.history_url_draft));
            }
            if ui
                .button(if settings.history_service_url.as_deref() == Some(state.history_url_draft.trim()) {
                    "✔ Saved"
                } else {
                    "Save"
                })
                .clicked()
            {
                let trimmed = state.history_url_draft.trim();
                settings.history_service_url =
                    if trimmed.is_empty() { None } else { Some(normalize_server_url(trimmed)) };
                action.settings_changed = true;
            }
        });
        if state.history_probe_in_flight {
            ui.colored_label(egui::Color32::GRAY, "Probing…");
        } else if let Some(result) = &state.history_probe_result {
            match result {
                Ok(()) => {
                    ui.colored_label(egui::Color32::GREEN, "Reachable");
                }
                Err(e) => {
                    ui.colored_label(egui::Color32::RED, format!("Not reachable: {e}"));
                }
            }
        }
        ui.add_space(8.0);
        ui.separator();

        ui.heading("Deploy & Manage Instances (optional)");
        ui.label(
            "Create → Test → Deploy → Test a guide-history-service instance, either on this \
             machine or a remote Linux host over SSH (key-based auth only — no password field; \
             relies on ssh-agent or a key file you point at below). Building and transferring \
             requires a full checkout of this repo (the guide-history-service source next to \
             this app's own) — this is a developer/self-hosting workflow, not a packaged \
             installer.",
        );
        let mut remove_idx: Option<usize> = None;
        for (i, target) in state.draft_deploy_targets.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut target.name);
                    ui.selectable_value(&mut target.kind, DeployKind::Local, "Local (this machine)");
                    ui.selectable_value(&mut target.kind, DeployKind::Remote, "Remote (SSH)");
                    if ui.button("Remove Row").clicked() {
                        remove_idx = Some(i);
                    }
                });
                if target.kind == DeployKind::Remote {
                    egui::Grid::new(("deploy_ssh_grid", i))
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("SSH host:");
                            ui.text_edit_singleline(&mut target.ssh_host);
                            ui.end_row();

                            ui.label("SSH port:");
                            ui.add(egui::DragValue::new(&mut target.ssh_port).range(1..=65535));
                            ui.end_row();

                            ui.label("SSH user:");
                            ui.text_edit_singleline(&mut target.ssh_user);
                            ui.end_row();

                            ui.label("SSH key (blank = agent/default identity):");
                            ui.text_edit_singleline(&mut target.ssh_key_path);
                            ui.end_row();

                            ui.label("Remote install dir:");
                            ui.text_edit_singleline(&mut target.remote_install_dir);
                            ui.end_row();
                        });
                }
                egui::CollapsingHeader::new("Advanced (service configuration)")
                    .id_salt(("deploy_advanced", i))
                    .show(ui, |ui| {
                        egui::Grid::new(("deploy_advanced_grid", i))
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("DVR server URL for this instance:");
                                ui.text_edit_singleline(&mut target.dvr_server_url);
                                ui.end_row();

                                ui.label("Poll interval (s):");
                                ui.add(egui::DragValue::new(&mut target.poll_secs).range(60..=86_400));
                                ui.end_row();

                                ui.label("Retention (s):");
                                ui.add(
                                    egui::DragValue::new(&mut target.retention_secs).range(3_600..=2_592_000),
                                );
                                ui.end_row();

                                ui.label("Fetch window (s):");
                                ui.add(
                                    egui::DragValue::new(&mut target.fetch_window_secs).range(600..=86_400),
                                );
                                ui.end_row();

                                ui.label("Listen port:");
                                ui.add(egui::DragValue::new(&mut target.listen_port).range(1..=65535));
                                ui.end_row();
                            });
                    });

                ui.horizontal(|ui| {
                    if ui.button("Test Connection").clicked() {
                        action.test_deploy_target = Some((target.id.clone(), target.clone()));
                    }
                    if ui.button("Deploy").clicked() {
                        action.run_deploy_target = Some((target.id.clone(), target.clone()));
                    }
                    if ui.button("Check Status").clicked() {
                        action.check_deploy_status = Some((target.id.clone(), target.clone()));
                    }
                    if ui.button("Remove (uninstall)").clicked() {
                        action.remove_deploy_target = Some((target.id.clone(), target.clone()));
                    }
                });

                match deploys.status(&target.id) {
                    None => {}
                    Some(DeployStatus::TestingConnection) => {
                        ui.colored_label(egui::Color32::GRAY, "Testing connection…");
                    }
                    Some(DeployStatus::ConnectionOk { remote_arch }) => {
                        ui.colored_label(
                            egui::Color32::GREEN,
                            format!("Reachable ({remote_arch}) — ready to deploy."),
                        );
                    }
                    Some(DeployStatus::ConnectionFailed(e)) => {
                        ui.colored_label(egui::Color32::RED, format!("Connection failed: {e}"));
                    }
                    Some(DeployStatus::Building) => {
                        ui.colored_label(egui::Color32::GRAY, "Building release binary…");
                    }
                    Some(DeployStatus::Transferring) => {
                        ui.colored_label(egui::Color32::GRAY, "Transferring binary…");
                    }
                    Some(DeployStatus::Installing) => {
                        ui.colored_label(egui::Color32::GRAY, "Installing service…");
                    }
                    Some(DeployStatus::HealthChecking) => {
                        ui.colored_label(egui::Color32::GRAY, "Checking service health…");
                    }
                    Some(DeployStatus::Running { programs_cached, warning }) => {
                        ui.colored_label(
                            egui::Color32::GREEN,
                            format!("Running — {programs_cached} programs cached."),
                        );
                        if let Some(w) = &warning {
                            ui.colored_label(egui::Color32::from_rgb(224, 123, 0), w);
                        }
                        let url = match target.kind {
                            DeployKind::Local => format!("http://127.0.0.1:{}", target.listen_port),
                            DeployKind::Remote => {
                                format!("http://{}:{}", target.ssh_host, target.listen_port)
                            }
                        };
                        if ui.button("Use as Programming History source").clicked() {
                            state.history_url_draft = url.clone();
                            settings.history_service_url = Some(url);
                            action.settings_changed = true;
                        }
                    }
                    Some(DeployStatus::Failed(e)) => {
                        ui.colored_label(egui::Color32::RED, format!("Failed: {e}"));
                    }
                    Some(DeployStatus::Removing) => {
                        ui.colored_label(egui::Color32::GRAY, "Removing…");
                    }
                    Some(DeployStatus::Removed) => {
                        ui.colored_label(egui::Color32::GRAY, "Removed.");
                    }
                }
            });
        }
        if let Some(i) = remove_idx {
            state.draft_deploy_targets.remove(i);
            state.deploy_targets_saved = false;
        }

        ui.horizontal(|ui| {
            if ui.button("Add Target").clicked() {
                let default_url = settings
                    .servers
                    .iter()
                    .find(|s| Some(&s.id) == settings.active_server_id.as_ref())
                    .map(|s| s.url.clone())
                    .unwrap_or_default();
                state
                    .draft_deploy_targets
                    .push(DeployTarget::new_local(new_deploy_id(), default_url));
                state.deploy_targets_saved = false;
            }
            if ui
                .button(if state.deploy_targets_saved { "✔ Saved" } else { "Save Targets" })
                .clicked()
            {
                state.deploy_error = None;
                let mut err = None;
                for t in &state.draft_deploy_targets {
                    if t.name.trim().is_empty() {
                        err = Some("Every target needs a Name.".to_string());
                        break;
                    }
                    if t.kind == DeployKind::Remote
                        && (t.ssh_host.trim().is_empty() || t.ssh_user.trim().is_empty())
                    {
                        err = Some(format!(
                            "\"{}\" is Remote but is missing SSH host and/or user.",
                            t.name
                        ));
                        break;
                    }
                }
                match err {
                    Some(e) => state.deploy_error = Some(e),
                    None => {
                        settings.guide_deploy_targets = state.draft_deploy_targets.clone();
                        action.settings_changed = true;
                        state.deploy_targets_saved = true;
                    }
                }
            }
        });
        if let Some(e) = &state.deploy_error {
            ui.colored_label(egui::Color32::RED, e);
        }
        ui.add_space(8.0);
        ui.separator();

        ui.heading("About");
        ui.label(format!("DVRDesk (native) v{}", env!("CARGO_PKG_VERSION")));
    });

    action
}
