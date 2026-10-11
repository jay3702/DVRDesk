//! Whether the DVR can actually serve a recording's file. Its database can
//! list a recording whose file is gone (`stream.mpg` 404s) or empty (0
//! bytes) — on one real DVR, every recording made before a move to new
//! storage — and nothing in the recording's own metadata says so. Checked
//! when a recording's detail page is shown, and for a whole show's
//! episodes while its episode grid is open (a background scan, a few
//! checks at a time, so a large show doesn't flood the DVR).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Checks in flight at once. A show can have hundreds of episodes; the DVR
/// is also serving live TV while it answers these.
const MAX_CONCURRENT: usize = 4;

/// How long a result is trusted before the next detail-page view checks
/// again — long enough not to re-ask on every revisit, short enough that a
/// file fixed on the DVR stops showing as missing.
const RECHECK_AFTER: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileStatus {
    Checking,
    Present,
    /// In the DVR's database, but its file can't be found (HTTP 404).
    Missing,
    /// The file exists but has no data.
    Empty,
    /// The check itself failed (network error, unexpected status) — says
    /// nothing either way about the file, so nothing is shown for it.
    Unknown,
}

pub struct FileChecks {
    results: Arc<Mutex<HashMap<String, (FileStatus, Instant)>>>,
    limit: Arc<tokio::sync::Semaphore>,
    runtime: tokio::runtime::Handle,
    ctx: egui::Context,
}

impl FileChecks {
    pub fn new(runtime: tokio::runtime::Handle, ctx: egui::Context) -> Self {
        Self {
            results: Arc::default(),
            limit: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT)),
            runtime,
            ctx,
        }
    }

    /// The file's status, starting a check if there's no recent result.
    /// `Checking` until the DVR answers.
    pub fn status(&self, server_url: &str, recording_id: &str) -> FileStatus {
        let key = key(server_url, recording_id);
        let mut results = self.results.lock().unwrap();
        if let Some((status, at)) = results.get(&key) {
            if *status == FileStatus::Checking || at.elapsed() < RECHECK_AFTER {
                return status.clone();
            }
        }
        results.insert(key.clone(), (FileStatus::Checking, Instant::now()));
        drop(results);

        let url = crate::api::recordings::direct_play_url(server_url, recording_id);
        let results = self.results.clone();
        let limit = self.limit.clone();
        let ctx = self.ctx.clone();
        self.runtime.spawn(async move {
            let status = match limit.acquire().await {
                Ok(_permit) => check(&url).await,
                Err(_) => FileStatus::Unknown,
            };
            results
                .lock()
                .unwrap()
                .insert(key, (status, Instant::now()));
            ctx.request_repaint();
        });
        FileStatus::Checking
    }

    /// The last result, without starting a check.
    pub fn cached(&self, server_url: &str, recording_id: &str) -> Option<FileStatus> {
        let results = self.results.lock().unwrap();
        results
            .get(&key(server_url, recording_id))
            .map(|(status, _)| status.clone())
    }
}

fn key(server_url: &str, recording_id: &str) -> String {
    format!("{}|{recording_id}", server_url.trim_end_matches('/'))
}

/// Asks for the file's first byte only. An empty file comes back as a
/// success with no content (the DVR answers 200 with `Content-Length: 0`
/// rather than a range error).
async fn check(url: &str) -> FileStatus {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    else {
        return FileStatus::Unknown;
    };
    let response = match client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return FileStatus::Unknown,
    };
    match response.status() {
        reqwest::StatusCode::NOT_FOUND => FileStatus::Missing,
        s if s.is_success() && response.content_length() == Some(0) => FileStatus::Empty,
        s if s.is_success() => FileStatus::Present,
        _ => FileStatus::Unknown,
    }
}
