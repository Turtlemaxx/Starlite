//! Small drawing helpers shared by the pages: truncated text, cover art,
//! glass pills and glass text fields.

use egui::load::{SizeHint, TexturePoll};
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Id, Painter, Pos2, Rect, Response,
    Sense, TextFormat, TextureOptions, Ui, Vec2,
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
    if let Some(uri) = uri {
        let poll = ui.ctx().try_load_texture(uri, TextureOptions::LINEAR, SizeHint::Scale(1.0.into()));
        if let Ok(TexturePoll::Ready { texture }) = poll {
            let uv = cover_uv(uri, texture.size, rect.size());
            egui::Image::from_texture(texture).uv(uv).corner_radius(radius.min(255.0) as u8).paint_at(ui, rect);
            return;
        }
    }
    placeholder(ui, icons, rect, radius, fallback, theme);
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
