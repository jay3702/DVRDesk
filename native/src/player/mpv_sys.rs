//! Minimal libmpv client + render API surface, loaded dynamically at runtime
//! via `libloading` rather than linked against `libmpv-dev`. Only the subset
//! actually used by `player/` is declared — this is not a general-purpose
//! libmpv binding.

#![allow(non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_double, c_int, c_void};
use std::sync::OnceLock;

use libloading::Library;

pub type MpvHandle = *mut c_void;
pub type MpvRenderContext = *mut c_void;

pub const MPV_FORMAT_NONE: c_int = 0;
pub const MPV_FORMAT_STRING: c_int = 1;
pub const MPV_FORMAT_FLAG: c_int = 3;
pub const MPV_FORMAT_INT64: c_int = 4;
pub const MPV_FORMAT_DOUBLE: c_int = 5;
pub const MPV_FORMAT_NODE: c_int = 6;
pub const MPV_FORMAT_NODE_ARRAY: c_int = 7;
pub const MPV_FORMAT_NODE_MAP: c_int = 8;
pub const MPV_FORMAT_BYTE_ARRAY: c_int = 9;

pub const MPV_EVENT_NONE: c_int = 0;
pub const MPV_EVENT_SHUTDOWN: c_int = 1;
pub const MPV_EVENT_LOG_MESSAGE: c_int = 2;
pub const MPV_EVENT_GET_PROPERTY_REPLY: c_int = 3;
pub const MPV_EVENT_SET_PROPERTY_REPLY: c_int = 4;
pub const MPV_EVENT_COMMAND_REPLY: c_int = 5;
pub const MPV_EVENT_START_FILE: c_int = 6;
pub const MPV_EVENT_END_FILE: c_int = 7;
pub const MPV_EVENT_FILE_LOADED: c_int = 8;
pub const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;
pub const MPV_EVENT_VIDEO_RECONFIG: c_int = 17;

pub const MPV_RENDER_PARAM_INVALID: c_int = 0;
pub const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
pub const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
pub const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
pub const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
pub const MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;

pub const MPV_RENDER_API_TYPE_OPENGL: &[u8] = b"opengl\0";

#[repr(C)]
pub struct mpv_event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_render_param {
    pub type_: c_int,
    pub data: *mut c_void,
}

pub type GetProcAddressFn =
    unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void;

#[repr(C)]
pub struct mpv_opengl_init_params {
    pub get_proc_address: GetProcAddressFn,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

/// Tagged-union node used for structured properties like `track-list`
/// (`MPV_FORMAT_NODE_ARRAY` of `MPV_FORMAT_NODE_MAP`s). Only the union
/// members actually read by `player/captions.rs` are meaningful here —
/// `ba` (byte array) is declared for layout correctness but never
/// dereferenced, since nothing this app reads uses that format.
#[repr(C)]
pub union mpv_node_u {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double_: c_double,
    pub list: *mut mpv_node_list,
    pub ba: *mut c_void,
}

#[repr(C)]
pub struct mpv_node {
    pub u: mpv_node_u,
    pub format: c_int,
}

#[repr(C)]
pub struct mpv_node_list {
    pub num: c_int,
    pub values: *mut mpv_node,
    /// Non-null only for `MPV_FORMAT_NODE_MAP`; null for `NODE_ARRAY`.
    pub keys: *mut *mut c_char,
}

/// Function-pointer table resolved from the dynamically loaded libmpv.
/// Deliberately not a `-sys` crate: avoids needing `libmpv-dev` headers or
/// an import lib at build time on any platform, and avoids MSVC/mingw
/// linking mismatches on Windows (mpv's Windows builds are mingw-built).
pub struct MpvApi {
    pub create: unsafe extern "C" fn() -> MpvHandle,
    pub initialize: unsafe extern "C" fn(MpvHandle) -> c_int,
    pub set_option_string: unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int,
    pub command: unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int,
    pub get_property: unsafe extern "C" fn(MpvHandle, *const c_char, c_int, *mut c_void) -> c_int,
    pub set_property_string: unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int,
    /// Must be called on any `mpv_node` populated via `get_property` with
    /// `MPV_FORMAT_NODE` — mpv allocates the node tree, this frees it.
    pub free_node_contents: unsafe extern "C" fn(*mut mpv_node),
    pub request_event: unsafe extern "C" fn(MpvHandle, c_int, c_int) -> c_int,
    pub wait_event: unsafe extern "C" fn(MpvHandle, c_double) -> *mut mpv_event,
    pub terminate_destroy: unsafe extern "C" fn(MpvHandle),
    pub error_string: unsafe extern "C" fn(c_int) -> *const c_char,

    pub render_context_create:
        unsafe extern "C" fn(*mut MpvRenderContext, MpvHandle, *mut mpv_render_param) -> c_int,
    pub render_context_render:
        unsafe extern "C" fn(MpvRenderContext, *mut mpv_render_param) -> c_int,
    pub render_context_report_swap: unsafe extern "C" fn(MpvRenderContext),
    pub render_context_free: unsafe extern "C" fn(MpvRenderContext),

    // Kept alive for the lifetime of the process; symbols above borrow from it.
    _library: Library,
}

/// Per-OS candidate library names/paths, tried in order.
fn candidates() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["mpv-2.dll", "mpv-1.dll", "libmpv-2.dll"]
    } else if cfg!(target_os = "macos") {
        &[
            "libmpv.2.dylib",
            "/opt/homebrew/lib/libmpv.2.dylib",
            "/usr/local/lib/libmpv.2.dylib",
        ]
    } else {
        &["libmpv.so.2", "libmpv.so.1", "libmpv.so"]
    }
}

static API: OnceLock<Result<MpvApi, String>> = OnceLock::new();

pub fn library() -> Result<&'static MpvApi, &'static str> {
    let result = API.get_or_init(|| unsafe { load() });
    result.as_ref().map_err(|e| e.as_str())
}

unsafe fn load() -> Result<MpvApi, String> {
    let mut last_err = String::new();
    let mut lib_opt: Option<Library> = None;
    for name in candidates() {
        match Library::new(name) {
            Ok(lib) => {
                lib_opt = Some(lib);
                break;
            }
            Err(e) => last_err = format!("{name}: {e}"),
        }
    }
    let lib = lib_opt.ok_or_else(|| {
        format!(
            "libmpv not found (tried {:?}); last error: {last_err}. Install mpv/libmpv2.",
            candidates()
        )
    })?;

    macro_rules! sym {
        ($name:literal) => {
            *lib.get(concat!($name, "\0").as_bytes())
                .map_err(|e| format!("missing symbol {}: {e}", $name))?
        };
    }

    let api = MpvApi {
        create: sym!("mpv_create"),
        initialize: sym!("mpv_initialize"),
        set_option_string: sym!("mpv_set_option_string"),
        command: sym!("mpv_command"),
        get_property: sym!("mpv_get_property"),
        set_property_string: sym!("mpv_set_property_string"),
        free_node_contents: sym!("mpv_free_node_contents"),
        request_event: sym!("mpv_request_event"),
        wait_event: sym!("mpv_wait_event"),
        terminate_destroy: sym!("mpv_terminate_destroy"),
        error_string: sym!("mpv_error_string"),
        render_context_create: sym!("mpv_render_context_create"),
        render_context_render: sym!("mpv_render_context_render"),
        render_context_report_swap: sym!("mpv_render_context_report_swap"),
        render_context_free: sym!("mpv_render_context_free"),
        _library: lib,
    };
    Ok(api)
}
