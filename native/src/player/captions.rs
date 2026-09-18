//! Real track-list parsing for caption detection — the FFI work deferred
//! from the Phase 2 spike (an `MPV_FORMAT_NODE` walk: `track-list` is an
//! array of maps, a tagged-union tree, not a flat property).
//!
//! Confirmed during testing (via a real Clicker debug log and hands-on
//! experimentation) that recordings can carry up to two caption-like
//! tracks: a `mov_text`-coded one, which is Channels DVR's own AI-generated
//! "Py-Captions" feature (the same content the old app fetched from a
//! sidecar `.srt` — see the plan's §5), and a genuine `eia_608` broadcast
//! closed-caption track. mpv does not auto-select either — confirmed by
//! testing: nothing renders until `sid` is set explicitly.
//!
//! `codec`/`selected` on CaptionTrack aren't read anywhere yet — kept for a
//! future diagnostics/stats view rather than trimmed, since they cost
//! nothing to keep and were already free from the track-list walk.
#![allow(dead_code)]

use std::ffi::{c_void, CStr, CString};

use super::mpv_sys::{
    mpv_node, mpv_node_list, MpvApi, MpvHandle, MPV_FORMAT_FLAG, MPV_FORMAT_INT64, MPV_FORMAT_NODE,
    MPV_FORMAT_NODE_ARRAY, MPV_FORMAT_NODE_MAP, MPV_FORMAT_STRING,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionKind {
    PyCaptions,
    Broadcast,
    Other,
}

/// Matches the old app's three-way caption dropdown exactly (Off /
/// Broadcast / "Py-Captions (SRT)") — same options, different mechanism:
/// the old app read SRT off a network share for the Py-Captions case, this
/// reads it out of the container mpv already opened, no separate file
/// access at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptionMode {
    #[default]
    Off,
    PyCaptions,
    Broadcast,
}

#[derive(Debug, Clone)]
pub struct CaptionTrack {
    pub sid: u32,
    pub codec: String,
    pub kind: CaptionKind,
    pub selected: bool,
}

fn classify(codec: &str) -> CaptionKind {
    match codec {
        "mov_text" => CaptionKind::PyCaptions,
        "eia_608" | "eia_708" | "cea608" | "cea708" => CaptionKind::Broadcast,
        _ => CaptionKind::Other,
    }
}

unsafe fn node_map_get<'a>(list: &'a mpv_node_list, key: &str) -> Option<&'a mpv_node> {
    if list.keys.is_null() {
        return None;
    }
    for i in 0..list.num as isize {
        let key_ptr = *list.keys.offset(i);
        if key_ptr.is_null() {
            continue;
        }
        if CStr::from_ptr(key_ptr).to_string_lossy() == key {
            return Some(&*list.values.offset(i));
        }
    }
    None
}

unsafe fn node_as_string(node: &mpv_node) -> Option<String> {
    if node.format != MPV_FORMAT_STRING || node.u.string.is_null() {
        return None;
    }
    Some(CStr::from_ptr(node.u.string).to_string_lossy().into_owned())
}

unsafe fn node_as_int64(node: &mpv_node) -> Option<i64> {
    (node.format == MPV_FORMAT_INT64).then_some(node.u.int64)
}

unsafe fn node_as_flag(node: &mpv_node) -> Option<bool> {
    (node.format == MPV_FORMAT_FLAG).then_some(node.u.flag != 0)
}

/// Queries `track-list` and returns every subtitle-type track. Cheap enough
/// to call every frame after a file loads — and doing so naturally solves
/// the timing race found during manual testing (setting `sid` immediately
/// after `loadfile` silently no-ops because mpv hasn't populated the track
/// list yet): just keep checking until it's non-empty rather than trying to
/// catch one precise "file loaded" event.
pub fn track_list(api: &MpvApi, handle: MpvHandle) -> Vec<CaptionTrack> {
    let mut tracks = Vec::new();
    unsafe {
        let name = CString::new("track-list").unwrap();
        let mut root: mpv_node = std::mem::zeroed();
        let rc = (api.get_property)(
            handle,
            name.as_ptr(),
            MPV_FORMAT_NODE,
            &mut root as *mut mpv_node as *mut c_void,
        );
        if rc < 0 || root.format != MPV_FORMAT_NODE_ARRAY || root.u.list.is_null() {
            return tracks;
        }

        let array = &*root.u.list;
        for i in 0..array.num as isize {
            let entry = &*array.values.offset(i);
            if entry.format != MPV_FORMAT_NODE_MAP || entry.u.list.is_null() {
                continue;
            }
            let map = &*entry.u.list;

            let is_sub = node_map_get(map, "type")
                .and_then(|n| node_as_string(n))
                .as_deref()
                == Some("sub");
            if !is_sub {
                continue;
            }
            let Some(sid) = node_map_get(map, "id").and_then(|n| node_as_int64(n)) else {
                continue;
            };
            let codec = node_map_get(map, "codec")
                .and_then(|n| node_as_string(n))
                .unwrap_or_default();
            let selected = node_map_get(map, "selected")
                .and_then(|n| node_as_flag(n))
                .unwrap_or(false);

            tracks.push(CaptionTrack {
                sid: sid.max(0) as u32,
                kind: classify(&codec),
                codec,
                selected,
            });
        }

        (api.free_node_contents)(&mut root as *mut mpv_node);
    }
    tracks
}

pub fn find_by_kind(tracks: &[CaptionTrack], kind: CaptionKind) -> Option<u32> {
    tracks.iter().find(|t| t.kind == kind).map(|t| t.sid)
}
