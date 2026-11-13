//! Small drawing helpers shared by the pages: truncated text, cover art,
//! glass pills and glass text fields.

use egui::load::{SizeHint, TexturePoll};
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Id, Painter, Pos2, Rect, Response,
    Sense, Stroke, TextFormat, TextureOptions, Ui, Vec2,
};

use crate::glass::{El, Kind};
use crate::icons::{known, Icons};
use crate::library::Cover;
use crate::theme::Theme;

pub fn font(size: f32, family: &str) -> FontId {
    if family == "regular" {
        FontId::proportional(size)
    } else {
        FontId::new(size, FontFamily::Name(family.into()))
    }
}

/// Single-line text, truncated with "…" to `max_w`. Returns its rect.
pub fn text_trunc(p: &Painter, pos: Pos2, align: Align2, text: &str, font: FontId, color: Color32, max_w: f32) -> Rect {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap = TextWrapping::truncate_at_width(max_w.max(1.0));
    let g = p.layout_job(job);
    let rect = align.anchor_size(pos, g.size());
    p.galley(rect.min, g, color);
    rect
}

fn placeholder(ui: &Ui, icons: &mut Icons, rect: Rect, radius: f32, icon: &'static str, theme: Theme) {
    let fill = if theme.is_light() { Color32::from_black_alpha(14) } else { Color32::from_white_alpha(14) };
    ui.painter().rect_filled(rect, CornerRadius::same(radius.min(255.0) as u8), fill);
    icons.paint(ui, icon, rect.center(), (rect.width() * 0.42).min(44.0), theme.text_dim());
}

/// Cover art from a URL (http(s) or file://) with rounded corners; shows a
/// placeholder icon while loading or if it fails.
pub fn cover(ui: &Ui, icons: &mut Icons, rect: Rect, uri: Option<&str>, radius: f32, fallback: &'static str, theme: Theme) {
    // Server covers are stored without the login; add it only here.
    let uri = uri.and_then(crate::stream::cover_link);
    if let Some(uri) = uri.as_deref() {
        let poll = ui.ctx().try_load_texture(uri, TextureOptions::LINEAR, SizeHint::Scale(1.0.into()));
        if let Ok(TexturePoll::Ready { texture }) = poll {
            let uv = cover_uv(uri, texture.size, rect.size());
            egui::Image::from_texture(texture).uv(uv).corner_radius(radius.min(255.0) as u8).paint_at(ui, rect);
            return;
        }
    }
    placeholder(ui, icons, rect, radius, fallback, theme);
}

/// A cover's texture once loaded, with the crop that fills `aspect`.
pub fn cover_texture(ctx: &egui::Context, uri: Option<&str>, aspect: Vec2) -> Option<(egui::TextureId, Rect)> {
    let uri = uri?;
    let resolved = crate::stream::cover_link(uri)?;
    match ctx.try_load_texture(&resolved, TextureOptions::LINEAR, SizeHint::Scale(1.0.into())) {
        Ok(TexturePoll::Ready { texture }) => Some((texture.id, cover_uv(&resolved, texture.size, aspect))),
        _ => None,
    }
}

/// The part of an image to show so it fills `dst` without stretching
/// (centre crop). YouTube's 4:3 thumbnails carry black bars above and below
/// a 16:9 picture, so those are trimmed first.
pub fn cover_uv(uri: &str, img: Vec2, dst: Vec2) -> Rect {
    let mut uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    if img.x <= 0.0 || img.y <= 0.0 || dst.x <= 0.0 || dst.y <= 0.0 {
        return uv;
    }
    let w = img.x;
    let mut h = img.y;
    if uri.contains("ytimg.com") && (img.x / img.y - 4.0 / 3.0).abs() < 0.02 {
        uv.min.y = 0.125;
        uv.max.y = 0.875;
        h *= 0.75;
    }
    let want = dst.x / dst.y;
    let have = w / h;
    if have > want {
        // Too wide: trim the sides.
        let keep = want / have;
        let m = (1.0 - keep) * 0.5 * uv.width();
        uv.min.x += m;
        uv.max.x -= m;
    } else if have < want {
        let keep = have / want;
        let m = (1.0 - keep) * 0.5 * uv.height();
        uv.min.y += m;
        uv.max.y -= m;
    }
    uv
}

pub fn preset_icon(name: &str) -> &'static str {
    known(name).unwrap_or("disc-3")
}

pub fn file_uri(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

/// A playlist cover: either a picked image or a Lucide preset icon.
pub fn playlist_cover(ui: &Ui, icons: &mut Icons, rect: Rect, c: &Cover, radius: f32, theme: Theme) {
    match c {
        Cover::Image(path) => cover(ui, icons, rect, Some(&file_uri(path)), radius, "disc-3", theme),
        Cover::Preset(name) => {
            let fill = if theme.is_light() { Color32::from_black_alpha(16) } else { Color32::from_white_alpha(18) };
            ui.painter().rect_filled(rect, CornerRadius::same(radius.min(255.0) as u8), fill);
            icons.paint(ui, preset_icon(name), rect.center(), (rect.width() * 0.5).round(), theme.accent());
        }
    }
}

/// Glass pill button. `on_glass` = it sits on another glass panel.
#[allow(clippy::too_many_arguments)]
pub fn pill(
    ui: &Ui,
    icons: &mut Icons,
    els: &mut Vec<El>,
    ppp: f32,
    id: Id,
    rect: Rect,
    label: &str,
    icon: Option<&'static str>,
    emphasis: bool,
    on_glass: bool,
    theme: Theme,
) -> Response {
    let resp = ui.interact(rect, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let ctx = ui.ctx();
    let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.16);
    let pr = ctx.animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), 0.09);
    let r = Rect::from_center_size(rect.center(), rect.size() + Vec2::splat(5.0 * pr));
    let kind = if on_glass { Kind::Layered } else { Kind::Glass };
    let glow = if emphasis { 0.55 } else { 0.0 } + 0.45 * h;
    els.push(El::rect(r, ppp, r.height() * 0.5, 1.0, 4.0 + 3.0 * h, kind, glow, pr));
    let f = font(13.5, "semibold");
    let p = ui.painter();
    let g = p.layout_no_wrap(label.to_owned(), f.clone(), theme.text());
    let total = g.size().x + if icon.is_some() { 24.0 } else { 0.0 };
    let mut x = rect.center().x - total * 0.5;
    if let Some(ic) = icon {
        icons.paint(ui, ic, pos2(x + 8.0, rect.center().y), 16.0, theme.text());
        x += 24.0;
    }
    p.galley(pos2(x, rect.center().y - g.size().y * 0.5), g, theme.text());
    resp
}

/// Glass text field. Returns the TextEdit response.
#[allow(clippy::too_many_arguments)]
pub fn field(
    ui: &mut Ui,
    els: &mut Vec<El>,
    ppp: f32,
    rect: Rect,
    value: &mut String,
    hint: &str,
    password: bool,
    on_glass: bool,
    theme: Theme,
) -> Response {
    let kind = if on_glass { Kind::Layered } else { Kind::Glass };
    els.push(El::rect(rect, ppp, 12.0, 1.0, 2.0, kind, 0.0, 0.0));
    let inner = rect.shrink2(vec2(14.0, 0.0));
    ui.put(
        inner,
        egui::TextEdit::singleline(value)
            .frame(false)
            .password(password)
            .font(font(14.0, "regular"))
            .text_color(theme.text())
            .hint_text(egui::RichText::new(hint).color(theme.text_dim()).font(font(14.0, "regular")))
            .vertical_align(egui::Align::Center)
            .margin(egui::Margin::ZERO)
            .desired_width(inner.width()),
    )
}

/// Glass icon button (circle). Returns the response.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    ui: &Ui,
    icons: &mut Icons,
    els: &mut Vec<El>,
    ppp: f32,
    id: Id,
    center: Pos2,
    d: f32,
    icon: &'static str,
    always: bool,
    on_glass: bool,
    theme: Theme,
) -> Response {
    let rect = Rect::from_center_size(center, Vec2::splat(d));
    let resp = ui.interact(rect, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let ctx = ui.ctx();
    let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.16);
    let pr = ctx.animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), 0.09);
    let s = if always { 1.0 } else { h };
    if s > 0.001 {
        let size = d * (0.84 + 0.16 * s) + 5.0 * pr;
        let kind = if on_glass { Kind::Layered } else { Kind::Glass };
        els.push(El::rect(Rect::from_center_size(center, Vec2::splat(size)), ppp, size * 0.5, s, 3.0, kind, 0.45 * h, pr));
    }
    icons.paint(ui, icon, center, (d * 0.45).round(), theme.text());
    resp
}

pub fn fmt_duration(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Songs whose title, artist or album contain every word of `query`
/// (any case); all of them when the query is empty. Indices into `tracks`.
pub fn filter_tracks(tracks: &[crate::library::Track], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            if words.is_empty() {
                return true;
            }
            let hay = format!("{} {} {}", t.title, t.artist, t.album.as_deref().unwrap_or("")).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|(i, _)| i)
        .collect()
}

/// A glass search field: magnifier, text, and an x to clear it. Ctrl+F
/// focuses it, Esc clears it.
#[allow(clippy::too_many_arguments)]
pub fn search_field(ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, id: Id, rect: Rect, value: &mut String, hint: &str, theme: Theme) {
    if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let focused = ui.memory(|m| m.has_focus(id));
    let lit = ui.ctx().animate_bool_with_time(id.with("lit"), focused || !value.is_empty(), 0.18);
    els.push(El::rect(rect, ppp, rect.height() * 0.5, 1.0, 3.0, Kind::Glass, 0.3 * lit, 0.0));
    let icon_c = pos2(rect.left() + 20.0, rect.center().y);
    let tint = if lit > 0.5 { theme.text() } else { theme.text_dim() };
    icons.paint(ui, "search", icon_c, 16.0, tint);
    let clear = !value.is_empty();
    let inner = Rect::from_min_max(pos2(rect.left() + 38.0, rect.top()), pos2(rect.right() - if clear { 40.0 } else { 16.0 }, rect.bottom()));
    let resp = ui.put(
        inner,
        egui::TextEdit::singleline(value)
            .id(id)
            .frame(false)
            .font(font(14.0, "regular"))
            .text_color(theme.text())
            .hint_text(egui::RichText::new(hint).color(theme.text_dim()).font(font(14.0, "regular")))
            .vertical_align(egui::Align::Center)
            .margin(egui::Margin::ZERO)
            .desired_width(inner.width()),
    );
    if (resp.has_focus() || resp.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        value.clear();
    }
    if clear {
        let c = pos2(rect.right() - 22.0, rect.center().y);
        let x = ui.interact(Rect::from_center_size(c, Vec2::splat(26.0)), id.with("clear"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Clear");
        let h = ui.ctx().animate_bool_with_time(id.with("xh"), x.hovered(), 0.12);
        if h > 0.0 {
            ui.painter().circle_filled(c, 12.0, if theme.is_light() { Color32::from_black_alpha((18.0 * h) as u8) } else { Color32::from_white_alpha((22.0 * h) as u8) });
        }
        icons.paint(ui, "x", c, 14.0, theme.text_dim());
        if x.clicked() {
            value.clear();
            ui.memory_mut(|m| m.request_focus(id));
        }
    }
}

#[cfg(test)]
mod search_tests {
    use super::filter_tracks;
    use crate::library::{Source, Track};

    fn t(title: &str, artist: &str, album: Option<&str>) -> Track {
        Track { id: title.into(), title: title.into(), artist: artist.into(), album: album.map(String::from), artwork: None, permalink: None, duration_ms: 0, source: Source::File { path: "x".into() } }
    }

    #[test]
    fn finds_by_title_artist_or_album_any_case() {
        let list = [t("All Falls Down", "Kanye West", Some("The College Dropout")), t("505", "Arctic Monkeys", Some("Favourite Worst Nightmare")), t("Heartless", "Kanye West", None)];
        assert_eq!(filter_tracks(&list, ""), vec![0, 1, 2]);
        assert_eq!(filter_tracks(&list, "kanye"), vec![0, 2]);
        assert_eq!(filter_tracks(&list, "  ARCTIC "), vec![1]);
        assert_eq!(filter_tracks(&list, "dropout"), vec![0]);
        assert_eq!(filter_tracks(&list, "kanye heart"), vec![2], "every word must match");
        assert!(filter_tracks(&list, "nothing like this").is_empty());
    }
}

/// What a line slider goes over.
pub struct Slide {
    pub min: f32,
    pub max: f32,
    /// The value the bold part of the line starts from (and a double-click goes back to).
    pub neutral: f32,
    pub step: f32,
    /// Faint marks every this much (0: none).
    pub marks: f32,
}

pub struct SlideOut {
    pub value: f32,
    pub changed: bool,
    /// The mouse let go (a good moment to save).
    pub released: bool,
    /// Hovered or dragged, 0..1 (eased).
    pub hot: f32,
}

/// A slider that is just a line: thin from end to end, bold between
/// "normal" and the value, with a short crossing line as the handle that
/// grows when you grab it. The drawn value glides to the real one.
/// `a` → `b` is the line from lowest to highest (any direction).
/// Double-click resets it.
#[allow(clippy::too_many_arguments)]
pub fn line_slider(ui: &Ui, id: Id, a: Pos2, b: Pos2, value: f32, s: &Slide, fg: Color32, dim: Color32, enabled: bool) -> SlideOut {
    let ctx = ui.ctx().clone();
    let axis = b - a;
    let len = axis.length().max(1.0);
    let (dir, across) = (axis / len, vec2(-axis.y, axis.x) / len);
    let hit = Rect::from_two_pos(a, b).expand(13.0);
    let resp = ui.interact(hit, id, if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let resp = if enabled { resp.on_hover_cursor(if axis.x.abs() > axis.y.abs() { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical }) } else { resp };
    let mut out = SlideOut { value, changed: false, released: false, hot: 0.0 };
    if enabled {
        if resp.double_clicked() {
            out.value = s.neutral;
            out.released = true;
        } else if let Some(pt) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked() || resp.drag_started()) {
            let t = ((pt - a).dot(dir) / len).clamp(0.0, 1.0);
            let raw = s.min + t * (s.max - s.min);
            out.value = ((raw / s.step).round() * s.step).clamp(s.min, s.max);
        }
        if resp.drag_stopped() || resp.clicked() {
            out.released = true;
        }
        out.changed = (out.value - value).abs() > s.step * 0.01;
    }
    // The drawn value glides (faster while dragging).
    let dt = ctx.input(|i| i.stable_dt).min(0.05);
    let shown_id = id.with("shown");
    let mut shown: f32 = ctx.data(|d| d.get_temp(shown_id)).unwrap_or(out.value);
    shown += (out.value - shown) * (dt * if resp.dragged() { 30.0 } else { 14.0 }).min(1.0);
    if (shown - out.value).abs() < (s.max - s.min) * 0.0005 {
        shown = out.value;
    } else {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(shown_id, shown));
    let hv = ctx.animate_bool_with_time(id.with("h"), (resp.hovered() || resp.dragged()) && enabled, 0.16);
    let dr = ctx.animate_bool_with_time(id.with("d"), resp.dragged(), 0.12);
    out.hot = hv;

    let k = if enabled { 1.0 } else { 0.4 };
    let at = |v: f32| a + dir * ((v - s.min) / (s.max - s.min) * len);
    let p = ui.painter();
    p.line_segment([a, b], Stroke::new(1.5_f32, dim.gamma_multiply(0.45 * k)));
    if s.marks > 0.0 {
        let mut m = (s.min / s.marks).ceil() * s.marks;
        while m <= s.max + 1e-4 {
            let c = at(m);
            let tall = if (m - s.neutral).abs() < 1e-4 { 5.0 } else { 2.5 };
            p.line_segment([c - across * tall, c + across * tall], Stroke::new(1.2_f32, dim.gamma_multiply(if tall > 3.0 { 0.8 } else { 0.4 } * k)));
            m += s.marks;
        }
    }
    if enabled {
        let (from, to) = (at(s.neutral.clamp(s.min, s.max)), at(shown));
        p.line_segment([from, to], Stroke::new(2.5 + 1.0 * hv, fg));
        let hh = 7.0 + 3.0 * hv + 2.0 * dr;
        if dr > 0.0 {
            p.circle_filled(to, 12.0 * dr, fg.gamma_multiply(0.12 * dr));
        }
        p.line_segment([to - across * hh, to + across * hh], Stroke::new(2.5 + 0.5 * hv, fg));
    }
    out
}
