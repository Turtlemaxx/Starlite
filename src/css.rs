//! CSS for the Custom theme (%APPDATA%\Starlite\theme.css).
//!
//! Starlite isn't a web page, so this is CSS for the parts it has: a handful
//! of selectors (`:root` / `body`, `.panel`, `.sidebar`, `.player`) and the
//! properties that mean something for them (colours, a background picture,
//! corner radius, blur, font). Custom properties (`--accent: …`) and
//! `var(--accent)` work. Anything else is listed with its line number
//! instead of being silently dropped. The file is read again whenever it's
//! saved, so changes show while you edit.

use std::collections::HashMap;
use std::path::PathBuf;

/// What a theme's CSS sets (None: left as the theme has it).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Css {
    pub background: Option<[u8; 3]>,
    /// `Some(None)`: `background-image: none`.
    pub image: Option<Option<PathBuf>>,
    pub accent: Option<[u8; 3]>,
    pub text: Option<[u8; 3]>,
    pub text_dim: Option<[u8; 3]>,
    /// Fills: colour and how solid (0..1).
    pub panel: Option<[f32; 4]>,
    pub sidebar: Option<[f32; 4]>,
    pub player: Option<[f32; 4]>,
    pub border: Option<[f32; 4]>,
    /// Points.
    pub radius: Option<f32>,
    pub blur: Option<f32>,
    pub font: Option<String>,
    /// What wasn't understood: (line, what).
    pub problems: Vec<(usize, String)>,
}

impl Css {
    /// Names of what it sets, for the settings line.
    pub fn sets(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut add = |on: bool, name| {
            if on {
                v.push(name);
            }
        };
        add(self.background.is_some(), "background");
        add(self.image.is_some(), "picture");
        add(self.accent.is_some(), "accent");
        add(self.text.is_some() || self.text_dim.is_some(), "text");
        add(self.panel.is_some(), "panels");
        add(self.sidebar.is_some(), "sidebar");
        add(self.player.is_some(), "player bar");
        add(self.border.is_some(), "borders");
        add(self.radius.is_some(), "corners");
        add(self.blur.is_some(), "blur");
        add(self.font.is_some(), "font");
        v
    }
}

/// The file a new theme.css starts as: everything explained, nothing switched on.
pub const TEMPLATE: &str = r#"/* Starlite theme CSS: for the Custom theme (Settings > Appearance > Custom).
   Save this file and Starlite shows the change right away.

   Starlite isn't a web page, so it understands the selectors and properties
   below and nothing else. Take a line out of its comment to use it.
   Colours: #rrggbb, #rrggbbaa, rgb(), rgba(), hsl() or a name like white. */

:root {
  /* --accent: #9682ff; */
  /* --background: #16142a; */
  /* --text: #f0eef8; */
  /* --text-dim: #a8a4bd; */
}

body {
  /* background: var(--background); */
  /* background-image: url("C:/Pictures/wallpaper.png"); */
  /* color: var(--text); */
  /* accent-color: var(--accent); */
  /* font-family: "Bahnschrift"; */        /* a font installed on this PC */
}

/* Panels, buttons and text boxes. */
.panel {
  /* background: rgba(255, 255, 255, 0.06); */
  /* border-color: rgba(255, 255, 255, 0.10); */
  /* border-radius: 14px; */
  /* backdrop-filter: blur(16px); */       /* how much panels blur what's behind */
}

.sidebar {
  /* background: rgba(0, 0, 0, 0.25); */
}

.player {
  /* background: rgba(150, 130, 255, 0.12); */
}
"#;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scope {
    Root,
    Panel,
    Sidebar,
    Player,
}

fn scope_of(selector: &str) -> Option<Scope> {
    match selector.trim() {
        ":root" | "html" | "body" | ".app" | ".window" | "*" => Some(Scope::Root),
        ".panel" | ".card" | ".button" => Some(Scope::Panel),
        ".sidebar" | ".nav" => Some(Scope::Sidebar),
        ".player" | ".player-bar" => Some(Scope::Player),
        _ => None,
    }
}

/// Comments blanked out (line breaks kept, so line numbers stay right).
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        let end = rest[i + 2..].find("*/").map(|e| i + 2 + e + 2).unwrap_or(rest.len());
        out.extend(rest[i..end].chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Splits at `sep`, but not inside brackets or quotes.
fn split_top(s: &str, sep: char) -> Vec<(usize, &str)> {
    let (mut depth, mut quote, mut start) = (0i32, None::<char>, 0usize);
    let mut out = Vec::new();
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, c) if c == sep && depth <= 0 => {
                out.push((start, &s[start..i]));
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push((start, &s[start..]));
    out
}

const NAMED: [(&str, [u8; 3]); 18] = [
    ("white", [255, 255, 255]),
    ("black", [0, 0, 0]),
    ("red", [255, 0, 0]),
    ("green", [0, 128, 0]),
    ("blue", [0, 0, 255]),
    ("yellow", [255, 255, 0]),
    ("orange", [255, 165, 0]),
    ("purple", [128, 0, 128]),
    ("pink", [255, 192, 203]),
    ("hotpink", [255, 105, 180]),
    ("cyan", [0, 255, 255]),
    ("teal", [0, 128, 128]),
    ("navy", [0, 0, 128]),
    ("gray", [128, 128, 128]),
    ("grey", [128, 128, 128]),
    ("silver", [192, 192, 192]),
    ("gold", [255, 215, 0]),
    ("crimson", [220, 20, 60]),
];

/// A CSS colour as [r, g, b, a], all 0..1.
pub fn color(v: &str) -> Option<[f32; 4]> {
    let v = v.trim().to_ascii_lowercase();
    if v == "transparent" {
        return Some([0.0, 0.0, 0.0, 0.0]);
    }
    if let Some(&(_, c)) = NAMED.iter().find(|(n, _)| *n == v) {
        return Some([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]);
    }
    if let Some(h) = v.strip_prefix('#') {
        if !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let h: String = if h.len() == 3 || h.len() == 4 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_string() };
        if h.len() != 6 && h.len() != 8 {
            return None;
        }
        let n = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
        return Some([n(0), n(2), n(4), if h.len() == 8 { n(6) } else { 1.0 }]);
    }
    let (name, args) = v.split_once('(')?;
    let args = args.strip_suffix(')')?;
    let parts: Vec<&str> = args.split([',', '/', ' ']).map(str::trim).filter(|p| !p.is_empty()).collect();
    // "50%" → 0.5 of `full`; a bare number as it is.
    let num = |s: &str, full: f32| -> Option<f32> {
        match s.strip_suffix('%') {
            Some(p) => p.parse::<f32>().ok().map(|x| x / 100.0 * full),
            None => s.trim_end_matches("deg").parse::<f32>().ok(),
        }
    };
    let alpha = match parts.get(3) {
        Some(a) => num(a, 1.0)?.clamp(0.0, 1.0),
        None => 1.0,
    };
    match name.trim() {
        "rgb" | "rgba" if parts.len() >= 3 => Some([(num(parts[0], 255.0)? / 255.0).clamp(0.0, 1.0), (num(parts[1], 255.0)? / 255.0).clamp(0.0, 1.0), (num(parts[2], 255.0)? / 255.0).clamp(0.0, 1.0), alpha]),
        "hsl" | "hsla" if parts.len() >= 3 => {
            let (h, s, l) = (num(parts[0], 360.0)?.rem_euclid(360.0) / 360.0, num(parts[1], 1.0)?.clamp(0.0, 1.0), num(parts[2], 1.0)?.clamp(0.0, 1.0));
            let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
            let p = 2.0 * l - q;
            let ch = |t: f32| {
                let t = t.rem_euclid(1.0);
                if t < 1.0 / 6.0 {
                    p + (q - p) * 6.0 * t
                } else if t < 0.5 {
                    q
                } else if t < 2.0 / 3.0 {
                    p + (q - p) * (2.0 / 3.0 - t) * 6.0
                } else {
                    p
                }
            };
            Some([ch(h + 1.0 / 3.0), ch(h), ch(h - 1.0 / 3.0), alpha])
        }
        _ => None,
    }
}

fn rgb8(c: [f32; 4]) -> [u8; 3] {
    [(c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8]
}

/// "14px" or "14" → 14.
fn length(v: &str) -> Option<f32> {
    let v = v.trim();
    v.strip_suffix("px").unwrap_or(v).trim().parse::<f32>().ok().filter(|x| x.is_finite() && *x >= 0.0)
}

/// `url("C:/a.png")` → the path.
fn url(v: &str) -> Option<String> {
    let i = v.find("url(")?;
    let rest = &v[i + 4..];
    let inner = rest[..rest.rfind(')')?].trim();
    let inner = inner.trim_matches(|c| c == '"' || c == '\'').trim();
    let inner = inner.strip_prefix("file:///").unwrap_or(inner);
    (!inner.is_empty()).then(|| inner.to_string())
}

/// `var(--x)` and `var(--x, fallback)` filled in.
fn resolve(value: &str, vars: &HashMap<String, String>) -> String {
    let mut v = value.to_string();
    // A variable can hold another one: a few rounds are plenty.
    for _ in 0..6 {
        let Some(i) = v.find("var(") else { break };
        let Some(len) = v[i..].find(')') else { break };
        let inner = &v[i + 4..i + len];
        let (name, fallback) = match inner.split_once(',') {
            Some((n, f)) => (n.trim(), f.trim()),
            None => (inner.trim(), ""),
        };
        let with = vars.get(name).cloned().unwrap_or_else(|| fallback.to_string());
        v.replace_range(i..i + len + 1, &with);
    }
    v
}

/// Reads a theme's CSS.
pub fn parse(text: &str) -> Css {
    // Notepad may put a byte-order mark in front.
    let text = strip_comments(text.trim_start_matches('\u{feff}'));
    let line_at = |pos: usize| text[..pos.min(text.len())].matches('\n').count() + 1;
    let mut css = Css::default();
    // Rules: (scope, where the block starts, its text).
    let mut rules: Vec<(Option<Scope>, usize, &str)> = Vec::new();
    let mut pos = 0;
    while let Some(open) = text[pos..].find('{') {
        let open = pos + open;
        let Some(close) = text[open..].find('}') else {
            css.problems.push((line_at(open), "a { that is never closed with }".to_string()));
            break;
        };
        let close = open + close;
        let selector = text[pos..open].trim();
        let block = &text[open + 1..close];
        for sel in selector.split(',') {
            let scope = scope_of(sel);
            if scope.is_none() {
                css.problems.push((line_at(open), format!("Starlite has no \u{201c}{}\u{201d} (it knows :root, body, .panel, .sidebar and .player)", sel.trim())));
            }
            rules.push((scope, open + 1, block));
        }
        pos = close + 1;
    }
    if !text[pos..].trim().is_empty() {
        css.problems.push((line_at(pos + text[pos..].find(|c: char| !c.is_whitespace()).unwrap_or(0)), "text outside of a { } block".to_string()));
    }
    // Variables first (they can be used before they're written).
    let mut vars: HashMap<String, String> = HashMap::new();
    for (scope, _, block) in &rules {
        if *scope == Some(Scope::Root) {
            for (_, d) in split_top(block, ';') {
                if let Some((k, v)) = d.split_once(':') {
                    if k.trim().starts_with("--") {
                        vars.insert(k.trim().to_string(), v.trim().to_string());
                    }
                }
            }
        }
    }
    for (scope, start, block) in &rules {
        let Some(scope) = scope else { continue };
        for (off, decl) in split_top(block, ';') {
            if decl.trim().is_empty() {
                continue;
            }
            let line = line_at(start + off + decl.find(|c: char| !c.is_whitespace()).unwrap_or(0));
            let Some((prop, raw)) = decl.split_once(':') else {
                css.problems.push((line, format!("\u{201c}{}\u{201d} isn't \u{201c}property: value\u{201d}", decl.trim())));
                continue;
            };
            let prop = prop.trim().to_ascii_lowercase();
            let value = resolve(raw.trim().trim_end_matches("!important").trim(), &vars);
            if let Err(why) = apply(&mut css, *scope, &prop, &value) {
                css.problems.push((line, why));
            }
        }
    }
    css.problems.sort();
    css
}

fn apply(css: &mut Css, scope: Scope, prop: &str, value: &str) -> Result<(), String> {
    let bad = || format!("\u{201c}{value}\u{201d} isn't something {prop} can be");
    let col = |v: &str| color(v).ok_or_else(bad);
    // "1px solid #fff", "url(a.png) #000": the first part that is a colour.
    let any_col = |v: &str| split_top(v, ' ').iter().find_map(|(_, part)| color(part)).ok_or_else(bad);
    let picture = |css: &mut Css, v: &str| -> Result<(), String> {
        if v.trim().eq_ignore_ascii_case("none") {
            css.image = Some(None);
        } else {
            css.image = Some(Some(PathBuf::from(url(v).ok_or_else(bad)?)));
        }
        Ok(())
    };
    match (scope, prop) {
        (Scope::Root, "--background" | "background" | "background-color") => {
            if value.contains("url(") {
                picture(css, &value[value.find("url(").unwrap()..=value.rfind(')').unwrap_or(value.len() - 1)])?;
                if let Ok(c) = any_col(value) {
                    css.background = Some(rgb8(c));
                }
            } else {
                css.background = Some(rgb8(col(value)?));
            }
        }
        (Scope::Root, "--background-image" | "background-image") => picture(css, value)?,
        (Scope::Root, "--accent" | "accent-color") => css.accent = Some(rgb8(col(value)?)),
        (Scope::Root, "--text" | "color") => css.text = Some(rgb8(col(value)?)),
        (Scope::Root, "--text-dim") => css.text_dim = Some(rgb8(col(value)?)),
        (Scope::Root, "--panel") | (Scope::Panel, "background" | "background-color") => css.panel = Some(col(value)?),
        (Scope::Root, "--sidebar") | (Scope::Sidebar, "background" | "background-color") => css.sidebar = Some(col(value)?),
        (Scope::Root, "--player") | (Scope::Player, "background" | "background-color") => css.player = Some(col(value)?),
        (Scope::Root, "--border") | (Scope::Panel, "border-color" | "border") => css.border = Some(any_col(value)?),
        (Scope::Root, "--radius") | (Scope::Panel, "border-radius") => css.radius = Some(length(value).ok_or_else(bad)?.min(40.0)),
        (Scope::Root, "--blur") | (Scope::Panel, "backdrop-filter") => {
            let inner = value.trim().strip_prefix("blur(").and_then(|v| v.strip_suffix(')')).unwrap_or(value);
            css.blur = Some(length(inner).ok_or_else(bad)?.min(60.0));
        }
        (Scope::Root, "--font" | "font-family") => {
            let first = split_top(value, ',').first().map(|(_, f)| f.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string()).unwrap_or_default();
            if first.is_empty() {
                return Err(bad());
            }
            css.font = Some(first);
        }
        // Other variables are fine: they're there to be used with var().
        (Scope::Root, p) if p.starts_with("--") => {}
        _ => {
            let place = match scope {
                Scope::Root => "body",
                Scope::Panel => ".panel",
                Scope::Sidebar => ".sidebar",
                Scope::Player => ".player",
            };
            return Err(format!("Starlite can't use \u{201c}{prop}\u{201d} on {place}"));
        }
    }
    Ok(())
}

/// The files of a font family (regular, semibold, bold) from Windows' font
/// list: pairs of ("Arial Bold (TrueType)", "arialbd.ttf"). A missing
/// weight falls back to the next one.
pub fn pick_fonts(table: &[(String, String)], family: &str) -> Option<[String; 3]> {
    let want = family.trim().to_lowercase();
    let face = |suffixes: &[&str]| -> Option<String> {
        table.iter().find_map(|(name, file)| {
            let n = name.to_lowercase();
            let n = n.split(" (").next().unwrap_or(&n).trim();
            let lower = file.to_lowercase();
            let usable = lower.ends_with(".ttf") || lower.ends_with(".otf");
            (usable && suffixes.iter().any(|s| n == format!("{want}{s}"))).then(|| file.clone())
        })
    };
    let regular = face(&["", " regular", " normal"])?;
    let bold = face(&[" bold"]).unwrap_or_else(|| regular.clone());
    let semibold = face(&[" semibold", " semi bold", " demibold", " demi", " medium"]).unwrap_or_else(|| bold.clone());
    Some([regular, semibold, bold])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(color("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(rgb8(color("#9682FF").unwrap()), [0x96, 0x82, 0xff]);
        assert_eq!(color("#00000080").unwrap()[3], 128.0 / 255.0);
        assert_eq!(color("rgba(255, 0, 0, 0.5)"), Some([1.0, 0.0, 0.0, 0.5]));
        assert_eq!(color("rgb(0 128 255 / 25%)").unwrap()[3], 0.25);
        assert_eq!(rgb8(color("hsl(0, 100%, 50%)").unwrap()), [255, 0, 0]);
        assert_eq!(rgb8(color("hsl(240deg 100% 50%)").unwrap()), [0, 0, 255]);
        assert_eq!(rgb8(color("HotPink").unwrap()), [255, 105, 180]);
        assert_eq!(color("transparent").unwrap()[3], 0.0);
        assert_eq!(color("#12345"), None);
        assert_eq!(color("14px"), None);
    }

    #[test]
    fn a_theme_is_read() {
        let css = parse(
            r#"/* my theme */
            :root { --accent: #ff8844; --glass: rgba(20, 20, 40, 0.5); --text-dim: #aaaacc }
            body {
              background: #101020;
              background-image: url("C:/Pictures/space wall.png");
              color: white; accent-color: var(--accent);
              font-family: "Bahnschrift", sans-serif;
            }
            .panel { background: var(--glass); border: 1px solid rgba(255,255,255,0.2); border-radius: 10px; backdrop-filter: blur(24px) }
            .sidebar, .player { background: var(--missing, #00000040); }"#,
        );
        assert_eq!(css.problems, vec![]);
        assert_eq!(css.background, Some([0x10, 0x10, 0x20]));
        assert_eq!(css.image, Some(Some(PathBuf::from("C:/Pictures/space wall.png"))));
        assert_eq!((css.accent, css.text, css.text_dim), (Some([0xff, 0x88, 0x44]), Some([255, 255, 255]), Some([0xaa, 0xaa, 0xcc])));
        assert_eq!(css.panel, Some([20.0 / 255.0, 20.0 / 255.0, 40.0 / 255.0, 0.5]));
        assert_eq!(css.border.unwrap()[3], 0.2);
        assert_eq!((css.radius, css.blur), (Some(10.0), Some(24.0)));
        assert_eq!(css.font.as_deref(), Some("Bahnschrift"));
        assert_eq!(css.sidebar, css.player);
        assert_eq!(css.sidebar.unwrap()[3], 0x40 as f32 / 255.0);
        assert_eq!(css.sets().len(), 11);
    }

    #[test]
    fn what_it_cant_use_is_listed_with_its_line() {
        let css = parse("body {\n  color: #fff;\n  margin: 4px;\n  background: bananas;\n}\n.header { color: red }\n.panel { border-radius: lots }");
        let lines: Vec<usize> = css.problems.iter().map(|p| p.0).collect();
        assert_eq!(lines, vec![3, 4, 6, 7]);
        assert!(css.problems[0].1.contains("margin") && css.problems[2].1.contains(".header"));
        // The good parts still count.
        assert_eq!(css.text, Some([255, 255, 255]));
        assert_eq!(css.background, None);
    }

    #[test]
    fn a_byte_order_mark_is_no_problem() {
        let css = parse("\u{feff}:root { --accent: #ff0000 }");
        assert_eq!((css.accent, css.problems.len()), (Some([255, 0, 0]), 0));
    }

    #[test]
    fn the_starter_file_changes_nothing() {
        let css = parse(TEMPLATE);
        assert_eq!(css, Css::default());
    }

    #[test]
    fn pictures_can_be_switched_off_and_radius_is_capped() {
        let css = parse(":root { --background-image: none; --radius: 900px; --blur: 8 }");
        assert_eq!((css.image.clone(), css.radius, css.blur), (Some(None), Some(40.0), Some(8.0)));
    }

    #[test]
    fn fonts_are_found_by_family() {
        let t: Vec<(String, String)> = [
            ("Arial (TrueType)", "arial.ttf"),
            ("Arial Bold (TrueType)", "arialbd.ttf"),
            ("Arial Black (TrueType)", "ariblk.ttf"),
            ("Segoe UI Semibold (TrueType)", "seguisb.ttf"),
            ("Segoe UI (TrueType)", "segoeui.ttf"),
            ("Segoe UI Bold (TrueType)", "segoeuib.ttf"),
            ("Cambria & Cambria Math (TrueType)", "cambria.ttc"),
            ("Bahnschrift (TrueType)", "bahnschrift.ttf"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(pick_fonts(&t, "segoe ui"), Some(["segoeui.ttf".into(), "seguisb.ttf".into(), "segoeuib.ttf".into()]));
        assert_eq!(pick_fonts(&t, "Arial"), Some(["arial.ttf".into(), "arialbd.ttf".into(), "arialbd.ttf".into()]));
        assert_eq!(pick_fonts(&t, "Bahnschrift").unwrap()[2], "bahnschrift.ttf");
        assert_eq!(pick_fonts(&t, "Comic Neue"), None);
    }
}
