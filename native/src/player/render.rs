//! Bridges eframe's GL context into mpv's OpenGL render API, and renders
//! mpv's current frame into whichever framebuffer is bound at call time
//! (intended to be called from inside an `egui::PaintCallback`, where
//! egui_glow has already bound its target framebuffer for this draw call).

use std::ffi::{c_char, c_void, CStr};

use egui_glow::glow;
use egui_glow::glow::HasContext as _;

use super::mpv_sys::{
    mpv_opengl_fbo, mpv_opengl_init_params, mpv_render_param, MpvApi, MpvHandle, MpvRenderContext,
    MPV_RENDER_API_TYPE_OPENGL, MPV_RENDER_PARAM_API_TYPE, MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
    MPV_RENDER_PARAM_FLIP_Y, MPV_RENDER_PARAM_INVALID, MPV_RENDER_PARAM_OPENGL_FBO,
    MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
};

/// Trampoline from mpv's `extern "C" fn(ctx, name) -> *mut c_void` shape into
/// the Rust closure eframe hands us. mpv resolves and caches every GL entry
/// point it needs during `mpv_render_context_create` and does not call this
/// again afterward, so the closure only needs to stay alive for that call.
unsafe extern "C" fn get_proc_address_trampoline(
    ctx: *mut c_void,
    name: *const c_char,
) -> *mut c_void {
    let closure = &*(ctx as *const &dyn Fn(&CStr) -> *const c_void);
    let cname = CStr::from_ptr(name);
    (closure)(cname) as *mut c_void
}

pub unsafe fn create_render_context(
    api: &MpvApi,
    mpv: MpvHandle,
    get_proc_address: &dyn Fn(&CStr) -> *const c_void,
) -> Result<MpvRenderContext, String> {
    // `&dyn Fn` is a fat pointer and can't be cast directly to `*mut c_void`;
    // box it once more to get a thin pointer usable as the C callback ctx.
    let boxed: Box<&dyn Fn(&CStr) -> *const c_void> = Box::new(get_proc_address);
    let ctx_ptr = Box::into_raw(boxed) as *mut c_void;

    let mut init_params = mpv_opengl_init_params {
        get_proc_address: get_proc_address_trampoline,
        get_proc_address_ctx: ctx_ptr,
    };

    let api_type = MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void;

    let mut params = [
        mpv_render_param {
            type_: MPV_RENDER_PARAM_API_TYPE,
            data: api_type,
        },
        mpv_render_param {
            type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
            data: &mut init_params as *mut _ as *mut c_void,
        },
        mpv_render_param {
            type_: MPV_RENDER_PARAM_INVALID,
            data: std::ptr::null_mut(),
        },
    ];

    let mut render_ctx: MpvRenderContext = std::ptr::null_mut();
    let rc = (api.render_context_create)(&mut render_ctx, mpv, params.as_mut_ptr());

    // Safe to free now: mpv has already called get_proc_address for
    // everything it needs by the time render_context_create returns.
    drop(Box::from_raw(
        ctx_ptr as *mut &dyn Fn(&CStr) -> *const c_void,
    ));

    if rc < 0 {
        return Err(format!("mpv_render_context_create failed: {rc}"));
    }
    Ok(render_ctx)
}

/// GL state mpv's renderer is free to change and that egui_glow does *not*
/// reliably re-assert before its own subsequent draw calls — unlike bound
/// textures/programs/buffers, which every draw call sets explicitly anyway.
/// Viewport/scissor leaking from a video renderer into the following UI
/// paint is a well-known failure class for exactly this kind of shared-
/// context paint callback; observed once as a "speckled" corruption of
/// unrelated UI after repeated play/close cycles, gone after a restart —
/// consistent with leaked GL state rather than a data/logic bug.
struct GlStateGuard {
    viewport: [i32; 4],
    scissor_box: [i32; 4],
    scissor_test: bool,
    blend: bool,
    depth_test: bool,
    cull_face: bool,
}

impl GlStateGuard {
    unsafe fn capture(gl: &glow::Context) -> Self {
        let mut viewport = [0i32; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
        let mut scissor_box = [0i32; 4];
        gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut scissor_box);
        Self {
            viewport,
            scissor_box,
            scissor_test: gl.is_enabled(glow::SCISSOR_TEST),
            blend: gl.is_enabled(glow::BLEND),
            depth_test: gl.is_enabled(glow::DEPTH_TEST),
            cull_face: gl.is_enabled(glow::CULL_FACE),
        }
    }

    unsafe fn restore(&self, gl: &glow::Context) {
        gl.viewport(
            self.viewport[0],
            self.viewport[1],
            self.viewport[2],
            self.viewport[3],
        );
        gl.scissor(
            self.scissor_box[0],
            self.scissor_box[1],
            self.scissor_box[2],
            self.scissor_box[3],
        );
        set_enabled(gl, glow::SCISSOR_TEST, self.scissor_test);
        set_enabled(gl, glow::BLEND, self.blend);
        set_enabled(gl, glow::DEPTH_TEST, self.depth_test);
        set_enabled(gl, glow::CULL_FACE, self.cull_face);
    }
}

unsafe fn set_enabled(gl: &glow::Context, cap: u32, enabled: bool) {
    if enabled {
        gl.enable(cap);
    } else {
        gl.disable(cap);
    }
}

pub unsafe fn render_into_current_fbo(
    api: &MpvApi,
    render_ctx: MpvRenderContext,
    gl: &glow::Context,
    width: i32,
    height: i32,
) {
    let fbo_binding = gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING);
    let state = GlStateGuard::capture(gl);

    let mut fbo = mpv_opengl_fbo {
        fbo: fbo_binding,
        w: width,
        h: height,
        internal_format: 0,
    };
    let mut flip_y: i32 = 1;
    let mut block: i32 = 0;

    let mut params = [
        mpv_render_param {
            type_: MPV_RENDER_PARAM_OPENGL_FBO,
            data: &mut fbo as *mut _ as *mut c_void,
        },
        mpv_render_param {
            type_: MPV_RENDER_PARAM_FLIP_Y,
            data: &mut flip_y as *mut _ as *mut c_void,
        },
        mpv_render_param {
            type_: MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
            data: &mut block as *mut _ as *mut c_void,
        },
        mpv_render_param {
            type_: MPV_RENDER_PARAM_INVALID,
            data: std::ptr::null_mut(),
        },
    ];

    (api.render_context_render)(render_ctx, params.as_mut_ptr());
    (api.render_context_report_swap)(render_ctx);

    state.restore(gl);
}
