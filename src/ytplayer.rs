//! YouTube's official embedded player (IFrame Player API) in a WebView2
//! child window. Starlite serves a tiny page from its own `starlite://`
//! scheme (seen by the web view as `https://starlite.player`), so YouTube
//! gets a real origin, as its embed rules require.
//!
//! Rules this follows: the player stays visible (at least 200×200) while it
//! plays, YouTube's ads and branding are untouched, and the audio is never
//! separated from the video or saved.

use std::borrow::Cow;
use std::num::NonZeroIsize;
use std::sync::{Arc, Mutex};

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use serde_json::Value;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::http::{header::CONTENT_TYPE, Response};
use wry::{WebView, WebViewBuilder};

use crate::audio::Status;

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8">
<meta name="referrer" content="strict-origin-when-cross-origin">
<style>html,body{margin:0;height:100%;background:#000;overflow:hidden}#p{position:absolute;inset:0}</style>
</head><body><div id="p"></div>
<script>
var player = null, ready = false, pending = null, rate = 1;
function send(o) { try { window.ipc.postMessage(JSON.stringify(o)); } catch (e) {} }
function onYouTubeIframeAPIReady() {
  player = new YT.Player('p', {
    width: '100%', height: '100%',
    playerVars: { playsinline: 1, rel: 0, autoplay: 1, origin: location.origin },
    events: {
      onReady: function () { ready = true; send({ ev: 'ready' }); if (pending) { var p = pending; pending = null; load(p.id, p.vol); } },
      onStateChange: function (e) {
        // A new video starts at normal speed: keep the chosen one.
        if (e.data === 1 && player.getPlaybackRate && player.getPlaybackRate() !== rate) player.setPlaybackRate(rate);
        send({ ev: 'state', s: e.data }); report();
      },
      onError: function (e) { send({ ev: 'error', code: e.data }); }
    }
  });
}
function report() {
  if (!ready || !player.getCurrentTime) return;
  send({ ev: 'time', t: player.getCurrentTime() || 0, d: player.getDuration() || 0 });
}
setInterval(function () { if (ready && player.getPlayerState && player.getPlayerState() === 1) report(); }, 500);
function load(id, vol) {
  if (!ready) { pending = { id: id, vol: vol }; return; }
  player.setVolume(vol); player.unMute(); player.loadVideoById(id);
}
function ctl(c, v) {
  if (!ready) return;
  if (c === 'play') player.playVideo();
  else if (c === 'pause') player.pauseVideo();
  else if (c === 'stop') player.stopVideo();
  else if (c === 'vol') player.setVolume(v);
  else if (c === 'rate') { rate = v; player.setPlaybackRate(v); }
  else if (c === 'seek') { player.seekTo(v, true); report(); }
}
</script>
<script src="https://www.youtube.com/iframe_api"></script>
</body></html>"#;

/// A borrowed handle to the main window for `build_as_child`.
struct Parent(isize);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let h = Win32WindowHandle::new(NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(h)) })
    }
}

pub struct YtPlayer {
    webview: WebView,
    events: Arc<Mutex<Vec<Value>>>,
    pub status: Status,
    pos: f64,
    dur: f64,
    pos_at: std::time::Instant,
    speed: f32,
    /// Bumped when a video plays to its end.
    pub finished: u64,
    bounds: Option<egui::Rect>,
    visible: bool,
}

/// What happened in the player since the last frame.
pub enum YtEvent {
    Ended,
    /// The reason is shown through `status`.
    Error,
}

impl YtPlayer {
    pub fn new(hwnd: isize, ctx: egui::Context) -> Result<YtPlayer, String> {
        let events: Arc<Mutex<Vec<Value>>> = Arc::default();
        let ev = events.clone();
        let parent = Parent(hwnd);
        let data = crate::library::data_dir().join("webview");
        let _ = std::fs::create_dir_all(&data);
        // Keep the web view's profile (cookies, YouTube sign-in) with Starlite's data.
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &data);

        #[cfg(windows)]
        use wry::WebViewBuilderExtWindows;
        let webview = WebViewBuilder::new()
            .with_https_scheme(true)
            .with_custom_protocol("starlite".into(), |_id, _req| {
                Response::builder()
                    .header(CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(Cow::Borrowed(PAGE.as_bytes()))
                    .unwrap()
            })
            .with_url("https://starlite.player/")
            .with_autoplay(true)
            .with_visible(false)
            .with_background_color((0, 0, 0, 255))
            .with_bounds(wry::Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(356.0, 200.0).into() })
            .with_ipc_handler(move |req| {
                if let Ok(v) = serde_json::from_str::<Value>(req.body()) {
                    ev.lock().unwrap().push(v);
                    ctx.request_repaint();
                }
            })
            // Links inside the player (YouTube logo, title) open in the real browser.
            // Only https links: anything else (file:, other protocols) is ignored.
            .with_new_window_req_handler(|url, _features| {
                if url.starts_with("https://") && !url.contains(char::is_whitespace) {
                    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &url]).spawn();
                }
                wry::NewWindowResponse::Deny
            })
            .build_as_child(&parent)
            .map_err(|e| format!("Couldn't start the YouTube player (WebView2): {e}"))?;
        // WebView2 takes keyboard focus when it's created; give it back.
        let _ = webview.focus_parent();
        Ok(YtPlayer { webview, events, status: Status::Idle, pos: 0.0, dur: 0.0, pos_at: std::time::Instant::now(), speed: 1.0, finished: 0, bounds: None, visible: false })
    }

    pub fn load(&mut self, video_id: &str, volume: f32) {
        let id = video_id.replace(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'), "");
        let _ = self.webview.evaluate_script(&format!("load('{id}', {})", (volume * 100.0).round() as i32));
        self.pos = 0.0;
        self.dur = 0.0;
        self.status = Status::Loading;
    }

    /// Return keyboard focus to Starlite (the player keeps it otherwise).
    pub fn focus_app(&self) {
        let _ = self.webview.focus_parent();
    }

    pub fn play(&self) {
        let _ = self.webview.evaluate_script("ctl('play')");
    }
    pub fn pause(&self) {
        let _ = self.webview.evaluate_script("ctl('pause')");
    }
    pub fn stop(&mut self) {
        let _ = self.webview.evaluate_script("ctl('stop')");
        self.status = Status::Idle;
    }
    pub fn seek(&mut self, secs: f64) {
        let _ = self.webview.evaluate_script(&format!("ctl('seek', {secs:.2})"));
        self.pos = secs;
        self.pos_at = std::time::Instant::now();
    }

    /// (position, length) in seconds, interpolated between reports.
    pub fn position(&self) -> (f64, f64) {
        let mut p = self.pos;
        if self.status == Status::Playing {
            p += self.pos_at.elapsed().as_secs_f64().min(1.0) * self.speed as f64;
        }
        (if self.dur > 0.0 { p.min(self.dur) } else { p }, self.dur)
    }

    /// Playback speed (YouTube rounds it to its own steps: 0.25 … 2).
    pub fn set_speed(&mut self, v: f32) {
        self.speed = v;
        let _ = self.webview.evaluate_script(&format!("ctl('rate', {v:.2})"));
    }

    pub fn set_volume(&self, v: f32) {
        let _ = self.webview.evaluate_script(&format!("ctl('vol', {})", (v * 100.0).round() as i32));
    }

    /// Place (or hide) the player. `rect` is in egui points = logical pixels.
    pub fn place(&mut self, rect: Option<egui::Rect>) {
        match rect {
            Some(r) => {
                if self.bounds != Some(r) {
                    let _ = self.webview.set_bounds(wry::Rect {
                        position: LogicalPosition::new(r.left() as f64, r.top() as f64).into(),
                        size: LogicalSize::new(r.width() as f64, r.height() as f64).into(),
                    });
                    self.bounds = Some(r);
                }
                if !self.visible {
                    let _ = self.webview.set_visible(true);
                    self.visible = true;
                }
            }
            None => {
                if self.visible {
                    let _ = self.webview.set_visible(false);
                    self.visible = false;
                }
            }
        }
    }

    /// Apply player events to `status`; returns what the app must react to.
    pub fn poll(&mut self) -> Vec<YtEvent> {
        let mut out = Vec::new();
        for v in std::mem::take(&mut *self.events.lock().unwrap()) {
            match v["ev"].as_str() {
                Some("state") => match v["s"].as_i64() {
                    Some(0) => {
                        self.status = Status::Idle;
                        self.finished += 1;
                        out.push(YtEvent::Ended);
                    }
                    Some(1) => self.status = Status::Playing,
                    Some(2) => self.status = Status::Paused,
                    Some(3) | Some(-1) if self.status != Status::Paused => self.status = Status::Loading,
                    _ => {}
                },
                Some("time") => {
                    self.pos = v["t"].as_f64().unwrap_or(0.0);
                    self.dur = v["d"].as_f64().unwrap_or(self.dur);
                    self.pos_at = std::time::Instant::now();
                }
                Some("error") => {
                    let msg = match v["code"].as_i64() {
                        Some(101) | Some(150) | Some(153) => "This video's owner doesn't allow it to play outside YouTube",
                        Some(100) => "This video was removed or is private",
                        Some(5) => "This video can't be played here",
                        _ => "YouTube couldn't play this video",
                    };
                    self.status = Status::Error(msg.into());
                    out.push(YtEvent::Error);
                }
                _ => {}
            }
        }
        out
    }
}
