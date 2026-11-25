#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod appearance;
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
mod settings;
mod shelf;
mod sound;
mod soundpage;
mod stream;
mod theme;
mod tree;
mod updater;
mod widgets;
#[cfg(windows)]
mod win;
#[cfg(not(windows))]
#[path = "unix.rs"]
mod win;
mod youtube;
mod ytplayer;

fn main() -> Result<(), eframe::Error> {
    library::migrate_old_data();
    // Linux: X11 (XWayland on a Wayland desktop), where the mini player can
    // be placed at the screen's edge and kept on top, and the YouTube player
    // can sit inside the window. Wayland allows neither.
    #[cfg(target_os = "linux")]
    {
        std::env::remove_var("WAYLAND_DISPLAY");
        // GTK (the YouTube player) would still find Wayland's default socket.
        std::env::set_var("GDK_BACKEND", "x11");
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Starlite")
            .with_app_id("starlite")
            .with_icon(std::sync::Arc::new(logo::window_icon()))
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([1040.0, 620.0])
            .with_decorations(false)
            // See-through where Starlite draws it so (glass on a plain theme, Windows).
            .with_transparent(cfg!(windows)),
        renderer: eframe::Renderer::Glow,
        // Windows: frames are paced by waiting for the compositor at the start
        // of each one (app.rs, set_frame); vsync on top would wait a second
        // refresh. Linux: vsync does it.
        vsync: !cfg!(windows),
        ..Default::default()
    };
    eframe::run_native(
        "Starlite",
        options,
        Box::new(|cc| Ok(Box::new(app::Starlite::new(cc)))),
    )
}
