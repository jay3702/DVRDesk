//! Two ways to bring settings over from the old (Tauri/React) DVRDesk app:
//!
//! - **Clipboard paste** (`import`) — the old app's "Copy Settings for
//!   Migration" button (`Settings.tsx::copyMigrationSettings`) writes a
//!   small, explicitly-versioned JSON payload to the clipboard; the user
//!   pastes it here. Works on every platform, needs the old app to be
//!   opened and clicked once.
//! - **Direct read** (`detect_legacy_install`/`import_from_legacy_install`)
//!   — reads the old app's localStorage straight off disk with no clipboard
//!   step and no need to ever open the old app at all. On Linux that's a
//!   plain SQLite database (WebKitGTK's local-storage backend); on Windows
//!   it's WebView2's Chromium LevelDB store, read from a temp copy via
//!   `rusty-leveldb`. Both were confirmed against real installs, not
//!   assumed. macOS (WKWebView) is not supported by the direct read yet.
//!
//! Both paths funnel into the same `apply()` merge logic. The old app's
//! `storageSharePath`/`preferRemux` are deliberately never part of either
//! path — both are obsolete concepts this app never carried over (see
//! `ui/settings.rs`'s own doc comment for why).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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

/// Returns the path to the old app's real local-storage store if it exists
/// on this machine.
///
/// - Linux: WebKitGTK's SQLite file — only the `tauri://localhost` origin
///   file, which is what a real installed build actually writes to (a second
///   `http_localhost_*` file only ever exists when running the Vite dev
///   server, never on an end-user install).
/// - Windows: WebView2's Chromium LevelDB directory, shared by every origin
///   (the origin is filtered per-key in `read_leveldb_store`).
///
/// The directory name is the old app's identifier, `com.jay.winchannels`.
/// A near-empty `com.jay.dvrdesk` WebView2 directory may also exist on
/// Windows machines. It was confirmed to hold no settings and is ignored.
#[cfg(not(windows))]
pub fn detect_legacy_install() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let path = base
        .data_dir()
        .join(OLD_APP_IDENTIFIER)
        .join("localstorage")
        .join("tauri_localhost_0.localstorage");
    path.exists().then_some(path)
}

#[cfg(windows)]
pub fn detect_legacy_install() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let path = base
        .data_local_dir()
        .join(OLD_APP_IDENTIFIER)
        .join("EBWebView")
        .join("Default")
        .join("Local Storage")
        .join("leveldb");
    path.join("CURRENT").exists().then_some(path)
}

/// The old app's localStorage, loaded into memory as key → decoded string.
type LegacyStore = HashMap<String, String>;

fn read_legacy_store(path: &Path) -> Result<LegacyStore, String> {
    #[cfg(windows)]
    return read_leveldb_store(path);
    #[cfg(not(windows))]
    return read_sqlite_store(path);
}

/// WebKitGTK's local-storage `ItemTable.value` column stores each string as
/// raw UTF-16LE bytes (confirmed by reading a real one directly), regardless
/// of the database's own declared text encoding — so this decodes it by
/// hand rather than treating it as ordinary SQLite TEXT.
///
/// Compiled on every platform (only *called* on non-Windows) so a Windows
/// build still type-checks it and the unit test below exercises it anywhere.
#[cfg_attr(windows, allow(dead_code))]
fn read_sqlite_store(path: &Path) -> Result<LegacyStore, String> {
    let conn = rusqlite::Connection::open(path)
        .map_err(|e| format!("Couldn't open the old app's settings database: {e}"))?;
    let mut stmt = conn
        .prepare("SELECT key, value FROM ItemTable")
        .map_err(|e| format!("Couldn't read the old app's settings database: {e}"))?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))
        .map_err(|e| format!("Couldn't read the old app's settings database: {e}"))?;
    Ok(rows
        .filter_map(Result::ok)
        .filter_map(|(k, bytes)| decode_utf16le(&bytes).map(|v| (k, v)))
        .collect())
}

/// The origin a real installed Tauri 2 build serves from on Windows
/// (WebView2 maps the custom protocol to `http://tauri.localhost`). Entries
/// under `http://localhost:1420` come from the Vite dev server and are
/// deliberately ignored, same as on Linux.
#[cfg(windows)]
const WEBVIEW2_ORIGIN: &str = "http://tauri.localhost";

/// Reads WebView2's Chromium localStorage LevelDB. The directory is copied
/// to a temp dir first (minus `LOCK`) because opening a LevelDB is a write
/// operation (log recovery, new manifest) — this must never touch the old
/// app's real store, and must work even while the old app is running and
/// holding its lock.
///
/// Chromium's on-disk format (`components/services/storage/dom_storage/`):
/// keys are `_<origin>\0<encoded key>` and values are `<encoded value>`,
/// where an encoded string is a 1-byte tag (`0` = UTF-16LE, `1` = Latin-1)
/// followed by the raw bytes.
#[cfg(windows)]
fn read_leveldb_store(path: &Path) -> Result<LegacyStore, String> {
    use rusty_leveldb::LdbIterator;

    let copy = std::env::temp_dir().join(format!("dvrdesk-legacy-ls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&copy);
    std::fs::create_dir_all(&copy)
        .map_err(|e| format!("Couldn't create a temp copy of the old app's settings: {e}"))?;
    let result = (|| {
        for entry in std::fs::read_dir(path)
            .map_err(|e| format!("Couldn't read the old app's settings directory: {e}"))?
        {
            let entry = entry.map_err(|e| format!("Couldn't read the old app's settings directory: {e}"))?;
            if entry.file_name() == "LOCK" || !entry.path().is_file() {
                continue;
            }
            std::fs::copy(entry.path(), copy.join(entry.file_name()))
                .map_err(|e| format!("Couldn't copy the old app's settings: {e}"))?;
        }

        let opts = rusty_leveldb::Options {
            create_if_missing: false,
            ..Default::default()
        };
        let mut db = rusty_leveldb::DB::open(&copy, opts)
            .map_err(|e| format!("Couldn't open the old app's settings database: {e}"))?;
        let mut iter = db
            .new_iter()
            .map_err(|e| format!("Couldn't read the old app's settings database: {e}"))?;

        let prefix = format!("_{WEBVIEW2_ORIGIN}\0");
        let mut store = LegacyStore::new();
        while let Some((k, v)) = iter.next() {
            let Some(encoded_key) = k.strip_prefix(prefix.as_bytes()) else {
                continue;
            };
            if let (Some(key), Some(value)) =
                (decode_chromium_string(encoded_key), decode_chromium_string(&v))
            {
                store.insert(key, value);
            }
        }
        Ok(store)
    })();
    let _ = std::fs::remove_dir_all(&copy);
    result
}

#[cfg(windows)]
fn decode_chromium_string(bytes: &[u8]) -> Option<String> {
    match bytes.split_first()? {
        (0, rest) => decode_utf16le(rest),
        (1, rest) => Some(rest.iter().map(|&b| b as char).collect()),
        _ => None,
    }
}

fn decode_utf16le(bytes: &[u8]) -> Option<String> {
    let utf16: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&utf16).ok()
}

fn read_bool(store: &LegacyStore, key: &str) -> bool {
    store.get(key).map(String::as_str) == Some("true")
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

    let store = read_legacy_store(&path)?;

    let servers: Vec<MigrationServer> = match store.get("dvr_servers") {
        Some(raw) => serde_json::from_str::<Vec<RawServer>>(raw)
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

    let active_server_id = store.get("dvr_active_server_id").cloned();

    let keybindings = match store.get("player_keybindings") {
        Some(raw) => {
            let r: RawKeybindings = serde_json::from_str(raw)
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

    let skip_intervals = match store.get("player_skip_intervals") {
        Some(raw) => {
            let r: RawSkipIntervals = serde_json::from_str(raw)
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

    let theme = store.get("app_theme").cloned().unwrap_or_default();

    Ok(apply(
        settings,
        servers,
        active_server_id,
        read_bool(&store, "diagnostics_enabled"),
        read_bool(&store, "live_show_hidden_channels"),
        keybindings,
        skip_intervals,
        &theme,
        read_bool(&store, "window_always_on_top"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// Builds a database shaped like WebKitGTK's real local-storage file
    /// (TEXT keys, UTF-16LE BLOB values) and reads it back, so the Linux
    /// path is covered on any platform.
    #[test]
    fn sqlite_store_decodes_webkitgtk_layout() {
        let path = std::env::temp_dir().join(format!("dvrdesk-ls-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB NOT NULL ON CONFLICT FAIL);",
            )
            .unwrap();
            let servers = r#"[{"id":"default","name":"Default","url":"http://192.168.3.150:8089","tailscaleUrl":"http://100.89.141.101:8089"}]"#;
            for (k, v) in [
                ("dvr_servers", servers),
                ("dvr_active_server_id", "default"),
                ("window_always_on_top", "true"),
            ] {
                conn.execute("INSERT INTO ItemTable VALUES (?1, ?2)", rusqlite::params![k, utf16le(v)])
                    .unwrap();
            }
        }
        let store = read_sqlite_store(&path);
        let _ = std::fs::remove_file(&path);
        let store = store.unwrap();

        assert_eq!(store.get("dvr_active_server_id").map(String::as_str), Some("default"));
        assert!(read_bool(&store, "window_always_on_top"));
        let servers: Vec<RawServer> = serde_json::from_str(&store["dvr_servers"]).unwrap();
        assert_eq!(servers[0].url, "http://192.168.3.150:8089");
        assert_eq!(servers[0].tailscale_url.as_deref(), Some("http://100.89.141.101:8089"));
    }

    /// Reads the real old-app store on this machine (WebView2 LevelDB on
    /// Windows, WebKitGTK SQLite on Linux), if one is installed. Run with
    /// `cargo test legacy_store -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn legacy_store_reads_real_install() {
        let Some(path) = detect_legacy_install() else {
            eprintln!("no legacy install found; skipping");
            return;
        };
        let store = read_legacy_store(&path).expect("read store");
        let mut keys: Vec<_> = store.keys().collect();
        keys.sort();
        for k in keys {
            eprintln!("{k} = {} chars", store[k].chars().count());
        }
        let mut settings = AppSettings::default();
        eprintln!("{:?}", import_from_legacy_install(&mut settings));
        for s in &settings.servers {
            eprintln!("server: {} ({}) {} ts={:?}", s.name, s.id, s.url, s.tailscale_url);
        }
        eprintln!("active={:?} theme={:?} aot={}", settings.active_server_id, settings.theme, settings.window_always_on_top);
    }
}
