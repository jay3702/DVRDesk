# DVRDesk Native

A native Rust/egui/libmpv desktop client for [Channels DVR](https://getchannels.com), rebuilding [DVRDesk](../)'s feature set (Recent Recordings, TV Shows, Movies, Videos, Collections, Search, Settings, offline Downloads, and a live program guide grid) without a webview — playback goes through libmpv directly instead of hls.js inside WebKitGTK, which fixed real reliability problems the webview-based version had no way around (crashes under retry pressure, recordings that simply wouldn't play).

## guide-history-service

A small standalone companion service, `../guide-history-service/`, that continuously captures Channels DVR's guide data so the grid can show programming history the DVR's own forward-only guide API doesn't retain on its own. It's independently versioned and can run anywhere on the network — see its own README, or the "Deploy & Manage Instances" section under Settings for a built-in local/remote (SSH) setup wizard.

## Acknowledgments

This rewrite owes a real debt to [Clicker](https://github.com/mackid1993/Clicker), mackid1993's own native Rust/egui/libmpv Channels DVR client. Clicker was never forked or copied from — but its publicly available source was read directly, multiple times, as a working reference for techniques that would otherwise have taken far longer to get right on our own: embedding libmpv's OpenGL rendering inside an `egui_glow` paint callback, the direct-file (`stream.mpg`) playback approach that sidesteps Channels DVR's HLS packaging, and the design of its downloads/timeshift/theming code. Most of this rewrite's v2 functionality traces back, directly or indirectly, to ideas first demonstrated there.

Thank you, mackid1993, for building Clicker and keeping it open — this project would look very different, and would have taken a lot longer to get here, without it.
