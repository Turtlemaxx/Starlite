//! Fullscreen Now Playing: big cover, title / artist / album, and lyrics that
//! follow the song when they're timed (LRC). The background is drawn by the
//! shader from the cover's colours (see `glass.rs::album_bg`).

use std::time::{Duration, Instant};

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Id, Rect, Sense, Ui, Vec2};

use crate::app::Starlite;
use crate::glass::El;
use crate::icons::Icons;
use crate::meta::Lyrics;
use crate::widgets::{cover, font, text_trunc};

/// Average colours of the cover's four quadrants, saturated a bit: the
/// palette for the fullscreen background. Runs on a worker thread.
pub fn palette_from(bytes: &[u8]) -> Option<[[f32; 3]; 4]> {
    let mut img = image::load_from_memory(bytes).ok()?.to_rgb8();
    // Letterboxed thumbnails (YouTube's 4:3 ones): drop the black bars.
    let (w, h) = img.dimensions();
    if h > 8 && (w as f32 / h as f32 - 4.0 / 3.0).abs() < 0.02 {
        let dark = |y: u32| (0..w).step_by(8).all(|x| img.get_pixel(x, y).0.iter().all(|&c| c < 24));
        if dark(2) && dark(h - 3) {
            img = image::imageops::crop_imm(&img, 0, h / 8, w, h * 3 / 4).to_image();
        }
    }
    let small = image::imageops::thumbnail(&img, 16, 16);
    let mut out = [[0f32; 3]; 4];
    for (qi, (qx, qy)) in [(0u32, 0u32), (1, 0), (0, 1), (1, 1)].iter().enumerate() {
        let mut acc = [0f32; 3];
        let mut n = 0.0;
        for y in qy * 8..qy * 8 + 8 {
            for x in qx * 8..qx * 8 + 8 {
                let p = small.get_pixel(x, y);
                for c in 0..3 {
                    acc[c] += p[c] as f32 / 255.0;
                }
                n += 1.0;
            }
        }
        let mut c = [acc[0] / n, acc[1] / n, acc[2] / n];
        // Push away from grey so the background has colour.
        let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        for v in &mut c {
            *v = (l + (*v - l) * 1.5).clamp(0.0, 1.0);
        }
        // Keep very dark covers from turning the screen black.
        let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        if l < 0.18 {
            for v in &mut c {
                *v = (*v + (0.18 - l) * 0.8).min(1.0);
            }
        }
        out[qi] = c;
    }
    Some(out)
}

/// Bytes of a cover URI (file:// or http(s)://).
pub fn cover_bytes(uri: &str) -> Option<Vec<u8>> {
    let resolved = crate::navidrome::resolve_cover(uri);
    let uri = resolved.as_deref().unwrap_or(uri);
    if uri.starts_with(crate::navidrome::COVER_SCHEME) {
        return None;
    }
    if let Some(path) = uri.strip_prefix("file://") {
        return std::fs::read(path).ok();
    }
    let resp = ureq::get(uri).timeout(std::time::Duration::from_secs(10)).call().ok()?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(resp.into_reader(), 8 << 20), &mut buf).ok()?;
    Some(buf)
}

impl Starlite {
    /// Draws the view; returns where the YouTube video should sit (if any).
    #[allow(clippy::too_many_arguments)]
    pub fn now_playing_view(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, dt: f32, area: Rect, alpha: f32, animating: &mut bool) -> Option<Rect> {
        let _ = (els, ppp);
        let Some(track) = self.current_track() else { return None };
        let white = Color32::WHITE.gamma_multiply(alpha);
        let dim = Color32::from_white_alpha(170).gamma_multiply(alpha);
        let faint = Color32::from_white_alpha(110).gamma_multiply(alpha);
        let p = ui.painter().clone();

        let lyrics: Option<Lyrics> = self.lyrics.as_ref().filter(|(id, _)| *id == track.id).and_then(|(_, l)| l.clone());
        let is_video = track.youtube_id().is_some();
        let two_col = lyrics.is_some();

        // Left: cover (or the video) + text.
        let col = if two_col { Rect::from_min_max(area.min, pos2(area.left() + area.width() * 0.46, area.bottom())) } else { area };
        let mut video_rect = None;
        let (art, text_top) = if is_video {
            let w = (col.width() - 80.0).min((area.height() - 160.0) * 16.0 / 9.0).max(356.0);
            let h = (w * 9.0 / 16.0).max(200.0);
            let r = Rect::from_center_size(pos2(col.center().x, area.top() + (area.height() - 120.0) * 0.5), vec2(w, h));
            video_rect = Some(r);
            (r, r.bottom() + 28.0)
        } else {
            let size = (col.width() - 120.0).min(area.height() - 170.0).clamp(160.0, 520.0);
            let r = Rect::from_center_size(pos2(col.center().x, area.top() + (area.height() - 120.0) * 0.5), Vec2::splat(size));
            let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(area));
            cu.set_opacity(alpha);
            // Soft shadow under the cover.
            cu.painter().add(egui::epaint::Shadow { offset: [0, 18], blur: 48, spread: 0, color: Color32::from_black_alpha(110) }.as_shape(r, egui::CornerRadius::same(22)));
            cover(&cu, icons, r, track.artwork.as_deref(), 22.0, "disc-3", self.theme);
            (r, r.bottom() + 30.0)
        };
        let tw = art.width().max(320.0);
        text_trunc(&p, pos2(art.center().x, text_top + 6.0), Align2::CENTER_CENTER, &track.title, font(26.0, "bold"), white, tw);
        let sub = match track.source_name() {
            Some(src) => format!("{} · {src}", track.artist),
            None => track.artist.clone(),
        };
        text_trunc(&p, pos2(art.center().x, text_top + 38.0), Align2::CENTER_CENTER, &sub, font(16.0, "semibold"), dim, tw);
        if let Some(album) = &track.album {
            text_trunc(&p, pos2(art.center().x, text_top + 62.0), Align2::CENTER_CENTER, album, font(13.5, "regular"), faint, tw);
        }
        if !two_col && !is_video {
            let looked = self.lyrics.as_ref().is_some_and(|(id, _)| *id == track.id);
            let note = match (looked, self.lprefs.online) {
                (true, true) => "No lyrics found",
                (true, false) => "No lyrics in this song's tags",
                (false, true) => "Looking for lyrics…",
                (false, false) => "",
            };
            if !note.is_empty() {
                p.text(pos2(art.center().x, text_top + 92.0), Align2::CENTER_CENTER, note, font(12.5, "regular"), faint);
            }
        }

        // Right: lyrics.
        if let Some(ly) = lyrics {
            let lr = Rect::from_min_max(pos2(col.right() + 20.0, area.top() + 20.0), pos2(area.right() - 40.0, area.bottom() - 20.0));
            self.draw_lyrics(ui, lr, &ly, dt, alpha, animating);
        }
        video_rect
    }

    fn draw_lyrics(&mut self, ui: &mut Ui, r: Rect, ly: &Lyrics, dt: f32, alpha: f32, animating: &mut bool) {
        let (pos, _) = self.playback_position();
        // A little ahead, so the glide lands as the line is sung.
        let pos_ms = (pos * 1000.0) as u64 + 200;
        let current = if ly.synced { ly.lines.iter().rposition(|l| l.at_ms.is_some_and(|t| t <= pos_ms)) } else { None };
        let now = Instant::now();
        let dt = dt.clamp(0.0, 0.05);

        // Lay out every line (wrapped) to know heights.
        let f = font(27.0, "bold");
        let gap = 20.0;
        let galleys: Vec<_> = ly
            .lines
            .iter()
            .map(|l| {
                let text = if l.text.is_empty() { "♪".to_string() } else { l.text.clone() };
                ui.painter().layout(text, f.clone(), Color32::WHITE, r.width())
            })
            .collect();
        let mut tops = Vec::with_capacity(galleys.len());
        let mut y = 0.0;
        for g in &galleys {
            tops.push(y);
            y += g.size().y + gap;
        }
        let total = y;

        // Synced: the current line sits at ~38% height; each line glides there
        // on its own spring, the ones further down a beat later, so the text
        // flows up like a wave. The wheel lets you look around for a moment.
        // Plain: the wheel scrolls.
        let st = &mut self.lyric_scroll;
        if st.lines.len() != galleys.len() {
            st.lines = vec![(f32::NAN, 0.0); galleys.len()];
        }
        let wheel = if ui.rect_contains_pointer(r) { ui.ctx().input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
        let target = if ly.synced {
            let anchor = r.height() * 0.38;
            let base = current.map(|i| tops[i] + galleys[i].size().y * 0.5 - anchor).unwrap_or(-anchor + 20.0);
            if wheel != 0.0 {
                st.user -= wheel;
                st.user_until = Some(now + Duration::from_millis(2500));
            }
            if st.user_until.is_some_and(|t| now >= t) {
                st.user = 0.0;
                st.user_until = None;
            }
            st.user = st.user.clamp(-anchor - base, (total - anchor - base).max(-anchor - base));
            base + st.user
        } else {
            st.plain = (st.plain - wheel).clamp(0.0, (total - r.height() * 0.6).max(0.0));
            st.plain
        };
        let browsing = st.user_until.is_some();
        let c = current.unwrap_or(0);
        for (i, (s, v)) in st.lines.iter_mut().enumerate() {
            if !ly.synced || browsing || s.is_nan() || (*s - target).abs() > r.height() * 1.5 {
                // Plain lyrics, the wheel (already smooth), a fresh song or a seek far away: no glide.
                *s = target;
                *v = 0.0;
                continue;
            }
            let lag = i.saturating_sub(c).min(7) as f32;
            *s = smooth_damp(*s, target, v, 0.34 + 0.045 * lag, dt);
            if (*s - target).abs() < 0.2 && v.abs() < 1.0 {
                *s = target;
                *v = 0.0;
            } else {
                *animating = true;
            }
        }
        let shown: Vec<f32> = st.lines.iter().map(|l| l.0).collect();

        let p = ui.painter().with_clip_rect(r);
        let fade = 90.0;
        let mut seek_to = None;
        for (i, g) in galleys.iter().enumerate() {
            let top = r.top() + tops[i] - shown[i];
            let h = g.size().y;
            if top + h < r.top() || top > r.bottom() {
                continue;
            }
            let target = if !ly.synced {
                0.9
            } else if Some(i) == current {
                1.0
            } else if browsing {
                0.6
            } else if current.is_some_and(|c| i < c) {
                0.3
            } else {
                0.42
            };
            let base = ui.ctx().animate_value_with_time(Id::new(("lyric_a", i)), target, 0.35);
            if (base - target).abs() > 0.002 {
                *animating = true;
            }
            // Fade out towards the top and bottom edges.
            let edge = ((top + h * 0.5 - r.top()) / fade).min((r.bottom() - top - h * 0.5) / fade).clamp(0.0, 1.0);
            let c = Color32::WHITE.gamma_multiply(base * edge * alpha);
            let rect = Rect::from_min_size(pos2(r.left(), top), g.size());
            if ly.synced {
                if let Some(at) = ly.lines[i].at_ms {
                    let resp = ui.interact(rect.intersect(r), Id::new(("lyric", i)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
                    if resp.clicked() {
                        seek_to = Some(at as f64 / 1000.0);
                    }
                }
            }
            p.galley_with_override_text_color(rect.min, g.clone(), c);
        }
        if let Some(s) = seek_to {
            self.lyric_scroll.user = 0.0;
            self.lyric_scroll.user_until = None;
            self.seek_to(s);
        }
    }
}

/// Where the lyrics are scrolled to (per line, for the wave).
#[derive(Default)]
pub struct LyricScroll {
    /// Each line's shown scroll and its velocity.
    lines: Vec<(f32, f32)>,
    /// Wheel offset while you look around timed lyrics, until `user_until`.
    user: f32,
    user_until: Option<Instant>,
    /// Scroll of untimed lyrics.
    plain: f32,
}

/// Critically damped spring towards `target` (eases in and out, no
/// overshoot); `smooth` is roughly the time to get there.
fn smooth_damp(cur: f32, target: f32, vel: &mut f32, smooth: f32, dt: f32) -> f32 {
    let omega = 2.0 / smooth.max(1e-4);
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = cur - target;
    let temp = (*vel + omega * change) * dt;
    *vel = (*vel - omega * temp) * exp;
    target + (change + temp) * exp
}

#[cfg(test)]
mod tests {
    use super::smooth_damp;

    #[test]
    fn glide_settles_without_overshoot() {
        let (mut s, mut v) = (0.0f32, 0.0f32);
        let mut prev = s;
        for frame in 0..90 {
            s = smooth_damp(s, 100.0, &mut v, 0.34, 1.0 / 60.0);
            assert!(s >= prev && s <= 100.0 + 1e-3, "frame {frame}: {s}");
            prev = s;
        }
        assert!((s - 100.0).abs() < 0.5, "settled by 1.5 s: {s}");
    }

    #[test]
    fn lower_lines_lag_behind() {
        let (mut a, mut va, mut b, mut vb) = (0.0f32, 0.0, 0.0f32, 0.0);
        for _ in 0..12 {
            a = smooth_damp(a, 100.0, &mut va, 0.34, 1.0 / 60.0);
            b = smooth_damp(b, 100.0, &mut vb, 0.34 + 0.045 * 3.0, 1.0 / 60.0);
        }
        assert!(a > b + 5.0, "{a} vs {b}");
    }
}
