#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod blur;
mod discord;
mod fx;
mod fxpanel;
mod glass;
mod icons;
mod library;
mod logo;
mod lyrics;
mod meta;
mod mini;
mod navidrome;
mod nowplaying;
mod pages;
mod preview;
mod player;
mod shelf;
mod sound;
mod theme;
mod tree;
mod widgets;
mod win;
mod youtube;
mod ytplayer;

fn main() -> Result<(), eframe::Error> {
    library::migrate_old_data();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Starlite")
            .with_app_id("starlite")
            .with_icon(std::sync::Arc::new(logo::window_icon()))
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([1040.0, 620.0])
            .with_decorations(false),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Starlite",
        options,
        Box::new(|cc| Ok(Box::new(app::Starlite::new(cc)))),
    )
}
