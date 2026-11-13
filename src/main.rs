#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod blur;
mod css;
mod customize;
mod discord;
mod dsp;
mod engine;
mod fx;
mod fxpanel;
mod glass;
mod icons;
mod jellyfin;
mod lastfm;
mod library;
mod logo;
mod lyrics;
mod m3u;
mod meta;
mod mini;
mod nav;
mod navidrome;
mod nowplaying;
mod pages;
mod preview;
mod player;
mod services;
mod shelf;
mod sound;
mod soundpage;
mod stream;
mod theme;
mod tree;
mod updater;
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
