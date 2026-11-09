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

/// Blocks until the compositor's next refresh. False if it can't.
pub fn wait_for_refresh() -> bool {
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
