//! Where this app's own files live, platform-appropriate — settings, cache,
//! logs, and (via `AppSettings`'s own resolvers) the default homes for
//! downloads and the live buffer when the user hasn't pointed those
//! somewhere else.
//!
//! A thin wrapper around the `directories` crate (already a dependency,
//! already used once for `config_dir` alone in `state/settings.rs` before
//! this module existed) rather than a hand-rolled platform switch — the
//! crate already gets Windows/macOS/Linux right, and duplicating that would
//! just be a second, worse copy of the same logic.

use std::path::PathBuf;
use std::sync::OnceLock;

fn project_dirs() -> Option<&'static directories::ProjectDirs> {
    static DIRS: OnceLock<Option<directories::ProjectDirs>> = OnceLock::new();
    DIRS.get_or_init(|| directories::ProjectDirs::from("com", "jay", "dvrdesk-native"))
        .as_ref()
}

/// Settings — `~/.config/dvrdesk-native` on Linux, roams with the user
/// profile on every platform.
pub fn config_dir() -> Option<PathBuf> {
    project_dirs().map(|d| d.config_dir().to_path_buf())
}

/// Large, rebuildable-if-lost data — the default home for downloads and the
/// live buffer when the user hasn't configured a directory of their own.
pub fn data_dir() -> Option<PathBuf> {
    project_dirs().map(|d| d.data_dir().to_path_buf())
}

/// This app's own cache and log file, when the user hasn't configured a
/// directory of their own.
pub fn cache_dir() -> Option<PathBuf> {
    project_dirs().map(|d| d.cache_dir().to_path_buf())
}
