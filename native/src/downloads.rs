//! Offline downloads — a copy of a completed recording's file fetched from
//! the DVR to local disk, so it plays without the server in the loop.
//! Ported from Clicker's own `downloads.rs` (read in full from its public
//! repo before this was written): same directory-is-source-of-truth
//! design, same resumable-via-`Range` transfer, same `.part`-then-rename
//! atomicity so a file with the final name is always a complete one.
//!
//! Two deliberate departures from the reference:
//! - No repaint closure — this app already threads `egui::Context` through
//!   every async call site, so `Inner` just holds a cloned one and calls
//!   `.request_repaint()` directly.
//! - A `{id}.meta.json` snapshot (title/subtitle/thumbnail/url) is written
//!   alongside each download. Clicker's own screen cross-references a
//!   library cache that's always loaded for the app's whole lifetime; this
//!   app's screens each fetch lazily, on first visit, so a download from a
//!   past session could otherwise show up with no title at all the first
//!   time the Downloads screen is opened this run.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// How many downloads run at once. The rest queue — asking the DVR for
/// eight files at once doesn't make any of them arrive sooner, and it's
/// also serving live TV to the same household while it does it.
const MAX_ACTIVE: usize = 2;

const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

#[derive(Clone)]
pub enum Status {
    /// Accepted, waiting for a slot.
    Queued,
    /// Fraction complete, 0 to 1, or negative when the total size isn't
    /// known yet.
    Active(f32),
    /// Stopped, with the partial file kept and resumable. Negative for one
    /// recovered at startup — the bytes on disk are known but the total
    /// isn't, since nothing has asked the server how long the recording is
    /// since the process restarted.
    Paused(f32),
    Done(PathBuf),
    Failed(String),
}

impl Status {
    pub fn is_finished(&self) -> bool {
        matches!(self, Status::Done(_) | Status::Failed(_))
    }

    pub fn is_resumable(&self) -> bool {
        matches!(self, Status::Paused(_) | Status::Failed(_))
    }
}

enum Outcome {
    Done(PathBuf),
    Paused(f32),
    Cancelled,
}

/// A snapshot of what to show for a download, captured at `start()` time so
/// the Downloads screen never depends on some other screen having already
/// fetched this recording's metadata this run.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DownloadRecord {
    /// Needed to (re)fetch — including on Resume, which happens in a later
    /// session than the one that clicked Download.
    pub url: String,
    pub title: String,
    /// Pre-formatted by the caller (e.g. "S1E4 — Episode Title"); `None`
    /// for a movie or anything with no episode information.
    pub subtitle: Option<String>,
    pub thumbnail_url: Option<String>,
}

struct Inner {
    dir: PathBuf,
    states: Mutex<HashMap<String, Status>>,
    records: Mutex<HashMap<String, DownloadRecord>>,
    /// Set to make a running transfer pause or give up. Kept apart from
    /// `states` so the signal survives the state being replaced.
    signals: Mutex<HashMap<String, Arc<AtomicU8>>>,
    /// Waiting for a slot, oldest first.
    queue: Mutex<VecDeque<String>>,
    http: reqwest::Client,
    runtime: tokio::runtime::Handle,
    ctx: egui::Context,
}

pub struct Downloads {
    inner: Arc<Inner>,
}

impl Downloads {
    pub fn new(runtime: tokio::runtime::Handle, dir: PathBuf, ctx: egui::Context) -> Self {
        let mut states = HashMap::new();
        let mut records = HashMap::new();

        // The directory is the source of truth, not a manifest that could
        // disagree with it.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };

                // A .part is a transfer the process did not live to
                // finish. Kept, not swept — it's most of a file, the
                // server serves ranges, and throwing it away would mean
                // starting a multi-gigabyte download again because a lid
                // closed.
                if path.extension().is_some_and(|e| e == "part") {
                    states.insert(stem.to_string(), Status::Paused(-1.0));
                } else if path.extension().is_some_and(|e| e == "mpg") {
                    states.insert(stem.to_string(), Status::Done(path.clone()));
                } else if path.extension().is_some_and(|e| e == "json") {
                    if let Some(id) = stem.strip_suffix(".meta") {
                        if let Ok(contents) = std::fs::read_to_string(&path) {
                            if let Ok(record) = serde_json::from_str::<DownloadRecord>(&contents) {
                                records.insert(id.to_string(), record);
                            }
                        }
                    }
                }
            }
        }
        // A Done/Paused entry with no recovered metadata still needs
        // something to show — a placeholder good enough that Resume just
        // can't work until the user re-downloads from scratch, which is an
        // acceptable degraded edge (the metadata file was lost or
        // corrupted independently of the media/part file it describes).
        for id in states.keys() {
            records.entry(id.clone()).or_insert_with(|| DownloadRecord {
                url: String::new(),
                title: format!("Recording {id}"),
                subtitle: None,
                thumbnail_url: None,
            });
        }

        Self {
            inner: Arc::new(Inner {
                dir,
                states: Mutex::new(states),
                records: Mutex::new(records),
                signals: Mutex::new(HashMap::new()),
                queue: Mutex::new(VecDeque::new()),
                http: reqwest::Client::new(),
                runtime,
                ctx,
            }),
        }
    }

    /// The local file for a recording, when a finished download exists.
    pub fn local_path(&self, id: &str) -> Option<PathBuf> {
        match self.inner.states.lock().unwrap().get(id) {
            Some(Status::Done(path)) => Some(path.clone()),
            _ => None,
        }
    }

    /// Current status, if anything is known about this id — used to decide
    /// what a Download click should tell the user (started / already
    /// running / already have it), since `start()` itself stays silent
    /// about which case applied.
    pub fn status(&self, id: &str) -> Option<Status> {
        self.inner.states.lock().unwrap().get(id).cloned()
    }

    /// Everything known about a download: running, then waiting, then
    /// paused, then failed, then finished.
    pub fn entries(&self) -> Vec<(String, Status, DownloadRecord)> {
        let states = self.inner.states.lock().unwrap();
        let records = self.inner.records.lock().unwrap();
        let mut all: Vec<(String, Status, DownloadRecord)> = states
            .iter()
            .map(|(id, s)| {
                let record = records.get(id).cloned().unwrap_or_else(|| DownloadRecord {
                    url: String::new(),
                    title: format!("Recording {id}"),
                    subtitle: None,
                    thumbnail_url: None,
                });
                (id.clone(), s.clone(), record)
            })
            .collect();
        drop(states);
        drop(records);

        fn rank(status: &Status) -> u8 {
            match status {
                Status::Active(_) => 0,
                Status::Queued => 1,
                Status::Paused(_) => 2,
                Status::Failed(_) => 3,
                Status::Done(_) => 4,
            }
        }
        all.sort_by(|a, b| rank(&a.1).cmp(&rank(&b.1)).then_with(|| a.0.cmp(&b.0)));
        all
    }

    /// Stop a transfer but keep what has arrived. A queued one is paused
    /// outright rather than being left to start and immediately stop again
    /// the moment a slot frees.
    pub fn pause(&self, id: &str) {
        if let Some(signal) = self.inner.signals.lock().unwrap().get(id) {
            signal.store(PAUSE, Ordering::SeqCst);
        }
        let mut was_queued = false;
        self.inner.queue.lock().unwrap().retain(|waiting| {
            let keep = waiting != id;
            was_queued |= !keep;
            keep
        });
        if was_queued {
            self.inner
                .states
                .lock()
                .unwrap()
                .insert(id.to_string(), Status::Paused(-1.0));
            self.inner.pump();
        }
    }

    /// Delete a finished download, or abandon one still running — one entry
    /// point for both, since from the outside they're the same wish ("I
    /// don't want this") and which applies depends on timing the person
    /// clicking can't see.
    pub fn remove(&self, id: &str) {
        // Signal first — a running task checks it between chunks, and
        // deleting the state without it would leave the task writing to a
        // file nothing is tracking any more.
        if let Some(signal) = self.inner.signals.lock().unwrap().get(id) {
            signal.store(CANCEL, Ordering::SeqCst);
        }
        self.inner
            .queue
            .lock()
            .unwrap()
            .retain(|queued| queued != id);

        let previous = self.inner.states.lock().unwrap().remove(id);
        if let Some(Status::Done(path)) = previous {
            let _ = std::fs::remove_file(path);
        }
        self.inner.records.lock().unwrap().remove(id);
        // A partial file belongs to nothing now — removing is the one
        // place that deletes one; pausing and crashing both keep it.
        let _ = std::fs::remove_file(self.inner.dir.join(format!("{id}.part")));
        let _ = std::fs::remove_file(self.inner.dir.join(format!("{id}.meta.json")));

        self.inner.pump();
    }

    /// Forget everything finished or failed, deleting the local files.
    pub fn clear_finished(&self) {
        let finished: Vec<String> = self
            .inner
            .states
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, s)| s.is_finished())
            .map(|(id, _)| id.clone())
            .collect();
        for id in finished {
            self.remove(&id);
        }
    }

    /// Start fetching a recording, or continue one that was stopped —
    /// the same call for both: whether this begins or resumes is decided
    /// by what's on disk, not by which button was pressed.
    pub fn start(&self, id: &str, record: DownloadRecord) {
        {
            let mut states = self.inner.states.lock().unwrap();
            match states.get(id) {
                Some(Status::Done(_)) | Some(Status::Active(_)) | Some(Status::Queued) => return,
                _ => {}
            }
            states.insert(id.to_string(), Status::Queued);
        }
        self.inner
            .records
            .lock()
            .unwrap()
            .insert(id.to_string(), record.clone());
        if let Ok(json) = serde_json::to_vec(&record) {
            let _ = std::fs::create_dir_all(&self.inner.dir);
            if let Err(e) = std::fs::write(self.inner.dir.join(format!("{id}.meta.json")), json) {
                crate::logline!("downloads: couldn't write metadata for {id}: {e}");
            }
        }
        self.inner.queue.lock().unwrap().push_back(id.to_string());
        self.inner.pump();
    }
}

impl Inner {
    fn active(&self) -> usize {
        self.states
            .lock()
            .unwrap()
            .values()
            .filter(|s| matches!(s, Status::Active(_)))
            .count()
    }

    /// Start whatever the concurrency limit has room for.
    fn pump(self: &Arc<Self>) {
        loop {
            if self.active() >= MAX_ACTIVE {
                break;
            }
            let Some(id) = self.queue.lock().unwrap().pop_front() else {
                break;
            };

            // Cancelled or paused between being queued and being reached.
            if !matches!(self.states.lock().unwrap().get(&id), Some(Status::Queued)) {
                continue;
            }
            let Some(url) = self.records.lock().unwrap().get(&id).map(|r| r.url.clone()) else {
                continue;
            };

            let signal = Arc::new(AtomicU8::new(RUN));
            self.signals
                .lock()
                .unwrap()
                .insert(id.clone(), Arc::clone(&signal));
            self.states
                .lock()
                .unwrap()
                .insert(id.clone(), Status::Active(-1.0));

            let inner = Arc::clone(self);
            self.runtime.spawn(async move {
                let result = fetch(&inner, &url, &id, &signal).await;
                inner.signals.lock().unwrap().remove(&id);

                {
                    let mut states = inner.states.lock().unwrap();
                    // A cancelled download has already been forgotten by
                    // `remove` — must not be resurrected as Failed.
                    if states.contains_key(&id) {
                        match result {
                            Ok(Outcome::Done(path)) => {
                                states.insert(id.clone(), Status::Done(path))
                            }
                            Ok(Outcome::Paused(done)) => {
                                states.insert(id.clone(), Status::Paused(done))
                            }
                            Ok(Outcome::Cancelled) => states.remove(&id),
                            // Failed, not lost — whatever arrived is still
                            // on disk and pressing Resume picks it up.
                            Err(e) => states.insert(id.clone(), Status::Failed(format!("{e}"))),
                        };
                    }
                }

                inner.ctx.request_repaint();
                inner.pump();
            });
        }
        self.ctx.request_repaint();
    }
}

/// Fetch one recording, continuing from whatever is already on disk.
async fn fetch(
    inner: &Arc<Inner>,
    url: &str,
    id: &str,
    signal: &Arc<AtomicU8>,
) -> Result<Outcome, String> {
    use tokio::io::AsyncWriteExt;

    std::fs::create_dir_all(&inner.dir)
        .map_err(|e| format!("creating the downloads directory: {e}"))?;

    // Written to a .part name and renamed on completion, so a file with
    // the real name is always a whole one.
    let partial = inner.dir.join(format!("{id}.part"));
    let done = inner.dir.join(format!("{id}.mpg"));

    let have = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);

    let mut request = inner.http.get(url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?
        .error_for_status()
        .map_err(|e| format!("GET {url}: {e}"))?;

    // Whether the server honored the range decides both where writing
    // starts and what the total is. A server that ignores it answers 200
    // with the whole file, and appending that to what's already there
    // would produce a corrupt file one and a half recordings long — so the
    // only safe reading of a 200 is "start again".
    let resumed = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let (mut received, total) = if resumed {
        (
            have,
            content_range_total(&response)
                .or_else(|| response.content_length().map(|len| len + have)),
        )
    } else {
        (0, response.content_length())
    };

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&partial)
        .await
        .map_err(|e| format!("opening {}: {e}", partial.display()))?;

    let mut stream = response;
    let mut last_report = std::time::Instant::now();
    let fraction = |received: u64| {
        total
            .map(|t| (received as f32 / t as f32).clamp(0.0, 1.0))
            .unwrap_or(-1.0)
    };

    loop {
        let chunk = stream
            .chunk()
            .await
            .map_err(|e| format!("reading the download: {e}"))?;
        let Some(chunk) = chunk else { break };

        // Checked per chunk, not just before/after the loop — stopping
        // should stop the transfer within about one chunk, not after the
        // whole thing finishes.
        match signal.load(Ordering::SeqCst) {
            PAUSE => {
                let _ = file.flush().await;
                drop(file);
                return Ok(Outcome::Paused(fraction(received)));
            }
            CANCEL => {
                drop(file);
                let _ = tokio::fs::remove_file(&partial).await;
                return Ok(Outcome::Cancelled);
            }
            _ => {}
        }

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("writing the download: {e}"))?;
        received += chunk.len() as u64;

        // Throttled — updating shared state per chunk would repaint the
        // interface hundreds of times a second for no visible benefit.
        if last_report.elapsed().as_millis() >= 250 {
            last_report = std::time::Instant::now();
            let mut states = inner.states.lock().unwrap();
            if states.contains_key(id) {
                states.insert(id.to_string(), Status::Active(fraction(received)));
            }
            drop(states);
            inner.ctx.request_repaint();
        }
    }

    let _ = file.flush().await;
    drop(file);

    match signal.load(Ordering::SeqCst) {
        PAUSE => return Ok(Outcome::Paused(fraction(received))),
        CANCEL => {
            let _ = tokio::fs::remove_file(&partial).await;
            return Ok(Outcome::Cancelled);
        }
        _ => {}
    }

    tokio::fs::rename(&partial, &done)
        .await
        .map_err(|e| format!("finishing the download: {e}"))?;
    Ok(Outcome::Done(done))
}

/// The whole file's length, from `Content-Range: bytes 12-99/100`.
/// `Content-Length` on a partial response is the length of just the part,
/// so it can't be used as the total without adding back what was already
/// held — this header states the answer outright.
fn content_range_total(response: &reqwest::Response) -> Option<u64> {
    let value = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    value.rsplit('/').next()?.trim().parse().ok()
}
