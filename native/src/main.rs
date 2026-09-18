mod api;
mod app;
mod async_bridge;
mod channel_genres;
mod deploy;
mod downloads;
mod log;
mod paths;
mod player;
mod screen;
mod state;
mod theme;
mod ui;

/// The app's own window/taskbar icon — reuses the existing Tauri app's icon
/// set directly (`../src-tauri/icons/`) rather than duplicating or
/// regenerating one, matching this whole rewrite's established convention
/// for shared assets (window geometry, server config format, etc. all
/// stayed independent, but icons didn't need to). Embedded at compile time
/// via `include_bytes!` — a packaged build can't rely on that relative path
/// existing on disk at runtime.
fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../../src-tauri/icons/128x128.png");
    let image = image::load_from_memory(bytes)
        .expect("bundled icon PNG failed to decode")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn main() -> eframe::Result<()> {
    let settings = state::AppSettings::load();
    log::init(&settings.cache_path());

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("DVRDesk")
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "DVRDesk",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
