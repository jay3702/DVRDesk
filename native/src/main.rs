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

fn main() -> eframe::Result<()> {
    let settings = state::AppSettings::load();
    log::init(&settings.cache_path());

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("DVRDesk"),
        ..Default::default()
    };

    eframe::run_native(
        "DVRDesk",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
