//! Getting libmpv onto a Windows PC that doesn't have it. DVRDesk doesn't
//! ship libmpv: the available Windows builds are GPLv3 with many libraries
//! compiled in, and redistributing one means publishing the corresponding
//! source for all of it with every release. Instead, on request, the app
//! downloads `libmpv-2.dll` straight from shinchiro/mpv-winbuild-cmake's
//! GitHub releases — the user fetches it from its publisher, as they would
//! by hand. (winget's mpv package doesn't help: it only contains mpv.exe.)
//!
//! shinchiro keeps only the ~30 most recent releases, so a pinned version
//! would stop working within months. The latest release is used instead,
//! verified against the SHA-256 digest GitHub's API reports for the asset.
//!
//! Linux and macOS get libmpv from the system package manager, so only the
//! path helper exists there.

use std::path::PathBuf;

/// Where a downloaded libmpv lives (and where `mpv_sys` looks first).
pub fn downloaded_libmpv_path() -> Option<PathBuf> {
    crate::paths::local_data_dir().map(|d| d.join("mpv").join("libmpv-2.dll"))
}

/// `mpv-dev-x86_64-YYYYMMDD-git-<hash>.7z` — the baseline x86-64 build,
/// not the `-v3` one that needs AVX2.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_dev_archive(name: &str) -> bool {
    name.strip_prefix("mpv-dev-x86_64-")
        .and_then(|rest| rest.strip_suffix(".7z"))
        .and_then(|rest| rest.split('-').next())
        .is_some_and(|date| date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use std::io::Write as _;
    use std::sync::{Arc, Mutex};

    use serde::Deserialize;
    use sha2::Digest as _;

    const RELEASES_URL: &str =
        "https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases/latest";

    #[derive(Debug, Clone)]
    pub enum InstallState {
        Starting,
        Downloading { done: u64, total: u64 },
        Extracting,
        Done { vulkan_missing: bool },
        Failed(String),
    }

    #[derive(Deserialize)]
    struct Release {
        assets: Vec<Asset>,
    }

    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
        size: u64,
        digest: Option<String>,
    }

    /// Downloads and installs libmpv, reporting progress through `state`.
    pub async fn install(state: Arc<Mutex<InstallState>>, ctx: egui::Context) {
        let set = |s: InstallState| {
            *state.lock().unwrap() = s;
            ctx.request_repaint();
        };
        set(InstallState::Starting);
        match run(&set).await {
            Ok(vulkan_missing) => set(InstallState::Done { vulkan_missing }),
            Err(e) => {
                crate::logline!("mpv install: {e}");
                set(InstallState::Failed(e));
            }
        }
    }

    async fn run(set: &impl Fn(InstallState)) -> Result<bool, String> {
        let dest = super::downloaded_libmpv_path().ok_or("No app data folder is available.")?;
        let dir = dest.parent().expect("libmpv path has a parent").to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;

        let client = reqwest::Client::builder()
            .user_agent("dvrdesk-native")
            .build()
            .map_err(|e| e.to_string())?;
        let release: Release = client
            .get(RELEASES_URL)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Couldn't reach GitHub to find mpv: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Unexpected reply from GitHub: {e}"))?;
        let asset = release
            .assets
            .into_iter()
            .find(|a| super::is_dev_archive(&a.name))
            .ok_or("The latest mpv build doesn't include the expected download.")?;
        let expected = asset
            .digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(str::to_ascii_lowercase)
            .ok_or("GitHub didn't provide a checksum for the mpv download, so it can't be verified.")?;

        let archive = dir.join("mpv-dev.7z.part");
        let mut file = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
        let mut hasher = sha2::Sha256::new();
        let mut resp = client
            .get(&asset.browser_download_url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Download failed: {e}"))?;
        let total = resp.content_length().unwrap_or(asset.size);
        let mut done = 0u64;
        set(InstallState::Downloading { done, total });
        while let Some(chunk) = resp.chunk().await.map_err(|e| format!("Download failed: {e}"))? {
            hasher.update(&chunk);
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            done += chunk.len() as u64;
            set(InstallState::Downloading { done, total });
        }
        drop(file);
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected {
            let _ = std::fs::remove_file(&archive);
            return Err("The mpv download didn't match its published checksum, so it wasn't installed.".into());
        }

        set(InstallState::Extracting);
        let tmp = dir.join("libmpv-2.dll.part");
        let (archive2, tmp2) = (archive.clone(), tmp.clone());
        tokio::task::spawn_blocking(move || extract_libmpv(&archive2, &tmp2))
            .await
            .map_err(|e| e.to_string())??;
        let _ = std::fs::remove_file(&archive);
        std::fs::rename(&tmp, &dest).map_err(|e| format!("Couldn't save libmpv: {e}"))?;
        crate::logline!("mpv install: installed {} from {}", dest.display(), asset.name);

        // libmpv-2.dll hard-links vulkan-1.dll, which GPU drivers install.
        let vulkan_missing = std::env::var("SystemRoot")
            .map(|root| !std::path::Path::new(&root).join("System32").join("vulkan-1.dll").is_file())
            .unwrap_or(false);
        Ok(vulkan_missing)
    }

    fn extract_libmpv(archive: &std::path::Path, out: &std::path::Path) -> Result<(), String> {
        let mut found = false;
        sevenz_rust2::decompress_file_with_extract_fn(archive, ".", |entry, reader, _| {
            if entry.name() == "libmpv-2.dll" {
                let mut f = std::fs::File::create(out)?;
                std::io::copy(reader, &mut f)?;
                found = true;
            } else {
                std::io::copy(reader, &mut std::io::sink())?;
            }
            Ok(true)
        })
        .map_err(|e| format!("Couldn't unpack the mpv download: {e}"))?;
        if found {
            Ok(())
        } else {
            Err("The mpv download didn't contain libmpv-2.dll.".into())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn picks_baseline_x86_64_archive() {
        assert!(super::is_dev_archive("mpv-dev-x86_64-20260928-git-e470f8986e.7z"));
        assert!(!super::is_dev_archive("mpv-dev-x86_64-v3-20260928-git-e470f8986e.7z"));
        assert!(!super::is_dev_archive("mpv-dev-aarch64-20260928-git-e470f8986e.7z"));
        assert!(!super::is_dev_archive("mpv-x86_64-20260928-git-e470f8986e.7z"));
    }
}
