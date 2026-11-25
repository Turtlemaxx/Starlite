//! Windows 11 window styling for the borderless window.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};

/// Work area and full bounds (physical px: left, top, right, bottom) of the
/// monitor at (or nearest to) a point.
pub fn monitor_rects(x: i32, y: i32) -> ([f32; 4], [f32; 4]) {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    let r0 = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, rcMonitor: r0, rcWork: r0, dwFlags: 0 };
    unsafe {
        let m = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(m, &mut mi);
    }
    let f = |r: RECT| [r.left as f32, r.top as f32, r.right as f32, r.bottom as f32];
    (f(mi.rcWork), f(mi.rcMonitor))
}

/// The mouse cursor (physical px).
pub fn cursor_pos() -> (f32, f32) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut p);
    }
    (p.x as f32, p.y as f32)
}

/// Is the mouse cursor over this window?
pub fn cursor_in_window(hwnd: isize) -> bool {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};
    let mut p = POINT { x: 0, y: 0 };
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe {
        if GetCursorPos(&mut p) == 0 || GetWindowRect(hwnd as _, &mut r) == 0 {
            return false;
        }
    }
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

/// Is there another monitor at this physical point?
pub fn monitor_at(x: f32, y: f32) -> bool {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONULL};
    let m = unsafe { MonitorFromPoint(POINT { x: x as i32, y: y as i32 }, MONITOR_DEFAULTTONULL) };
    !m.is_null()
}

/// A top-level window of ours by its exact title.
pub fn find_window(title: &str) -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
    let w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let h = unsafe { FindWindowW(std::ptr::null(), w.as_ptr()) };
    (!h.is_null()).then_some(h as isize)
}

/// Is Starlite the app in front? True when the foreground window belongs to
/// this process (the main window with its YouTube player, file dialogs),
/// except `ignore` (the mini player itself, which doesn't count as "in").
pub fn app_in_front(ignore: Option<isize>) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_null() || ignore == Some(fg as isize) {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(fg, &mut pid) };
    pid == std::process::id()
}

/// Is the Windows taskbar light (Settings → Personalization → Colors)?
pub fn taskbar_is_light() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let key: Vec<u16> = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize".encode_utf16().chain(Some(0)).collect();
    let name: Vec<u16> = "SystemUsesLightTheme".encode_utf16().chain(Some(0)).collect();
    let mut v: u32 = 0;
    let mut size = 4u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut v as *mut u32 as *mut _, &mut size) };
    r == 0 && v == 1
}

/// Windows' own blur of what's behind the window (Acrylic), or none. The
/// window is see-through where Starlite draws it so.
pub fn set_backdrop(hwnd: isize, on: bool, dark: bool) {
    use windows_sys::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE};
    use windows_sys::Win32::UI::Controls::MARGINS;
    let h = hwnd as HWND;
    // 3: the transient-window backdrop (Acrylic), 1: none.
    let kind: i32 = if on { 3 } else { 1 };
    let dark: i32 = dark as i32;
    // The blur needs the window's frame stretched over all of it.
    let m = if on { -1 } else { 0 };
    let margins = MARGINS { cxLeftWidth: m, cxRightWidth: m, cyTopHeight: m, cyBottomHeight: m };
    unsafe {
        DwmSetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &dark as *const _ as *const core::ffi::c_void, 4);
        DwmExtendFrameIntoClientArea(h, &margins);
        DwmSetWindowAttribute(h, DWMWA_SYSTEMBACKDROP_TYPE as u32, &kind as *const _ as *const core::ffi::c_void, 4);
    }
    keep_own_buttons(hwnd, on);
}

/// With the frame stretched over the window, Windows would draw its own
/// minimise / maximise / close buttons over Starlite's. They come from the
/// window's system menu style, so that goes while the blur is on (and comes
/// back after). Cheap to call every frame: it only changes something when needed.
pub fn keep_own_buttons(hwnd: isize, blur_on: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_SYSMENU,
    };
    let h = hwnd as HWND;
    unsafe {
        let style = GetWindowLongPtrW(h, GWL_STYLE);
        let has = style & WS_SYSMENU as isize != 0;
        if has == !blur_on {
            return;
        }
        let new = if blur_on { style & !(WS_SYSMENU as isize) } else { style | WS_SYSMENU as isize };
        SetWindowLongPtrW(h, GWL_STYLE, new);
        SetWindowPos(h, std::ptr::null_mut(), 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// The display's refresh rate (Hz), from the compositor; 60 if unknown.
pub fn refresh_hz() -> f64 {
    use windows_sys::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
    let mut info: DWM_TIMING_INFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<DWM_TIMING_INFO>() as u32;
    let ok = unsafe { DwmGetCompositionTimingInfo(std::ptr::null_mut(), &mut info) } == 0;
    let (n, d) = (info.rateRefresh.uiNumerator, info.rateRefresh.uiDenominator);
    if ok && n > 0 && d > 0 {
        n as f64 / d as f64
    } else {
        60.0
    }
}

/// How many screen refreshes make up (about) this frame interval.
pub fn refreshes_for(interval: std::time::Duration) -> u32 {
    (refresh_hz() * interval.as_secs_f64()).round().max(1.0) as u32
}

/// Blocks until just after the screen's next refresh. Uses the compositor's
/// timing (when the last refresh was, how long one lasts), so it doesn't
/// depend on anything on screen changing; DwmFlush when that isn't known.
/// False if neither works.
pub fn wait_for_refresh() -> bool {
    use windows_sys::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
    use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let mut info: DWM_TIMING_INFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<DWM_TIMING_INFO>() as u32;
    let (mut now, mut freq) = (0i64, 0i64);
    let ok = unsafe { DwmGetCompositionTimingInfo(std::ptr::null_mut(), &mut info) == 0 && QueryPerformanceCounter(&mut now) != 0 && QueryPerformanceFrequency(&mut freq) != 0 };
    let (period, last) = (info.qpcRefreshPeriod as i64, info.qpcVBlank as i64);
    if ok && period > 0 && freq > 0 && last > 0 {
        let next = last + ((now - last).div_euclid(period) + 1) * period;
        // A little after it, once the compositor has picked up the last frame.
        let wait = (next + freq / 2000 - now) as f64 / freq as f64;
        if wait > 0.0 && wait < 0.1 {
            std::thread::sleep(std::time::Duration::from_secs_f64(wait));
        }
        return true;
    }
    unsafe { windows_sys::Win32::Graphics::Dwm::DwmFlush() == 0 }
}

/// Native rounded corners and a border that blends with the current theme.
pub fn style_window(hwnd: isize, light: bool) {
    let h = hwnd as HWND;
    unsafe {
        let corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(
            h,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corner as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&corner) as u32,
        );
        // COLORREF is 0x00BBGGRR.
        let border: u32 = if light { 0x00D0_CFCF } else { 0x0030_2C2C };
        DwmSetWindowAttribute(
            h,
            DWMWA_BORDER_COLOR as u32,
            &border as *const _ as *const core::ffi::c_void,
            4,
        );
    }
}

/// Windows' list of installed fonts (for everyone and for this user):
/// pairs like ("Arial Bold (TrueType)", "arialbd.ttf").
pub fn font_table() -> Vec<(String, String)> {
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ};
    let sub: Vec<u16> = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts".encode_utf16().chain(Some(0)).collect();
    let mut out = Vec::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(root, sub.as_ptr(), 0, KEY_READ, &mut key) != 0 {
                continue;
            }
            for i in 0..20_000u32 {
                let mut name = [0u16; 512];
                let mut data = [0u16; 1024];
                let (mut name_len, mut data_len, mut ty) = (name.len() as u32, (data.len() * 2) as u32, 0u32);
                let r = RegEnumValueW(key, i, name.as_mut_ptr(), &mut name_len, std::ptr::null(), &mut ty, data.as_mut_ptr() as *mut u8, &mut data_len);
                // 259: no more values. Anything else odd (a very long entry): skip it.
                if r == 259 {
                    break;
                }
                if r != 0 || ty != REG_SZ {
                    continue;
                }
                let n = String::from_utf16_lossy(&name[..(name_len as usize).min(name.len())]);
                let d = String::from_utf16_lossy(&data[..(data_len as usize / 2).min(data.len())]);
                out.push((n, d.trim_end_matches('\0').to_string()));
            }
            RegCloseKey(key);
        }
    }
    out
}

/// Linux learns these from egui each frame; Windows asks the system itself.
pub fn note_focus(_focused: bool) {}
pub fn note_monitor(_w: f32, _h: f32) {}
pub fn note_mini_hover(_over: bool) {}
pub fn note_cursor(_x: f32, _y: f32) {}

/// Opens a folder in Explorer, or an https link in the default browser.
pub fn open(target: &std::ffi::OsStr) {
    if target.to_string_lossy().starts_with("https://") {
        let _ = std::process::Command::new("rundll32").arg("url.dll,FileProtocolHandler").arg(target).spawn();
    } else {
        let _ = std::process::Command::new("explorer").arg(target).spawn();
    }
}

/// Opens a text file in Notepad.
pub fn edit_text(path: &std::path::Path) -> bool {
    std::process::Command::new("notepad.exe").arg(path).spawn().is_ok()
}

/// The user's region (e.g. "DE") from the Windows locale, for local charts.
pub fn region() -> String {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    let name = String::from_utf16_lossy(&buf[..(n.max(1) as usize - 1)]);
    name.rsplit('-').next().filter(|r| r.len() == 2 && r.chars().all(|c| c.is_ascii_uppercase())).unwrap_or("US").to_string()
}

/// Segoe UI is Windows' own (picked by file name in app.rs).
pub const UI_FONTS: &[&str] = &[];
