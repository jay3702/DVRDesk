//! Two ways to bring settings over from the old (Tauri/React) DVRDesk app:
//!
//! - **Clipboard paste** (`import`) — the old app's "Copy Settings for
//!   Migration" button (`Settings.tsx::copyMigrationSettings`) writes a
//!   small, explicitly-versioned JSON payload to the clipboard; the user
//!   pastes it here. Works on every platform, needs the old app to be
//!   opened and clicked once.
//! - **Direct read** (`detect_legacy_install`/`import_from_legacy_install`)
//!   — on Linux, the old app's settings live in a plain, standard SQLite
//!   database (WebKitGTK's local-storage backend — confirmed by opening a
//!   real one directly, not assumed), so native can read it straight off
//!   disk with no clipboard step and no need to ever open the old app at
//!   all. **Linux only for now** — WebView2 (Windows) stores local storage
//!   in a LevelDB store instead, a materially harder format to parse
//!   safely and one that can't be verified from a Linux dev machine; that
//!   path is a deliberate future follow-up, not attempted here.
//!
//! Both paths funnel into the same `apply()` merge logic. The old app's
//! `storageSharePath`/`preferRemux` are deliberately never part of either
//! path — both are obsolete concepts this app never carried over (see
//! `ui/settings.rs`'s own doc comment for why).

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Deserialize;

use crate::state::settings::{AppSettings, KeybindingsConfig, ServerOption, SkipIntervalsConfig};

const SUPPORTED_VERSION: u32 = 1;

/// The old app's own Tauri bundle identifier (from its `tauri.conf.json`) —
/// also the name of its WebView data directory on disk.
const OLD_APP_IDENTIFIER: &str = "com.jay.winchannels";

#[derive(Debug, Deserialize)]
struct MigrationServer {
    id: String,
    name: String,
    url: String,
    #[serde(default)]
    tailscale_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MigrationPayload {
    dvrdesk_migration_version: u32,
    #[serde(default)]
    servers: Vec<MigrationServer>,
    #[serde(default)]
    active_server_id: Option<String>,
    #[serde(default)]
    diagnostics_enabled: bool,
    #[serde(default)]
    show_hidden_live_channels: bool,
    #[serde(default)]
    keybindings: KeybindingsConfig,
    #[serde(default)]
    skip_intervals: SkipIntervalsConfig,
    #[serde(default)]
    theme: String,
    #[serde(default)]
    window_always_on_top: bool,
}

/// Parses and applies a pasted migration payload onto `settings` in place.
/// Returns a short human-readable summary on success.
pub fn import(settings: &mut AppSettings, text: &str) -> Result<String, String> {
    let payload: MigrationPayload = serde_json::from_str(text.trim())
        .map_err(|e| format!("Couldn't parse that as a DVRDesk migration payload: {e}"))?;

    if payload.dvrdesk_migration_version != SUPPORTED_VERSION {
        return Err(format!(
            "Unsupported migration payload version {} (expected {SUPPORTED_VERSION}) — this may be from a newer or older DVRDesk release.",
            payload.dvrdesk_migration_version
        ));
    }

    Ok(apply(
        settings,
        payload.servers,
        payload.active_server_id,
        payload.diagnostics_enabled,
        payload.show_hidden_live_channels,
        payload.keybindings,
        payload.skip_intervals,
        &payload.theme,
        payload.window_always_on_top,
    ))
}

/// Merges the given data onto `settings` in place. Servers are merged — an
/// existing id is never touched or removed, only ids not already present
/// are appended. Every other field is a scalar preference and is
/// overwritten unconditionally, since this only ever runs as an explicit,
/// user-clicked action (either pasting, or clicking "Import Automatically").
fn apply(
    settings: &mut AppSettings,
    servers: Vec<MigrationServer>,
    active_server_id: Option<String>,
    diagnostics_enabled: bool,
    show_hidden_live_channels: bool,
    keybindings: KeybindingsConfig,
    skip_intervals: SkipIntervalsConfig,
    theme: &str,
    window_always_on_top: bool,
) -> String {
    let existing_ids: HashSet<String> = settings.servers.iter().map(|s| s.id.clone()).collect();
    let mut added = 0usize;
    for s in servers {
        if existing_ids.contains(&s.id) {
            continue;
        }
        settings.servers.push(ServerOption {
            id: s.id,
            name: s.name,
            url: s.url,
            tailscale_url: s.tailscale_url,
        });
        added += 1;
    }

    if let Some(active_id) = active_server_id {
        if settings.servers.iter().any(|s| s.id == active_id) {
            settings.active_server_id = Some(active_id);
        }
    }

    settings.diagnostics_enabled = diagnostics_enabled;
    settings.show_hidden_live_channels = show_hidden_live_channels;
    settings.keybindings = keybindings;
    settings.skip_intervals = skip_intervals;
    settings.window_always_on_top = window_always_on_top;
    settings.theme = match theme.to_lowercase().as_str() {
        "dark" => egui::ThemePreference::Dark,
        "light" => egui::ThemePreference::Light,
        _ => egui::ThemePreference::System,
    };

    format!("Imported settings from DVRDesk (legacy) — {added} new server(s) added.")
}

// ---- Direct read from the old app's real WebKitGTK local storage (Linux) ----

#[derive(Debug, Deserialize)]
struct RawServer {
    id: String,
    name: String,
    url: String,
    #[serde(default, rename = "tailscaleUrl")]
    tailscale_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawKeybindings {
    skip_forward: Vec<String>,
    skip_back: Vec<String>,
    fast_forward: Vec<String>,
    fast_reverse: Vec<String>,
    play_pause: Vec<String>,
    close: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSkipIntervals {
    skip_forward: u32,
    skip_back: u32,
    fast_forward: u32,
    fast_reverse: u32,
}

/// Returns the path to the old app's real local-storage database if it
/// exists on this machine — only the `tauri://localhost` origin file, which
/// is what a real installed build actually writes to (a second
/// `http_localhost_*` file only ever exists when running the Vite dev
/// server, never on an end-user install).
pub fn detect_legacy_install() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let path = base
        .data_dir()
        .join(OLD_APP_IDENTIFIER)
        .join("localstorage")
        .join("tauri_localhost_0.localstorage");
    path.exists().then_some(path)
}

/// WebKitGTK's local-storage `ItemTable.value` column stores each string as
/// raw UTF-16LE bytes (confirmed by reading a real one directly), regardless
/// of the database's own declared text encoding — so this decodes it by
/// hand rather than treating it as ordinary SQLite TEXT.
fn read_raw_value(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    let bytes: Vec<u8> = conn
        .query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .ok()?;
    let utf16: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&utf16).ok()
}

fn read_bool(conn: &rusqlite::Connection, key: &str) -> bool {
    read_raw_value(conn, key).as_deref() == Some("true")
}

/// Reads the old app's settings straight off disk and applies them — no
/// clipboard, no need to ever open the old app. Coupled to the old app's
/// *current* localStorage key names (`useStore.ts`'s `*_KEY` constants):
/// unlike the versioned clipboard payload, there's no version envelope
/// here, so if those key names or shapes ever change, this silently finds
/// nothing rather than importing something wrong — an accepted tradeoff
/// given both apps are maintained together.
pub fn import_from_legacy_install(settings: &mut AppSettings) -> Result<String, String> {
    let path = detect_legacy_install().ok_or_else(|| {
        "No existing DVRDesk (legacy) installation was found on this machine.".to_string()
    })?;

    let conn = rusqlite::Connection::open(&path)
        .map_err(|e| format!("Couldn't open the old app's settings database: {e}"))?;

    let servers: Vec<MigrationServer> = match read_raw_value(&conn, "dvr_servers") {
        Some(raw) => serde_json::from_str::<Vec<RawServer>>(&raw)
            .map_err(|e| format!("Couldn't parse the old app's server list: {e}"))?
            .into_iter()
            .map(|s| MigrationServer {
                id: s.id,
                name: s.name,
                url: s.url,
                tailscale_url: s.tailscale_url,
            })
            .collect(),
        None => Vec::new(),
    };

    let active_server_id = read_raw_value(&conn, "dvr_active_server_id");

    let keybindings = match read_raw_value(&conn, "player_keybindings") {
        Some(raw) => {
            let r: RawKeybindings = serde_json::from_str(&raw)
                .map_err(|e| format!("Couldn't parse the old app's keybindings: {e}"))?;
            KeybindingsConfig {
                skip_forward: r.skip_forward,
                skip_back: r.skip_back,
                fast_forward: r.fast_forward,
                fast_reverse: r.fast_reverse,
                play_pause: r.play_pause,
                close: r.close,
            }
        }
        None => KeybindingsConfig::default(),
    };

    let skip_intervals = match read_raw_value(&conn, "player_skip_intervals") {
        Some(raw) => {
            let r: RawSkipIntervals = serde_json::from_str(&raw)
                .map_err(|e| format!("Couldn't parse the old app's skip intervals: {e}"))?;
            SkipIntervalsConfig {
                skip_forward: r.skip_forward,
                skip_back: r.skip_back,
                fast_forward: r.fast_forward,
                fast_reverse: r.fast_reverse,
            }
        }
        None => SkipIntervalsConfig::default(),
    };

    let theme = read_raw_value(&conn, "app_theme").unwrap_or_default();

    Ok(apply(
        settings,
        servers,
        active_server_id,
        read_bool(&conn, "diagnostics_enabled"),
        read_bool(&conn, "live_show_hidden_channels"),
        keybindings,
        skip_intervals,
        &theme,
        read_bool(&conn, "window_always_on_top"),
    ))
}
