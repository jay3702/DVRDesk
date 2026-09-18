//! Persisted app settings — the Rust equivalent of `useStore.ts`'s
//! per-localStorage-key fields, collapsed into one JSON file on disk.
//! `#[serde(default)]` on every field means adding a new setting later never
//! breaks loading an older settings file (mirrors `DEFAULT_KEYBINDINGS`/
//! `DEFAULT_SKIP_INTERVALS` merge-in behavior from the old store).

use std::io::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerOption {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub tailscale_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeployKind {
    Local,
    Remote,
}

/// A configured place to run a `guide-history-service` instance, managed
/// from Settings' "Deploy & Manage Instances" section — see `deploy.rs` for
/// the actual create/test/deploy/test logic this only stores the config
/// for. Mirrors `ServerOption`'s shape (a `Vec<T>` of drafts, one row per
/// configured target).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeployTarget {
    pub id: String,
    pub name: String,
    pub kind: DeployKind,
    /// Only meaningful when `kind == Remote`.
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    /// Empty means fall back to `ssh-agent`/the user's default identity —
    /// never a password; see `deploy.rs`'s doc comment for why.
    pub ssh_key_path: String,
    /// Only meaningful when `kind == Remote` — ignored for `Local`, which
    /// always installs under this app's own data directory instead.
    pub remote_install_dir: String,
    /// -> `GHS_SERVER_URL` on the deployed instance — the Channels DVR
    /// server *that instance* should poll (usually the same one this app
    /// itself points at, but kept independent since a remote box might not
    /// reach the DVR via the same URL this app does).
    pub dvr_server_url: String,
    /// -> `GHS_POLL_SECS`.
    pub poll_secs: u64,
    /// -> `GHS_RETENTION_SECS`.
    pub retention_secs: i64,
    /// -> `GHS_FETCH_WINDOW_SECS`.
    pub fetch_window_secs: u32,
    /// -> `GHS_LISTEN_ADDR`'s port half; always binds `0.0.0.0` so other
    /// machines on the network can read it too, matching the service's own
    /// stated design intent.
    pub listen_port: u16,
}

impl DeployTarget {
    pub fn new_local(id: String, dvr_server_url: String) -> Self {
        Self {
            id,
            name: "This machine".to_string(),
            kind: DeployKind::Local,
            ssh_host: String::new(),
            ssh_port: 22,
            ssh_user: String::new(),
            ssh_key_path: String::new(),
            remote_install_dir: "~/.dvrdesk-guide-history-service".to_string(),
            dvr_server_url,
            poll_secs: 900,
            retention_secs: 172_800,
            fetch_window_secs: 7_200,
            listen_port: 8790,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeybindingsConfig {
    pub skip_forward: Vec<String>,
    pub skip_back: Vec<String>,
    pub fast_forward: Vec<String>,
    pub fast_reverse: Vec<String>,
    pub play_pause: Vec<String>,
    pub close: Vec<String>,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            skip_forward: vec!["ArrowRight".into(), "l".into()],
            skip_back: vec!["ArrowLeft".into(), "j".into()],
            fast_forward: vec!["Shift+ArrowRight".into()],
            fast_reverse: vec!["Shift+ArrowLeft".into()],
            play_pause: vec![" ".into(), "k".into()],
            close: vec!["Escape".into()],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkipIntervalsConfig {
    pub skip_forward: u32,
    pub skip_back: u32,
    pub fast_forward: u32,
    pub fast_reverse: u32,
}

impl Default for SkipIntervalsConfig {
    fn default() -> Self {
        Self {
            skip_forward: 30,
            skip_back: 10,
            fast_forward: 60,
            fast_reverse: 60,
        }
    }
}

fn default_theme() -> egui::ThemePreference {
    egui::ThemePreference::System
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSettings {
    #[serde(default)]
    pub servers: Vec<ServerOption>,
    #[serde(default)]
    pub active_server_id: Option<String>,
    #[serde(default)]
    pub diagnostics_enabled: bool,
    #[serde(default)]
    pub show_hidden_live_channels: bool,
    #[serde(default)]
    pub keybindings: KeybindingsConfig,
    #[serde(default)]
    pub skip_intervals: SkipIntervalsConfig,
    #[serde(default = "default_theme")]
    pub theme: egui::ThemePreference,
    #[serde(default)]
    pub window_always_on_top: bool,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// URL of an optionally separately-run `guide-history-service`
    /// instance — entirely optional; when unset (the default), the guide
    /// grid behaves exactly as it does without this feature at all.
    #[serde(default)]
    pub history_service_url: Option<String>,
    /// Configured places to create/deploy/manage a `guide-history-service`
    /// instance — see `deploy.rs`. Entirely optional; an empty list just
    /// means nothing has been set up here yet, same as before this feature
    /// existed.
    #[serde(default)]
    pub guide_deploy_targets: Vec<DeployTarget>,

    /// Where offline downloads are kept. Empty means beside the rest of
    /// this app's own data (see `download_path`).
    #[serde(default)]
    pub download_dir: String,
    /// Where the live buffer is written. Empty means the same.
    #[serde(default)]
    pub buffer_dir: String,
    /// Where this app's own cache and log file go. Empty means the
    /// default beside this app's own data.
    #[serde(default)]
    pub cache_dir: String,
    /// How much disk the live buffer may use, in gigabytes. Zero turns it
    /// off entirely — live channels play exactly as they do without this
    /// feature, with no pause/rewind.
    #[serde(default)]
    pub live_buffer_gb: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            active_server_id: None,
            diagnostics_enabled: false,
            show_hidden_live_channels: false,
            keybindings: KeybindingsConfig::default(),
            skip_intervals: SkipIntervalsConfig::default(),
            theme: default_theme(),
            window_always_on_top: false,
            sidebar_collapsed: false,
            history_service_url: None,
            guide_deploy_targets: Vec::new(),
            download_dir: String::new(),
            buffer_dir: String::new(),
            cache_dir: String::new(),
            live_buffer_gb: 0,
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    crate::paths::config_dir().map(|dir| dir.join("settings.json"))
}

/// A configured directory, or the default one beside this app's own data.
/// The configured path is used exactly as given, not as a parent to append
/// a leaf onto — someone who points this at a directory already named
/// `DVRDeskDownloads` means that directory, not a `Downloads` folder inside
/// it.
fn resolve(configured: &str, default_leaf: &str) -> PathBuf {
    let configured = configured.trim();
    if !configured.is_empty() {
        return PathBuf::from(configured);
    }
    crate::paths::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(default_leaf)
}

/// Whether a directory can actually be written to, checked by writing to
/// it rather than by inspecting permission bits — a path can look readable
/// and still refuse a write for reasons no attribute reports (an ACL, a
/// read-only mount, a sandbox), so creating a file and removing it again is
/// the only check that means anything.
pub fn writable(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".dvrdesk-write-test");
    std::fs::write(&probe, b"")?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

impl AppSettings {
    /// Loads settings from disk, falling back to defaults if the file is
    /// missing, unreadable, or fails to parse (e.g. a genuinely corrupt
    /// file) — a fresh/broken settings file should never prevent the app
    /// from starting.
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
                crate::logline!(
                    "settings: failed to parse {}: {e} — using defaults",
                    path.display()
                );
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Atomic write-temp-then-rename so a crash or power loss mid-write
    /// can't leave a half-written, unparseable settings file behind.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = settings_path() else {
            return Err(std::io::Error::other("no config directory available"));
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json =
            serde_json::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;

        let tmp_path = path.with_extension("json.tmp");
        {
            let mut tmp = std::fs::File::create(&tmp_path)?;
            tmp.write_all(json.as_bytes())?;
            tmp.sync_all()?;
        }
        std::fs::rename(&tmp_path, &path)
    }

    /// Where downloads go, configured or default.
    pub fn download_path(&self) -> PathBuf {
        resolve(&self.download_dir, "Downloads")
    }

    /// Where the live buffer goes, configured or default.
    pub fn buffer_path(&self) -> PathBuf {
        resolve(&self.buffer_dir, "Timeshift")
    }

    /// Where this app's own cache and log file go, configured or default.
    pub fn cache_path(&self) -> PathBuf {
        let configured = self.cache_dir.trim();
        if !configured.is_empty() {
            return PathBuf::from(configured);
        }
        crate::paths::cache_dir().unwrap_or_else(std::env::temp_dir)
    }
}
