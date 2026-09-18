pub mod captions;
pub mod commercial_skip;
pub mod keybindings;
pub mod mpv_sys;
pub mod render;

use std::ffi::{c_void, CStr, CString};
use std::sync::Arc;

use mpv_sys::{MpvApi, MpvHandle, MpvRenderContext};

/// Minimal player for the Phase 2 risk spike: proves libmpv loads, renders
/// into eframe's GL context via an egui::PaintCallback, and plays real
/// Channels DVR manifest URLs. Event-thread/property-observation wiring
/// (Msg channel, position-changed notifications) is deferred to Phase 3
/// once this core risk is validated — for now, playback position is polled
/// directly each frame, which is sufficient to prove feasibility.
pub struct Player {
    api: &'static MpvApi,
    handle: MpvHandle,
    render_ctx: MpvRenderContext,
}

// The mpv handle/render context are only ever touched from the UI thread in
// this spike (no background event thread yet); Send+Sync just lets Player
// live in App without fighting the borrow checker over &mut self access.
unsafe impl Send for Player {}
unsafe impl Sync for Player {}

impl Player {
    pub fn new(get_proc_address: &dyn Fn(&CStr) -> *const c_void) -> Result<Self, String> {
        let api = mpv_sys::library().map_err(|e| e.to_string())?;
        unsafe {
            let handle = (api.create)();
            if handle.is_null() {
                return Err("mpv_create returned null".into());
            }

            set_option(api, handle, "vo", "libmpv");
            set_option(api, handle, "hwdec", "auto-safe");
            set_option(api, handle, "keep-open", "yes");
            set_option(api, handle, "osc", "no");

            let rc = (api.initialize)(handle);
            if rc < 0 {
                let msg = mpv_error_string(api, rc);
                (api.terminate_destroy)(handle);
                return Err(format!("mpv_initialize failed ({rc}): {msg}"));
            }

            let render_ctx = match render::create_render_context(api, handle, get_proc_address) {
                Ok(ctx) => ctx,
                Err(e) => {
                    (api.terminate_destroy)(handle);
                    return Err(e);
                }
            };

            Ok(Player {
                api,
                handle,
                render_ctx,
            })
        }
    }

    pub fn load_url(&self, url: &str) {
        unsafe {
            let load = CString::new("loadfile").unwrap();
            let uri = CString::new(url).unwrap_or_else(|_| CString::new("").unwrap());
            let argv = [load.as_ptr(), uri.as_ptr(), std::ptr::null()];
            (self.api.command)(self.handle, argv.as_ptr());
            // mpv's `pause` property is persistent across loadfile calls —
            // it does not reset to "playing" just because a new file was
            // loaded. Without this, a Play click after any earlier Pause
            // click loads the new file but leaves it sitting paused, which
            // looks indistinguishable from "won't play anything."
            set_property_runtime(self.api, self.handle, "pause", "no");
        }
    }

    /// Actually releases the current file (closes the underlying DVR
    /// session/connection) rather than just pausing — pausing alone leaves
    /// the network stream and decoder held open.
    pub fn stop(&self) {
        unsafe {
            let stop = CString::new("stop").unwrap();
            let argv = [stop.as_ptr(), std::ptr::null()];
            (self.api.command)(self.handle, argv.as_ptr());
        }
    }

    /// Absolute seek, in seconds. Used for resume-on-open — mpv's string
    /// property setter parses the numeric value directly, no separate
    /// double-typed setter needed.
    pub fn seek(&self, secs: f64) {
        unsafe {
            set_property_runtime(self.api, self.handle, "time-pos", &secs.to_string());
        }
    }

    pub fn set_pause(&self, paused: bool) {
        unsafe {
            set_property_runtime(
                self.api,
                self.handle,
                "pause",
                if paused { "yes" } else { "no" },
            );
        }
    }

    pub fn position_secs(&self) -> Option<f64> {
        unsafe { get_double(self.api, self.handle, "time-pos") }
    }

    pub fn duration_secs(&self) -> Option<f64> {
        unsafe { get_double(self.api, self.handle, "duration") }
    }

    pub fn paused(&self) -> bool {
        unsafe { get_flag(self.api, self.handle, "pause").unwrap_or(false) }
    }

    /// Diagnostic stats — the mpv-native equivalents of the old app's
    /// hls.js-specific "stats for nerds" fields (dropped/decoded frames,
    /// buffer-ahead, bandwidth). hls.js's concepts (ABR "level", bandwidth
    /// estimate) don't map onto mpv at all, so this is a fresh set of
    /// fields reflecting what mpv actually exposes, not a field-for-field
    /// port.
    pub fn dropped_frames(&self) -> Option<i64> {
        unsafe { get_int64(self.api, self.handle, "frame-drop-count") }
    }

    /// Audio/video desync in seconds — positive/negative indicates which
    /// stream is ahead. `None` when not meaningful (e.g. no video track).
    pub fn avsync_secs(&self) -> Option<f64> {
        unsafe { get_double(self.api, self.handle, "avsync") }
    }

    /// Seconds of demuxed data cached ahead of the current position — the
    /// mpv-native equivalent of hls.js's buffer-ahead stat.
    pub fn buffer_ahead_secs(&self) -> Option<f64> {
        unsafe { get_double(self.api, self.handle, "demuxer-cache-duration") }
    }

    pub fn video_bitrate_bps(&self) -> Option<f64> {
        unsafe { get_double(self.api, self.handle, "video-bitrate") }
    }

    pub fn audio_bitrate_bps(&self) -> Option<f64> {
        unsafe { get_double(self.api, self.handle, "audio-bitrate") }
    }

    /// Every subtitle-type track currently known to mpv — empty until the
    /// stream has been probed enough to populate `track-list`. Cheap to
    /// call every frame; see `player::captions` for why that's the
    /// intended usage pattern rather than a one-shot post-load check.
    pub fn caption_tracks(&self) -> Vec<captions::CaptionTrack> {
        captions::track_list(self.api, self.handle)
    }

    /// Selects a subtitle/caption track by mpv's track id ("sid"). mpv does
    /// not auto-select embedded CEA-608/708 caption tracks the way it does a
    /// "real" subtitle stream — confirmed experimentally: nothing renders
    /// until this is set explicitly, even though ffmpeg's demuxer exposes
    /// the track. `0` clears the selection (subtitles off).
    pub fn set_sid(&self, sid: u32) {
        unsafe {
            if sid == 0 {
                set_property_runtime(self.api, self.handle, "sid", "no");
            } else {
                set_property_runtime(self.api, self.handle, "sid", &sid.to_string());
            }
        }
    }

    /// Renders mpv's current frame into whichever framebuffer is bound —
    /// call from inside an `egui::PaintCallback`.
    pub fn render(&self, gl: &egui_glow::glow::Context, width: i32, height: i32) {
        unsafe {
            render::render_into_current_fbo(self.api, self.render_ctx, gl, width, height);
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        unsafe {
            (self.api.render_context_free)(self.render_ctx);
            (self.api.terminate_destroy)(self.handle);
        }
    }
}

/// Builds an `egui::PaintCallback` that renders this player's current frame
/// into `rect`. Centralizes the `Send + Sync` workaround `CallbackFn::new`'s
/// bounds require for a borrowed `&Player` (see the inline comment) so call
/// sites don't each need to reimplement it — this was starting to get
/// copy-pasted per screen.
///
/// SAFETY: the returned callback must not outlive the frame it was created
/// for — it holds a raw pointer derived from `player`'s borrow, valid only
/// because egui_glow invokes paint callbacks synchronously within the same
/// `update()` call that creates them, never stored for later.
pub fn paint_callback(
    player: &Player,
    rect: egui::Rect,
    ctx: &egui::Context,
) -> egui::PaintCallback {
    struct SendPtr(*const Player);
    unsafe impl Send for SendPtr {}
    unsafe impl Sync for SendPtr {}
    impl SendPtr {
        fn get(&self) -> *const Player {
            self.0
        }
    }
    let player_ptr = SendPtr(player as *const Player);
    let width = (rect.width() * ctx.pixels_per_point()) as i32;
    let height = (rect.height() * ctx.pixels_per_point()) as i32;

    egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |_info, painter| {
            let gl = painter.gl();
            unsafe {
                (*player_ptr.get()).render(gl, width, height);
            }
        })),
    }
}

unsafe fn mpv_error_string(api: &MpvApi, code: i32) -> String {
    let ptr = (api.error_string)(code);
    if ptr.is_null() {
        return "unknown error".into();
    }
    CStr::from_ptr(ptr).to_string_lossy().into_owned()
}

unsafe fn set_option(api: &MpvApi, handle: MpvHandle, name: &str, value: &str) {
    let name = CString::new(name).unwrap();
    let value = CString::new(value).unwrap();
    (api.set_option_string)(handle, name.as_ptr(), value.as_ptr());
}

unsafe fn set_property_runtime(api: &MpvApi, handle: MpvHandle, name: &str, value: &str) {
    let name = CString::new(name).unwrap();
    let value = CString::new(value).unwrap();
    (api.set_property_string)(handle, name.as_ptr(), value.as_ptr());
}

unsafe fn get_double(api: &MpvApi, handle: MpvHandle, name: &str) -> Option<f64> {
    let name = CString::new(name).unwrap();
    let mut value: f64 = 0.0;
    let rc = (api.get_property)(
        handle,
        name.as_ptr(),
        mpv_sys::MPV_FORMAT_DOUBLE,
        &mut value as *mut f64 as *mut c_void,
    );
    if rc >= 0 && value.is_finite() {
        Some(value)
    } else {
        None
    }
}

unsafe fn get_int64(api: &MpvApi, handle: MpvHandle, name: &str) -> Option<i64> {
    let name = CString::new(name).unwrap();
    let mut value: i64 = 0;
    let rc = (api.get_property)(
        handle,
        name.as_ptr(),
        mpv_sys::MPV_FORMAT_INT64,
        &mut value as *mut i64 as *mut c_void,
    );
    if rc >= 0 {
        Some(value)
    } else {
        None
    }
}

unsafe fn get_flag(api: &MpvApi, handle: MpvHandle, name: &str) -> Option<bool> {
    let name = CString::new(name).unwrap();
    let mut value: i32 = 0;
    let rc = (api.get_property)(
        handle,
        name.as_ptr(),
        mpv_sys::MPV_FORMAT_FLAG,
        &mut value as *mut i32 as *mut c_void,
    );
    if rc >= 0 {
        Some(value != 0)
    } else {
        None
    }
}
