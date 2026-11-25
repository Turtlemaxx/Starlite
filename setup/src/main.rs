//! Starlite Setup: installs Starlite for the current user (no admin rights):
//! the program in %LOCALAPPDATA%\Programs\Starlite (or a folder you pick),
//! Start menu and desktop shortcuts, and an entry in Windows' list of
//! installed apps whose Uninstall runs this same program with --uninstall.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("Starlite Setup is for Windows. On Linux, see the README for the one-line install.");
}

#[cfg(windows)]
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let uninstall = args.iter().any(|a| a == "--uninstall");
    // Without a window: --silent [--dir <folder>] [--no-desktop], or
    // --uninstall --silent [--delete-data].
    if args.iter().any(|a| a == "--silent" || a == "/S") {
        let code = win::silent(&args, uninstall);
        std::process::exit(code);
    }
    let icon = image::load_from_memory(include_bytes!("logo128.png")).ok().map(|i| {
        let i = i.to_rgba8();
        egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() }
    });
    let mut vp = egui::ViewportBuilder::default()
        .with_title(if uninstall { "Remove Starlite" } else { "Starlite Setup" })
        .with_inner_size([560.0, 372.0])
        .with_resizable(false)
        .with_maximize_button(false);
    if let Some(i) = icon {
        vp = vp.with_icon(std::sync::Arc::new(i));
    }
    let opts = eframe::NativeOptions { viewport: vp, renderer: eframe::Renderer::Glow, ..Default::default() };
    eframe::run_native("Starlite Setup", opts, Box::new(move |cc| Ok(Box::new(win::Setup::new(cc, uninstall)))))
}

#[cfg(windows)]
mod win {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Rect, Sense, Stroke, TextureHandle};

    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const APP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Starlite.exe"));
    const NOTICES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/THIRD-PARTY-NOTICES.txt"));
    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Starlite";
    const UNINSTALLER: &str = "Uninstall Starlite.exe";

    const BG: Color32 = Color32::from_rgb(28, 28, 31);
    const PANEL: Color32 = Color32::from_rgb(38, 38, 42);
    const LINE: Color32 = Color32::from_rgb(58, 58, 64);
    const TEXT: Color32 = Color32::from_rgb(240, 240, 242);
    const DIM: Color32 = Color32::from_rgb(150, 150, 158);
    const BAD: Color32 = Color32::from_rgb(235, 90, 100);

    enum Page {
        Welcome,
        Done,
        Failed(String),
        Remove,
        Removed,
    }

    pub struct Setup {
        page: Page,
        dir: PathBuf,
        desktop: bool,
        open_after: bool,
        wipe_data: bool,
        logo: TextureHandle,
    }

    fn font(size: f32, w: &str) -> FontId {
        FontId::new(size, if w == "regular" { FontFamily::Proportional } else { FontFamily::Name(w.into()) })
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn env_dir(var: &str) -> PathBuf {
        PathBuf::from(std::env::var(var).unwrap_or_default())
    }

    // ── Registry (the installed-apps entry) ──

    fn reg_read(name: &str) -> Option<String> {
        use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, wide(KEY).as_ptr(), wide(name).as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut size) };
        (r == 0).then(|| String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]))
    }

    fn reg_write(values: &[(&str, String)], dwords: &[(&str, u32)]) -> Result<(), String> {
        use windows_sys::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ};
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            if RegCreateKeyExW(HKEY_CURRENT_USER, wide(KEY).as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, std::ptr::null(), &mut key, std::ptr::null_mut()) != 0 {
                return Err("Couldn't add Starlite to the list of installed apps".into());
            }
            for (name, v) in values {
                let w = wide(v);
                RegSetValueExW(key, wide(name).as_ptr(), 0, REG_SZ, w.as_ptr() as *const u8, (w.len() * 2) as u32);
            }
            for (name, v) in dwords {
                RegSetValueExW(key, wide(name).as_ptr(), 0, REG_DWORD, v as *const u32 as *const u8, 4);
            }
            RegCloseKey(key);
        }
        Ok(())
    }

    fn reg_delete() {
        use windows_sys::Win32::System::Registry::{RegDeleteTreeW, HKEY_CURRENT_USER};
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(KEY).as_ptr()) };
    }

    // ── Shortcuts (through Windows' own scripting, no extra files) ──

    fn powershell(script: &str) -> bool {
        use std::os::windows::process::CommandExt;
        let ps = env_dir("SystemRoot").join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        std::process::Command::new(ps)
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .creation_flags(0x0800_0000)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn quote(p: &Path) -> String {
        format!("'{}'", p.display().to_string().replace('\'', "''"))
    }

    fn start_menu_link() -> PathBuf {
        env_dir("APPDATA").join(r"Microsoft\Windows\Start Menu\Programs\Starlite.lnk")
    }

    fn make_shortcuts(dir: &Path, desktop: bool) {
        let exe = dir.join("Starlite.exe");
        let one = |link: &str| {
            format!(
                "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({link}); $s.TargetPath={t}; $s.WorkingDirectory={d}; $s.IconLocation={t}+',0'; $s.Description='Starlite music player'; $s.Save();",
                t = quote(&exe),
                d = quote(dir)
            )
        };
        let mut script = one(&quote(&start_menu_link()));
        if desktop {
            script.push_str(&one("(Join-Path ([Environment]::GetFolderPath('Desktop')) 'Starlite.lnk')"));
        }
        powershell(&script);
    }

    fn remove_shortcuts() {
        let _ = std::fs::remove_file(start_menu_link());
        powershell("Remove-Item -LiteralPath (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Starlite.lnk') -ErrorAction SilentlyContinue");
    }

    // ── Install / remove ──

    fn install(dir: &Path, desktop: bool) -> Result<(), String> {
        if APP.is_empty() {
            return Err("This setup was built without Starlite inside it".into());
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {} ({e})", dir.display()))?;
        let exe = dir.join("Starlite.exe");
        // A Starlite that's running can't be overwritten, but it can be moved
        // aside (it tidies Starlite.old.exe away itself on its next start).
        if exe.exists() && std::fs::write(&exe, APP).is_err() {
            let old = dir.join("Starlite.old.exe");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&exe, &old).map_err(|e| format!("Close Starlite first, then try again ({e})"))?;
        }
        std::fs::write(&exe, APP).map_err(|e| format!("Couldn't write Starlite.exe ({e})"))?;
        let _ = std::fs::write(dir.join("THIRD-PARTY-NOTICES.txt"), NOTICES);
        // This program again, as the uninstaller.
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        let un = dir.join(UNINSTALLER);
        if me != un {
            std::fs::copy(&me, &un).map_err(|e| format!("Couldn't add the uninstaller ({e})"))?;
        }
        make_shortcuts(dir, desktop);
        let size_kb = ((APP.len() + NOTICES.len()) / 1024) as u32;
        reg_write(
            &[
                ("DisplayName", "Starlite".into()),
                ("DisplayVersion", VERSION.into()),
                ("Publisher", "Starlite".into()),
                ("DisplayIcon", format!("{},0", exe.display())),
                ("InstallLocation", dir.display().to_string()),
                ("UninstallString", format!("\"{}\" --uninstall", un.display())),
                ("QuietUninstallString", format!("\"{}\" --uninstall", un.display())),
                ("URLInfoAbout", "https://github.com/Turtlemaxx/Starlite".into()),
            ],
            &[("NoModify", 1), ("NoRepair", 1), ("EstimatedSize", size_kb)],
        )
    }

    fn remove(dir: &Path, wipe_data: bool) {
        remove_shortcuts();
        reg_delete();
        for f in ["Starlite.exe", "Starlite.old.exe", "THIRD-PARTY-NOTICES.txt"] {
            let _ = std::fs::remove_file(dir.join(f));
        }
        let _ = std::fs::remove_dir_all(dir.join("update"));
        if wipe_data {
            let _ = std::fs::remove_dir_all(env_dir("APPDATA").join("Starlite"));
        }
        // The uninstaller is this program: it's removed (with the folder,
        // if nothing else is left in it, like downloaded songs) once it has quit.
        use std::os::windows::process::CommandExt;
        let un = dir.join(UNINSTALLER);
        let cmd = format!("ping 127.0.0.1 -n 3 >nul & del /f /q \"{}\" & rmdir \"{}\"", un.display(), dir.display());
        // Passed as written: cmd.exe reads quotes its own way.
        let _ = std::process::Command::new(env_dir("SystemRoot").join(r"System32\cmd.exe")).arg("/c").raw_arg(format!("\"{cmd}\"")).creation_flags(0x0800_0000).spawn();
    }

    /// Installs or removes without a window; the exit code says how it went.
    pub fn silent(args: &[String], uninstall: bool) -> i32 {
        let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(PathBuf::from);
        if uninstall {
            let dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_default();
            remove(&dir, args.iter().any(|a| a == "--delete-data"));
            return 0;
        }
        let dir = after("--dir").unwrap_or_else(|| reg_read("InstallLocation").map(PathBuf::from).unwrap_or_else(|| env_dir("LOCALAPPDATA").join("Programs").join("Starlite")));
        match install(&dir, !args.iter().any(|a| a == "--no-desktop")) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }
    }

    fn launch(dir: &Path) {
        let _ = std::process::Command::new(dir.join("Starlite.exe")).current_dir(dir).spawn();
    }

    impl Setup {
        pub fn new(cc: &eframe::CreationContext<'_>, uninstall: bool) -> Self {
            let mut fonts = FontDefinitions::default();
            let fdir = env_dir("WINDIR").join("Fonts");
            for (name, file) in [("regular", "segoeui.ttf"), ("semibold", "seguisb.ttf"), ("bold", "segoeuib.ttf")] {
                let mut fam = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
                if let Ok(b) = std::fs::read(fdir.join(file)) {
                    fonts.font_data.insert(name.into(), Arc::new(FontData::from_owned(b)));
                    fam.insert(0, name.into());
                }
                let key = if name == "regular" { FontFamily::Proportional } else { FontFamily::Name(name.into()) };
                fonts.families.insert(key, fam);
            }
            cc.egui_ctx.set_fonts(fonts);
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let img = image::load_from_memory(include_bytes!("logo.png")).expect("logo").to_rgba8();
            let logo = cc.egui_ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], &img), egui::TextureOptions::LINEAR);
            // Where it was installed before, or where the uninstaller is, or the default.
            let here = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
            let dir = if uninstall {
                here.unwrap_or_default()
            } else {
                reg_read("InstallLocation").map(PathBuf::from).unwrap_or_else(|| env_dir("LOCALAPPDATA").join("Programs").join("Starlite"))
            };
            Setup { page: if uninstall { Page::Remove } else { Page::Welcome }, dir, desktop: true, open_after: true, wipe_data: false, logo }
        }
    }

    /// A button: filled for the main one, outlined otherwise.
    fn button(ui: &mut egui::Ui, r: Rect, label: &str, main: bool) -> bool {
        let resp = ui.interact(r, ui.id().with(label), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let h = resp.hovered();
        let p = ui.painter();
        let (fill, fg) = if main { (if h { Color32::from_rgb(255, 255, 255) } else { Color32::from_rgb(236, 236, 240) }, Color32::from_rgb(20, 20, 22)) } else { (if h { Color32::from_rgb(52, 52, 58) } else { PANEL }, TEXT) };
        p.rect_filled(r, CornerRadius::same(8), fill);
        if !main {
            p.rect_stroke(r, CornerRadius::same(8), Stroke::new(1.0_f32, LINE), egui::StrokeKind::Inside);
        }
        p.text(r.center(), Align2::CENTER_CENTER, label, font(13.5, "semibold"), fg);
        resp.clicked()
    }

    fn check(ui: &mut egui::Ui, at: egui::Pos2, label: &str, on: &mut bool) {
        let r = Rect::from_min_size(at, vec2(360.0, 26.0));
        let resp = ui.interact(r, ui.id().with(label), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            *on = !*on;
        }
        let b = Rect::from_center_size(pos2(at.x + 9.0, r.center().y), vec2(18.0, 18.0));
        let p = ui.painter();
        if *on {
            p.rect_filled(b, CornerRadius::same(5), Color32::from_rgb(236, 236, 240));
            let pts = [pos2(b.left() + 4.5, b.center().y), pos2(b.left() + 7.5, b.bottom() - 5.0), pos2(b.right() - 4.0, b.top() + 5.0)];
            p.line_segment([pts[0], pts[1]], Stroke::new(2.0_f32, Color32::from_rgb(20, 20, 22)));
            p.line_segment([pts[1], pts[2]], Stroke::new(2.0_f32, Color32::from_rgb(20, 20, 22)));
        } else {
            p.rect_stroke(b, CornerRadius::same(5), Stroke::new(1.5_f32, Color32::from_rgb(110, 110, 118)), egui::StrokeKind::Inside);
        }
        p.text(pos2(at.x + 28.0, r.center().y), Align2::LEFT_CENTER, label, font(13.5, "regular"), TEXT);
    }

    impl eframe::App for Setup {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(BG)).show(ctx, |ui| {
                let full = ui.max_rect();
                let p = ui.painter().clone();
                // Header: the logo, name, version.
                p.image(self.logo.id(), Rect::from_min_size(pos2(32.0, 30.0), vec2(64.0, 64.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                p.text(pos2(112.0, 48.0), Align2::LEFT_CENTER, "Starlite", font(26.0, "bold"), TEXT);
                p.text(pos2(112.0, 78.0), Align2::LEFT_CENTER, format!("Version {VERSION}"), font(13.0, "regular"), DIM);
                let foot = full.bottom() - 30.0;
                let (bw, bh) = (116.0, 36.0);
                let right = Rect::from_min_size(pos2(full.right() - 32.0 - bw, foot - bh * 0.5), vec2(bw, bh));
                let left = right.translate(vec2(-bw - 10.0, 0.0));

                match &self.page {
                    Page::Welcome => {
                        p.text(pos2(32.0, 132.0), Align2::LEFT_CENTER, "Install to", font(13.0, "semibold"), DIM);
                        let field = Rect::from_min_size(pos2(32.0, 146.0), vec2(full.width() - 64.0 - 104.0, 36.0));
                        p.rect_filled(field, CornerRadius::same(8), PANEL);
                        p.rect_stroke(field, CornerRadius::same(8), Stroke::new(1.0_f32, LINE), egui::StrokeKind::Inside);
                        let shown = self.dir.display().to_string();
                        let g = p.layout_no_wrap(shown.clone(), font(13.0, "regular"), TEXT);
                        let clip = p.with_clip_rect(field.shrink(8.0));
                        clip.galley(pos2(field.left() + 12.0, field.center().y - g.size().y * 0.5), g, TEXT);
                        if button(ui, Rect::from_min_size(pos2(field.right() + 8.0, field.top()), vec2(96.0, 36.0)), "Change\u{2026}", false) {
                            if let Some(d) = rfd::FileDialog::new().set_directory(&self.dir).pick_folder() {
                                // A folder of its own inside the one picked.
                                self.dir = if d.file_name().is_some_and(|n| n.eq_ignore_ascii_case("starlite")) { d } else { d.join("Starlite") };
                            }
                        }
                        check(ui, pos2(32.0, 200.0), "Desktop shortcut", &mut self.desktop);
                        check(ui, pos2(32.0, 230.0), "Open Starlite when it's done", &mut self.open_after);
                        p.text(pos2(32.0, foot), Align2::LEFT_CENTER, "No admin rights needed. Starlite updates itself.", font(12.0, "regular"), DIM);
                        if button(ui, left, "Cancel", false) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if button(ui, right, "Install", true) {
                            self.page = match install(&self.dir, self.desktop) {
                                Ok(()) => {
                                    if self.open_after {
                                        launch(&self.dir);
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    }
                                    Page::Done
                                }
                                Err(e) => Page::Failed(e),
                            };
                        }
                    }
                    Page::Done => {
                        p.text(pos2(32.0, 140.0), Align2::LEFT_CENTER, "Starlite is installed.", font(17.0, "semibold"), TEXT);
                        p.text(pos2(32.0, 168.0), Align2::LEFT_CENTER, "It's in the Start menu, and you can remove it in Settings \u{2192} Apps.", font(13.0, "regular"), DIM);
                        if button(ui, left, "Close", false) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if button(ui, right, "Open Starlite", true) {
                            launch(&self.dir);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    Page::Failed(e) => {
                        p.text(pos2(32.0, 140.0), Align2::LEFT_CENTER, "Starlite couldn't be installed.", font(17.0, "semibold"), TEXT);
                        let g = p.layout(e.clone(), font(13.0, "regular"), BAD, full.width() - 64.0);
                        p.galley(pos2(32.0, 160.0), g, BAD);
                        if button(ui, left, "Close", false) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if button(ui, right, "Try again", true) {
                            self.page = Page::Welcome;
                        }
                    }
                    Page::Remove => {
                        p.text(pos2(32.0, 140.0), Align2::LEFT_CENTER, "Remove Starlite from this PC?", font(17.0, "semibold"), TEXT);
                        p.text(pos2(32.0, 168.0), Align2::LEFT_CENTER, "Your downloaded songs stay where they are.", font(13.0, "regular"), DIM);
                        check(ui, pos2(32.0, 200.0), "Also delete my settings, playlists and logins", &mut self.wipe_data);
                        if button(ui, left, "Cancel", false) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if button(ui, right, "Remove", true) {
                            remove(&self.dir, self.wipe_data);
                            self.page = Page::Removed;
                        }
                    }
                    Page::Removed => {
                        p.text(pos2(32.0, 140.0), Align2::LEFT_CENTER, "Starlite was removed.", font(17.0, "semibold"), TEXT);
                        if button(ui, right, "Close", true) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            });
        }
    }
}
