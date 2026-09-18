//! A log file, because a packaged desktop build has no console to catch
//! `eprintln!` output — once this ships, every line printed today is
//! printed into a handle nobody is holding. Deliberately not a logging
//! framework: no levels, no filtering, no configuration. Anything this app
//! already decided was worth printing to stderr is worth keeping, and a
//! bug report becomes "send me that file" instead of "please run this from
//! a terminal."
//!
//! Rolls over once past a size cap, one previous generation kept — bounded
//! enough that this can never be the reason a disk fills, generous enough
//! that a fault from a little while ago is still in there.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 5 * 1024 * 1024;

struct LogFile {
    path: PathBuf,
    file: Option<File>,
}

fn state() -> &'static Mutex<LogFile> {
    static STATE: OnceLock<Mutex<LogFile>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(LogFile {
            path: PathBuf::new(),
            file: None,
        })
    })
}

/// Opens the log file at `{cache_dir}/dvrdesk.log`. Called once, early in
/// `main()`, right after settings load — the path depends on the
/// (possibly user-configured) cache directory, which isn't known any
/// earlier than that.
pub fn init(cache_dir: &std::path::Path) {
    let path = cache_dir.join("dvrdesk.log");
    let _ = std::fs::create_dir_all(cache_dir);

    let mut guard = state().lock().unwrap();
    guard.path = path.clone();
    guard.file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    if let Some(f) = &mut guard.file {
        let _ = writeln!(f, "\n=== DVRDesk started ===");
    }
}

/// Appends one timestamped line. Failures are silently ignored — logging
/// isn't important enough to interrupt anything over, and `init()` not
/// having been called yet (or having failed to find a writable directory)
/// just means this line is stderr-only, same as before this module existed.
pub fn write_line(msg: &str) {
    let Ok(mut guard) = state().lock() else {
        return;
    };
    if guard.file.is_none() {
        return;
    }

    if std::fs::metadata(&guard.path)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false)
    {
        let rolled = guard.path.with_extension("log.1");
        let _ = std::fs::remove_file(&rolled);
        let _ = std::fs::rename(&guard.path, &rolled);
        guard.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&guard.path)
            .ok();
    }

    if let Some(f) = &mut guard.file {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let _ = writeln!(f, "[{stamp}] {}", msg.trim_end());
    }
}

/// `eprintln!`-identical call syntax — prints to stderr (unchanged dev-time
/// behavior) and also appends to the log file, so every existing call site
/// becomes a one-token rename.
#[macro_export]
macro_rules! logline {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        eprintln!("{msg}");
        $crate::log::write_line(&msg);
    }};
}
