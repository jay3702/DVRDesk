//! Scrubber hover thumbnails.
//!
//! libmpv has no thumbnail API, so a second, headless mpv (no video output,
//! no audio, paused) opens the same stream on a worker thread. For each
//! hovered time it keyframe-seeks, waits for the frame, and writes a small
//! JPEG with mpv's own screenshot command; the UI thread decodes that into
//! an egui texture and caches it per time bucket.
//!
//! The worker is created lazily on the first hover (so it costs nothing —
//! and opens no extra connection to the DVR — unless the user scrubs), and
//! is torn down when the playing URL changes or playback closes.

use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, CString};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::mpv_sys::{self, MpvApi, MpvHandle};

/// One thumbnail per this many seconds of video.
pub const BUCKET_SECS: f64 = 10.0;
/// Thumbnail width in pixels; mpv scales the decoded frame to this.
const THUMB_WIDTH: u32 = 240;

pub fn bucket_of(secs: f64) -> u32 {
    (secs.max(0.0) / BUCKET_SECS) as u32
}

struct Decoded {
    bucket: u32,
    image: Option<egui::ColorImage>,
    /// Superseded before it ran — not a failure, so it may be asked again.
    skipped: bool,
}

struct Worker {
    url: String,
    requests: Sender<u32>,
    results: Receiver<Decoded>,
    textures: HashMap<u32, egui::TextureHandle>,
    /// Buckets sent to the worker and not yet answered (or answered with a
    /// failure — kept so a bad spot isn't retried every frame).
    asked: HashSet<u32>,
}

/// Interior-mutable so the overlay (which only holds shared references) can
/// drive it.
#[derive(Default)]
pub struct Thumbnails {
    worker: Mutex<Option<Worker>>,
}

impl Thumbnails {
    /// Tears the worker down (its channel closing makes the thread exit).
    pub fn reset(&self) {
        *self.worker.lock().unwrap() = None;
    }

    /// The thumbnail nearest `secs` for the stream at `url`, if one is ready
    /// — otherwise asks the worker for it and returns the closest cached
    /// neighbour so the card is never blank while it loads.
    pub fn get(&self, ctx: &egui::Context, url: &str, secs: f64) -> Option<egui::TextureHandle> {
        let mut guard = self.worker.lock().unwrap();
        if guard.as_ref().is_some_and(|w| w.url != url) {
            *guard = None;
        }
        if guard.is_none() {
            *guard = spawn(url, ctx.clone());
        }
        let w = guard.as_mut()?;

        while let Ok(done) = w.results.try_recv() {
            if done.skipped {
                w.asked.remove(&done.bucket);
            }
            if let Some(img) = done.image {
                let tex = ctx.load_texture(
                    format!("thumb-{}", done.bucket),
                    img,
                    egui::TextureOptions::LINEAR,
                );
                w.textures.insert(done.bucket, tex);
            }
        }

        let bucket = bucket_of(secs);
        if let Some(tex) = w.textures.get(&bucket) {
            return Some(tex.clone());
        }
        if w.asked.insert(bucket) {
            let _ = w.requests.send(bucket);
        }
        w.textures
            .iter()
            .min_by_key(|(b, _)| b.abs_diff(bucket))
            .map(|(_, t)| t.clone())
    }
}

fn spawn(url: &str, ctx: egui::Context) -> Option<Worker> {
    let api = mpv_sys::library().ok()?;
    let (req_tx, req_rx) = channel::<u32>();
    let (res_tx, res_rx) = channel::<Decoded>();
    let url_owned = url.to_string();
    std::thread::Builder::new()
        .name("scrub-thumbs".into())
        .spawn(move || run(api, &url_owned, req_rx, res_tx, ctx))
        .ok()?;
    Some(Worker {
        url: url.to_string(),
        requests: req_tx,
        results: res_rx,
        textures: HashMap::new(),
        asked: HashSet::new(),
    })
}

fn run(
    api: &'static MpvApi,
    url: &str,
    requests: Receiver<u32>,
    results: Sender<Decoded>,
    ctx: egui::Context,
) {
    unsafe {
        let h = (api.create)();
        if h.is_null() {
            return;
        }
        for (k, v) in [
            ("vo", "null"),
            ("ao", "null"),
            ("aid", "no"),
            ("sid", "no"),
            ("hwdec", "no"),
            ("pause", "yes"),
            ("hr-seek", "no"),
            ("osc", "no"),
            ("ytdl", "no"),
            ("config", "no"),
            ("load-scripts", "no"),
            ("vd-lavc-threads", "2"),
            ("vd-lavc-skiploopfilter", "all"),
            ("screenshot-format", "jpeg"),
            ("screenshot-jpeg-quality", "70"),
            ("vf", &format!("scale={THUMB_WIDTH}:-2")),
        ] {
            set_opt(api, h, k, v);
        }
        if (api.initialize)(h) < 0 {
            (api.terminate_destroy)(h);
            return;
        }

        let out = std::env::temp_dir().join(format!("dvrdesk-thumb-{}.jpg", std::process::id()));
        if command(api, h, &["loadfile", url]) >= 0 && wait_for(api, h, FILE_LOADED, 20.0) {
            // The initial seek-to-start also reports a restart; let it land.
            wait_for(api, h, PLAYBACK_RESTART, 5.0);
            while let Ok(mut bucket) = requests.recv() {
                // Scrubbing outpaces decoding: only the newest request matters.
                while let Ok(newer) = requests.try_recv() {
                    results
                        .send(Decoded {
                            bucket,
                            image: None,
                            skipped: true,
                        })
                        .ok();
                    bucket = newer;
                }
                let image = grab(api, h, bucket, &out);
                if results
                    .send(Decoded {
                        bucket,
                        image,
                        skipped: false,
                    })
                    .is_err()
                {
                    break;
                }
                ctx.request_repaint();
            }
        }
        let _ = std::fs::remove_file(&out);
        (api.terminate_destroy)(h);
    }
}

const FILE_LOADED: i32 = mpv_sys::MPV_EVENT_FILE_LOADED;
const PLAYBACK_RESTART: i32 = mpv_sys::MPV_EVENT_PLAYBACK_RESTART;

unsafe fn grab(api: &MpvApi, h: MpvHandle, bucket: u32, out: &PathBuf) -> Option<egui::ColorImage> {
    // Drop any stale restart event so the wait below is for *this* seek.
    while {
        let e = (api.wait_event)(h, 0.0);
        !e.is_null() && (*e).event_id != mpv_sys::MPV_EVENT_NONE
    } {}
    let t = (bucket as f64 * BUCKET_SECS).to_string();
    if command(api, h, &["seek", &t, "absolute+keyframes"]) < 0 {
        return None;
    }
    if !wait_for(api, h, PLAYBACK_RESTART, 8.0) {
        return None;
    }
    let _ = std::fs::remove_file(out);
    if command(
        api,
        h,
        &["screenshot-to-file", &out.to_string_lossy(), "video"],
    ) < 0
    {
        return None;
    }
    let img = image::open(out).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

/// Waits up to `secs` for event `id`; false on timeout, end-of-file or
/// shutdown.
unsafe fn wait_for(api: &MpvApi, h: MpvHandle, id: i32, secs: f64) -> bool {
    let deadline = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < deadline {
        let e = (api.wait_event)(h, 0.25);
        if e.is_null() {
            continue;
        }
        let eid = (*e).event_id;
        if eid == id {
            return true;
        }
        if eid == mpv_sys::MPV_EVENT_END_FILE || eid == mpv_sys::MPV_EVENT_SHUTDOWN {
            return false;
        }
    }
    false
}

unsafe fn command(api: &MpvApi, h: MpvHandle, args: &[&str]) -> i32 {
    let owned: Vec<CString> = args
        .iter()
        .map(|a| CString::new(*a).unwrap_or_default())
        .collect();
    let mut argv: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    argv.push(std::ptr::null());
    (api.command)(h, argv.as_ptr())
}

unsafe fn set_opt(api: &MpvApi, h: MpvHandle, k: &str, v: &str) {
    let k = CString::new(k).unwrap();
    let v = CString::new(v).unwrap();
    (api.set_option_string)(h, k.as_ptr(), v.as_ptr());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs libmpv and THUMB_TEST_URL"]
    fn grabs_frames() {
        let url = std::env::var("THUMB_TEST_URL").unwrap();
        let ctx = egui::Context::default();
        let t = Thumbnails::default();
        let start = Instant::now();
        let mut got = None;
        while start.elapsed() < Duration::from_secs(15) {
            if let Some(tex) = t.get(&ctx, &url, 35.0) {
                got = Some(tex);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let tex = got.expect("no thumbnail");
        assert!(
            tex.size()[0].abs_diff(THUMB_WIDTH as usize) <= 2,
            "{:?}",
            tex.size()
        );
    }
}
