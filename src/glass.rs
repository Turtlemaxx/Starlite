//! The renderer for the window's background and panels. One full-window
//! fragment shader draws the theme background (plain, a sakura tree with
//! falling petals, stars, bubbles, or the Custom theme's colour / picture)
//! and then up to MAX_EL elements on top of it: the nav bar, pills, panels
//! and buttons. egui draws text and icons above.
//!
//! Elements come in two looks. The simple one (the default) is flat: a
//! slightly see-through fill, a hairline and a small shadow, blurring what's
//! behind a little. Liquid glass (Settings → Appearance) refracts the
//! background (lensing with chromatic dispersion) and frosts it; glass
//! stacked on other glass (nav pills) layers over what is already there.

use eframe::glow::{self, HasContext};

use crate::theme::BgImage;
use crate::tree::TreeImage;

pub const MAX_EL: usize = 24;

const VS: &str = r#"#version 330 core
const vec2 V[3] = vec2[3](vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
void main() { gl_Position = vec4(V[gl_VertexID], 0.0, 1.0); }
"#;

const FS: &str = r#"#version 330 core
out vec4 o;
#define MAX_EL 24

uniform vec2  u_res;            // framebuffer size (px)
uniform float u_ppp;            // pixels per point
uniform float u_time;
uniform int   u_theme;          // 0 light, 1 dark, 2 cherry blossom, 3 stars, 4 bubbles, 5 custom
uniform int   u_count;
uniform vec4  u_rect[MAX_EL];   // centre xy, half size zw (px)
uniform vec4  u_prop[MAX_EL];   // radius px, strength, shadow size pt, kind
uniform vec4  u_fx[MAX_EL];     // hover glow, press, value, -
uniform vec2  u_light;          // unit vector towards the light
uniform vec3  u_ptr;            // pointer px + strength
uniform sampler2D u_tree;
uniform sampler2D u_tree_blur;
uniform vec4  u_tree_rect;      // min xy, size zw (px)
uniform float u_has_tree;
uniform int   u_overlay;        // 1 = overlay pass (dialogs, popovers) over the UI
uniform float u_fs;             // 0..1 fullscreen Now Playing (album-colour background)
uniform vec3  u_pal[4];         // album cover palette
uniform float u_dim;            // overlay: how much to darken the UI behind
uniform vec3  u_cbg;            // custom theme: background colour
uniform vec3  u_accent;         // custom theme: accent colour
uniform int   u_light_ui;       // custom theme: light background, light glass
uniform sampler2D u_img;        // custom theme: background picture
uniform float u_has_img;
uniform vec2  u_img_size;       // picture size (texels)
uniform int   u_flat;           // 1: simple flat panels instead of liquid glass
uniform vec4  u_cls[3];         // fill of panels / the sidebar / the player bar (a < 0: the look's own)
uniform vec4  u_edge;           // flat: hairline round panels (a < 0: the look's own)
uniform float u_round;          // largest corner radius of panels, px (pills and circles stay round)
uniform float u_frost;          // how far panels blur what's behind them, px

float hash1(float n) { return fract(sin(n * 127.1) * 43758.5453); }
float hash2(vec2 p) { return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453); }

bool light_ui() { return u_theme == 0 || (u_theme == 5 && u_light_ui == 1); }

vec3 bg_color() {
    // The simple look uses the greys of an ordinary desktop app.
    if (u_theme == 0) return u_flat == 1 ? vec3(0.953, 0.953, 0.961) : vec3(0.925, 0.925, 0.932);
    if (u_theme == 5) return u_cbg;
    if (u_theme == 1) return u_flat == 1 ? vec3(0.110, 0.110, 0.118) : vec3(0.071, 0.071, 0.078);
    if (u_theme == 3) return vec3(0.006, 0.006, 0.010);
    return vec3(0.010, 0.008, 0.010);
}

// Sakura petals drifting down from the canopy, fluttering as they fall.
vec4 petals(vec2 p, float blur) {
    vec4 acc = vec4(0.0);
    for (int i = 0; i < 30; i++) {
        float f = float(i);
        float h1 = hash1(f + 0.13), h2 = hash1(f * 1.91 + 3.7), h3 = hash1(f * 2.73 + 9.1);
        float dur = 10.0 + 8.0 * h2;
        float life = fract(u_time / dur + h1);
        vec2 org = u_tree_rect.xy + u_tree_rect.zw * vec2(0.15 + 0.75 * h3, 0.10 + 0.40 * h2);
        float fall = u_res.y - org.y + 30.0 * u_ppp;
        vec2 pos = org + vec2((40.0 + 300.0 * h1) * u_ppp * life
                              + 16.0 * u_ppp * sin(u_time * (0.8 + h3) + f * 2.0),
                              fall * life);
        float sz0 = (3.0 + 2.6 * h3) * u_ppp;
        // Behind frosted glass a petal spreads into a soft, dimmer glow.
        float sz = sz0 + blur * 0.8;
        float energy = (sz0 * sz0) / (sz * sz);
        vec2 d = p - pos;
        float reach = sz * 2.0 + blur;
        if (dot(d, d) > reach * reach) continue;
        float a = u_time * (0.5 + 1.1 * h2) + f;
        float flutter = 0.30 + 0.70 * abs(sin(u_time * (1.2 + 1.8 * h1) + f));
        vec2 q = mat2(cos(a), -sin(a), sin(a), cos(a)) * d;
        q.x /= flutter;
        float e = length(q / vec2(sz, sz * 0.62)) - 1.0;
        float aa = (1.0 + blur * 1.2) / sz;
        float cov = (1.0 - smoothstep(-aa, aa, e)) * energy;
        cov *= smoothstep(0.0, 0.05, life) * (1.0 - smoothstep(0.88, 1.0, life)) * 0.92;
        vec3 pc = mix(vec3(1.0, 0.70, 0.78), vec3(1.0, 0.87, 0.91), h2) * (0.70 + 0.30 * flutter);
        acc.rgb = acc.rgb * (1.0 - cov) + pc * cov;
        acc.a = acc.a * (1.0 - cov) + cov;
    }
    return acc;
}

// Twinkling star field: two hashed grid layers (many faint stars, fewer
// bright ones). Bright stars flash a four-point glint at the top of their
// twinkle. Behind frosted glass every star spreads into a soft, dimmer glow.
vec3 stars(vec2 p, float blur) {
    vec3 acc = vec3(0.0);
    for (int layer = 0; layer < 2; layer++) {
        float big = float(layer);
        float cell = mix(30.0, 84.0, big) * u_ppp;
        vec2 id = floor(p / cell);
        for (int dy = -1; dy <= 1; dy++) {
            for (int dx = -1; dx <= 1; dx++) {
                vec2 cid = id + vec2(float(dx), float(dy)) + big * 131.7;
                float h = hash2(cid);
                if (h > mix(0.42, 0.55, big)) continue;
                float h2 = hash2(cid + 3.1), h3 = hash2(cid + 7.7), h4 = hash2(cid + 11.3);
                vec2 pos = (floor(p / cell) + vec2(float(dx), float(dy)) + 0.15 + 0.7 * vec2(h2, h3)) * cell;
                vec2 d = p - pos;
                float r0 = (mix(0.55, 1.05, big) + 0.6 * h4) * u_ppp;
                float r = r0 + blur * 0.45;
                float tw = 0.5 + 0.5 * sin(u_time * (0.6 + 2.4 * h2) + h3 * 6.2832);
                float bright = mix(0.35, 1.0, big) * (0.25 + 0.75 * tw) * (0.6 + 0.4 * h);
                float core = exp(-dot(d, d) / (r * r)) * (r0 * r0) / (r * r);
                float glint = 0.0;
                if (layer == 1) {
                    float peak = pow(max(tw - 0.55, 0.0) / 0.45, 2.0) * step(0.45, h4);
                    float w = 0.55 * u_ppp + blur * 0.35;
                    float len = (5.0 + 5.0 * h3) * u_ppp;
                    glint = peak * (exp(-abs(d.y) / w - abs(d.x) / len) + exp(-abs(d.x) / w - abs(d.y) / len))
                          * (w > 1.5 * u_ppp ? 0.25 : 0.8);
                }
                vec3 tint = mix(vec3(0.86, 0.90, 1.0), vec3(1.0, 0.94, 0.86), step(0.8, h3));
                acc += tint * bright * (core + glint);
            }
        }
    }
    return acc;
}

vec3 hue(float h) {
    return clamp(abs(fract(h + vec3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0, 0.0, 1.0);
}

// Bubbles: deep blue-black with a faint teal glow along the bottom, where
// the bubbles come from.
vec3 bubbles_bg(vec2 p) {
    vec2 q = p / u_res;
    vec3 c = vec3(0.014, 0.020, 0.034);
    float g = exp(-pow((1.0 - q.y) * 2.4, 2.0)) * (0.75 + 0.25 * sin(q.x * 4.0 + u_time * 0.07));
    return c + vec3(0.006, 0.034, 0.052) * g;
}

// Liquid-glass bubbles rising from the bottom. Each one pops at its own
// height and place, then a new one starts somewhere else. A bubble is a
// clear sphere: a bright, iridescent rim (a thin film, its colours drifting),
// a soft highlight and almost nothing in the middle. Popping leaves a ring
// that spreads and a few droplets that fly off.
vec3 bubbles(vec2 p, float blur) {
    vec3 c = bubbles_bg(p);
    for (int i = 0; i < 20; i++) {
        float f = float(i);
        float dur = 11.0 + 8.0 * hash1(f * 1.37 + 0.21);
        float cyc = u_time / dur + hash1(f * 2.91 + 0.7);
        float n = floor(cyc);
        float s = fract(cyc) * dur;
        float k1 = hash1(f * 7.31 + n * 1.13 + 0.5);
        float k2 = hash1(f * 3.17 + n * 2.71 + 1.9);
        float k3 = hash1(f * 5.53 + n * 0.37 + 3.3);
        float r = (8.0 + 30.0 * k1 * k1) * u_ppp;
        float y0 = u_res.y + r + 6.0 * u_ppp;
        float y_pop = u_res.y * mix(0.08, 0.74, k2);
        float rise = dur * (0.60 + 0.28 * k3);
        float u = min(s / rise, 1.0);
        vec2 ctr = vec2(u_res.x * (0.04 + 0.92 * hash1(f * 9.7 + n * 4.1 + 2.2)), mix(y0, y_pop, u * (0.55 + 0.45 * u)));
        ctr.x += sin(s * (1.1 + k1) + f) * (3.0 + 7.0 * k3) * u_ppp;
        float pop = s - rise;
        vec2 d = p - ctr;
        float reach = r * 2.6 + blur * 2.0 + 4.0 * u_ppp;
        if (abs(d.x) > reach || abs(d.y) > reach) continue;
        float soft = 1.0 + blur * 0.7;
        float energy = (r * r) / ((r + blur * 0.5) * (r + blur * 0.5));
        if (pop < 0.0) {
            // Wobbles a little as it rises.
            float w = 0.035 * sin(s * 3.1 + f * 1.7);
            vec2 e = d / vec2(1.0 + w, 1.0 - w);
            float dist = length(e);
            if (dist > r + soft * 2.0) continue;
            float nd = clamp(dist / r, 0.0, 1.0);
            float cov = 1.0 - smoothstep(r - soft, r + soft, dist);
            float film = 0.5 + 0.5 * sin(e.y / r * 3.0 + e.x / r * 1.7 + u_time * 0.8 + f * 2.3);
            vec3 iri = mix(vec3(0.86, 0.94, 1.0), hue(fract(0.52 + 0.5 * film + nd * 0.3)), 0.38);
            float rim = pow(nd, 6.0) * 0.8 + pow(nd, 2.0) * 0.05;
            // A crisp glassy edge, brightest towards the light (top left).
            float lit = 0.55 + 0.45 * dot(normalize(e + 1e-4), vec2(-0.7, -0.7));
            float edge = exp(-pow((r - dist) / (0.9 * u_ppp + blur * 0.6), 2.0)) * 0.35 * lit;
            vec2 h1 = e / r - vec2(-0.36, -0.42);
            vec2 h2 = e / r - vec2(0.38, 0.40);
            float spec = exp(-dot(h1, h1) * 24.0) * 0.75 + exp(-dot(h2, h2) * 46.0) * 0.18;
            c += (iri * (0.025 + rim) + vec3(spec + edge)) * cov * energy;
        } else if (pop < 0.5) {
            float a = 1.0 - pop / 0.5;
            float dist = length(d);
            // A thin ring that spreads and fades.
            float rr = r * (1.0 + 1.6 * pop / 0.5);
            float ring = exp(-pow((dist - rr) / (0.8 * u_ppp + blur * 0.5 + 0.06 * r * (1.0 - a)), 2.0));
            c += mix(vec3(0.85, 0.93, 1.0), hue(fract(0.55 + f * 0.13)), 0.4) * ring * a * a * 0.55 * energy;
            // Droplets flying off (falling a little).
            for (int j = 0; j < 7; j++) {
                float ang = float(j) * 0.8976 + k1 * 6.2832;
                float dd = r * (1.0 + 2.4 * (pop / 0.5) * (0.7 + 0.3 * hash1(f + float(j))));
                vec2 dp = ctr + vec2(cos(ang), sin(ang)) * dd + vec2(0.0, 90.0 * pop * pop * u_ppp);
                float ds = (1.3 + 0.9 * hash1(f * 3.0 + float(j))) * u_ppp + blur * 0.4;
                vec2 q = p - dp;
                c += vec3(0.80, 0.90, 1.0) * exp(-dot(q, q) / (ds * ds)) * a * 0.7 * energy;
            }
        }
    }
    return c;
}

// Custom theme: the picked colour, or the picked picture (filling the
// window) with a touch of that colour. Under frosted glass it's blurred.
vec3 custom_bg(vec2 p, float blur) {
    if (u_has_img < 0.5) return u_cbg;
    float sc = max(u_res.x / u_img_size.x, u_res.y / u_img_size.y);
    vec2 uv = ((p - u_res * 0.5) / sc + u_img_size * 0.5) / u_img_size;
    vec3 im;
    if (blur > 0.5) {
        float lod = log2(max(blur / sc, 1.0)) + 1.0;
        vec2 o = vec2(blur / sc * 0.6) / u_img_size;
        im = (textureLod(u_img, uv, lod).rgb * 2.0
            + textureLod(u_img, uv + o, lod).rgb + textureLod(u_img, uv - o, lod).rgb
            + textureLod(u_img, uv + vec2(o.x, -o.y), lod).rgb + textureLod(u_img, uv + vec2(-o.x, o.y), lod).rgb) / 6.0;
    } else {
        im = texture(u_img, uv).rgb;
    }
    return mix(im, u_cbg, 0.22);
}

vec4 tree(vec2 p, float blur) {
    vec2 uv = (p - u_tree_rect.xy) / u_tree_rect.zw;
    if (u_has_tree < 0.5 || any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0)))) return vec4(0.0);
    // Frosted glass reads the pre-blurred copy; the open background the sharp one.
    return blur < 0.5 ? texture(u_tree, uv) : texture(u_tree_blur, uv);
}

// Slowly drifting colour fields made from the album cover's palette.
vec3 album_bg(vec2 p) {
    vec2 q = p / u_res;
    float t = u_time * 0.06;
    vec2 c0 = vec2(0.25 + 0.15 * sin(t * 1.3), 0.30 + 0.15 * cos(t * 1.1));
    vec2 c1 = vec2(0.80 + 0.12 * cos(t * 0.9), 0.25 + 0.18 * sin(t * 1.4));
    vec2 c2 = vec2(0.30 + 0.18 * cos(t * 1.2 + 2.0), 0.85 + 0.10 * sin(t * 0.8));
    vec2 c3 = vec2(0.75 + 0.15 * sin(t * 0.7 + 1.0), 0.80 + 0.12 * cos(t * 1.5));
    vec2 ar = vec2(u_res.x / u_res.y, 1.0);
    float w0 = exp(-dot((q - c0) * ar, (q - c0) * ar) * 3.0);
    float w1 = exp(-dot((q - c1) * ar, (q - c1) * ar) * 3.0);
    float w2 = exp(-dot((q - c2) * ar, (q - c2) * ar) * 3.0);
    float w3 = exp(-dot((q - c3) * ar, (q - c3) * ar) * 3.0);
    vec3 col = (u_pal[0] * w0 + u_pal[1] * w1 + u_pal[2] * w2 + u_pal[3] * w3 + u_pal[0] * 0.15) / (w0 + w1 + w2 + w3 + 0.15);
    // Keep it deep enough for white text.
    return col * 0.62;
}

vec3 backdrop(vec2 p, float blur) {
    vec3 c = bg_color();
    if (u_fs > 0.001) {
        vec3 a = album_bg(p);
        if (u_fs >= 0.999) return a;
        c = mix(c, a, u_fs);
        return c;
    }
    if (u_theme == 2) {
        vec4 t = tree(p, blur);
        c = c * (1.0 - t.a) + t.rgb;
        vec4 q = petals(p, blur);
        c = c * (1.0 - q.a) + q.rgb;
    } else if (u_theme == 3) {
        c += stars(p, blur);
    } else if (u_theme == 4) {
        c = bubbles(p, blur);
    } else if (u_theme == 5) {
        c = custom_bg(p, blur);
    }
    return c;
}

float sdf(vec2 p, vec4 r, float rad) {
    vec2 q = abs(p - r.xy) - r.zw + rad;
    vec2 m = max(q, 0.0);
    // Fully rounded elements (buttons, pills) are true circles / capsules;
    // panels get continuous-curvature (superellipse) corners.
    float n = rad >= min(r.z, r.w) - 0.5 ? 2.0 : 4.0;
    return min(max(q.x, q.y), 0.0) + pow(pow(m.x, n) + pow(m.y, n), 1.0 / n) - rad;
}

// Volume track (kind 2): a capsule with the filled part up to `value`.
vec3 track(vec3 col, vec2 p, int i) {
    vec4 R = u_rect[i];
    float d = sdf(p, R, R.w);
    float cov = clamp(0.5 - d, 0.0, 1.0);
    if (cov <= 0.0) return col;
    float fill_x = R.x - R.z + 2.0 * R.z * u_fx[i].z;
    vec3 on = u_theme == 5 ? u_accent
            : (light_ui() ? vec3(0.20, 0.20, 0.23)
            : (u_theme == 2 ? vec3(1.0, 0.74, 0.81) : (u_theme == 4 ? vec3(0.78, 0.92, 1.0) : vec3(0.93))));
    vec3 off = light_ui() ? mix(col, vec3(0.55), 0.35) : mix(col, vec3(1.0), 0.16);
    vec3 c = mix(off, on, clamp(fill_x - p.x + 0.5, 0.0, 1.0));
    return mix(col, c, cov);
}

// An element's corner radius: panels can be capped (pills and circles stay round).
float radius_of(int i) {
    vec4 R = u_rect[i];
    float rad = u_prop[i].x;
    return rad >= min(R.z, R.w) - 0.5 ? rad : min(rad, u_round);
}

// Progress fill (the player bar doubles as the seek bar).
vec3 progress(vec3 c, vec2 p, int i, float d) {
    vec4 R = u_rect[i];
    float fill = u_fx[i].w;
    if (fill < 0.0) return c;
    float fx = R.x - R.z + 2.0 * R.z * fill;
    float f = clamp(fx - p.x + 0.5, 0.0, 1.0);
    vec3 tint = u_theme == 5 ? mix(c, u_accent, 0.18)
              : (light_ui() ? c * 0.93 : (u_theme == 2 ? mix(c, vec3(1.0, 0.72, 0.80), 0.16) : c + vec3(0.065)));
    c = mix(c, tint, f);
    // Bright playhead line.
    float head = exp(-abs(p.x - fx) / (1.1 * u_ppp)) * smoothstep(5.0 * u_ppp, 10.0 * u_ppp, -d) * step(0.002, fill) * step(fill, 0.998);
    return c + head * (light_ui() ? 0.0 : 0.35) - head * (light_ui() ? 0.25 : 0.0);
}

// The simple look: flat, slightly see-through panels with a hairline and a
// small soft shadow. No bending, no reflections. Panels still blur what's
// behind them a little, so pictures and moving backgrounds stay calm.
vec3 flat_layer(vec3 col, vec2 p, int i, int kind, int cls, bool over) {
    vec4 R = u_rect[i];
    vec4 P = u_prop[i];
    float rad = radius_of(i), s = P.y, sh = P.z;
    float glow = u_fx[i].x, press = u_fx[i].y, value = u_fx[i].z;
    if (s <= 0.001) return col;
    bool light = light_ui();
    float d = sdf(p, R, rad);
    if (d > sh * 5.0 * u_ppp + 4.0) return col;

    vec3 shadowed = col;
    if (kind == 0 && cls != 1) {
        float ds = sdf(p - vec2(0.0, sh * 0.3 * u_ppp), R, rad);
        shadowed = col * (1.0 - s * (light ? 0.06 : 0.24) * exp(-max(ds, 0.0) / (sh * 1.3 * u_ppp + 1.0)));
    } else if (kind == 3) {
        float ds = sdf(p - vec2(0.0, 1.0 * u_ppp), R, rad);
        shadowed = col * (1.0 - s * (light ? 0.22 : 0.40) * exp(-max(ds, 0.0) / (2.5 * u_ppp)));
    }
    float cov = clamp(0.5 - d, 0.0, 1.0);
    if (cov <= 0.0) return shadowed;

    vec3 c;
    vec4 edge = u_edge.a >= 0.0 ? u_edge : (light ? vec4(0.0, 0.0, 0.0, 0.10) : vec4(1.0, 1.0, 1.0, 0.085));
    float line = 1.0 - smoothstep(0.0, 1.0 * u_ppp, -d);
    if (kind == 0) {
        vec3 under = backdrop(p, u_frost);
        vec4 surf = u_cls[cls].a >= 0.0 ? u_cls[cls]
                  : (light ? vec4(1.0, 1.0, 1.0, cls == 1 ? 0.34 : 0.66) : vec4(1.0, 1.0, 1.0, cls == 1 ? 0.028 : 0.052));
        // Dialogs and menus sit over the page: more solid, and calmer behind.
        if (over) {
            under = mix(under, bg_color(), 0.55);
            if (u_cls[cls].a < 0.0) surf.a = light ? 0.88 : 0.095;
        }
        c = mix(under, surf.rgb, clamp(surf.a, 0.0, 1.0));
        // Hover, emphasis and press only shift the tone a touch.
        float lift = (light ? -0.030 : 0.050) * glow + (light ? -0.040 : 0.035) * press;
        c += lift;
        c = mix(c, edge.rgb, edge.a * line);
    } else if (kind == 3) {
        // The volume knob: a plain disc.
        c = light ? vec3(1.0) : vec3(0.94);
        c = mix(c, vec3(0.0), (light ? 0.16 : 0.0) * line);
    } else {
        // Something on a panel (the picked row, a hovered button): a faint tint.
        float a = (light ? 0.060 : 0.085) + (light ? 0.050 : 0.065) * glow + (light ? 0.040 : 0.040) * press;
        c = mix(col, light ? vec3(0.0) : vec3(1.0), a);
    }
    c = progress(c, p, i, d);
    return mix(shadowed, clamp(c, 0.0, 1.0), cov * min(s, 1.0));
}

// kind: 0 glass on the background (refracts it), 1 glass stacked on glass,
// 3 lens (stacked glass that magnifies the volume track under it).
vec3 layer(vec3 col, vec2 p, int i, int kind, int cls, vec3 pre_track, int tr) {
    vec4 R = u_rect[i];
    vec4 P = u_prop[i];
    float rad = radius_of(i), s = P.y, sh = P.z;
    float glow = u_fx[i].x, press = u_fx[i].y, value = u_fx[i].z;
    if (s <= 0.001) return col;
    bool light = light_ui();

    float d = sdf(p, R, rad);
    if (d > sh * 7.0 * u_ppp + 4.0) return col;   // far outside: untouched

    // Shadow: a soft drop shadow plus a tight contact shadow.
    float ds = sdf(p - vec2(0.0, sh * 0.5 * u_ppp), R, rad);
    float shAmt = (light ? 0.10 : 0.45) * (1.0 + 0.6 * glow);
    vec3 shadowed = col * (1.0 - s * shAmt * exp(-max(ds, 0.0) / (sh * 2.2 * u_ppp)))
                        * (1.0 - s * (light ? 0.06 : 0.25) * exp(-max(d, 0.0) / (1.5 * u_ppp)));
    float cov = clamp(0.5 - d, 0.0, 1.0);
    if (cov <= 0.0) return shadowed;

    vec2 e = vec2(1.0, 0.0);
    vec2 g = vec2(sdf(p + e.xy, R, rad) - sdf(p - e.xy, R, rad),
                  sdf(p + e.yx, R, rad) - sdf(p - e.yx, R, rad));
    vec2 n = g / max(length(g), 1e-5);
    float inside = -d;
    float bevel = clamp(rad * 0.9, 4.0 * u_ppp, 20.0 * u_ppp);
    float edge = 1.0 - clamp(inside / bevel, 0.0, 1.0);
    float slope = edge * edge * (1.0 + 0.6 * edge);
    float lit = max(dot(n, u_light), 0.0), back = max(dot(n, -u_light), 0.0);
    float sl = pow(lit, 3.0), sb = pow(back, 3.0);

    vec2 dp = p - u_ptr.xy;
    float near = exp(-dot(dp, dp) / pow(120.0 * u_ppp, 2.0)) * u_ptr.z;

    // What the glass sees.
    vec3 under;
    vec2 disp = -n * bevel * 0.9 * slope;
    if (kind == 0) {
        float blur = u_frost;
        if (u_theme == 2) {
            float k = 0.05 * slope;
            under = vec3(backdrop(p + disp * (1.0 + k), blur).r,
                         backdrop(p + disp, blur).g,
                         backdrop(p + disp * (1.0 - k), blur).b);
        } else {
            under = backdrop(p + disp, blur);
        }
    } else if (kind == 3 && tr >= 0) {
        // Magnify the track beneath the lens (1.6x) and bend it at the rim.
        vec2 q = R.xy + (p - R.xy) / 1.6 + disp * 0.6;
        under = track(pre_track, q, tr);
    } else {
        under = col;
    }

    // Body: milky glass. `value` is clarity: the lens clears up with it, and
    // clear panels (the nav bar) let the background read straight through.
    float milk;
    if (kind == 3) {
        milk = mix(0.92, 0.04, value);
    } else {
        float base = light ? 0.70 : 0.07;
        float clear = light ? 0.10 : 0.015;
        milk = mix(base, clear, value) + (light ? 0.12 : 0.06) * glow + (light ? 0.10 : 0.07) * press;
    }

    // A set panel colour tints the glass instead of plain white.
    vec3 tintc = vec3(1.0);
    if (kind == 0 && u_cls[cls].a >= 0.0) {
        tintc = u_cls[cls].rgb;
        milk = clamp(u_cls[cls].a + (light ? 0.12 : 0.06) * glow + (light ? 0.10 : 0.07) * press, 0.0, 1.0);
    }
    vec3 c = mix(under, tintc, milk);
    if (light) {
        c -= slope * (0.09 + 0.05 * glow) * (1.0 - 0.75 * lit);      // refraction ring
        c += pow(edge, 6.0) * (0.30 * sl + 0.15 * sb);
        vec3 lc = mix(vec3(0.70, 0.70, 0.74), vec3(1.0), clamp(0.05 + 1.4 * sl + 0.5 * sb + near, 0.0, 1.0));
        c = mix(c, lc, (1.0 - smoothstep(0.0, 1.2 * u_ppp, inside)) * 0.95);
    } else {
        c += slope * 0.06 * (0.25 + lit);
        c += pow(edge, 6.0) * (0.45 * sl + 0.22 * sb + 0.25 * near);
        c += exp(-inside / (4.0 * u_ppp)) * (0.03 + 0.12 * sl + 0.06 * sb);
        c += near * 0.035;
        vec3 lc = mix(vec3(0.18), vec3(0.88), clamp(0.08 + 1.2 * sl + 0.7 * sb + near, 0.0, 1.0));
        c = mix(c, lc, (1.0 - smoothstep(0.0, 1.2 * u_ppp, inside)) * 0.95);
    }
    c = progress(c, p, i, d);
    return mix(shadowed, clamp(c, 0.0, 1.0), cov * min(s, 1.0));
}

void main() {
    vec2 p = vec2(gl_FragCoord.x, u_res.y - gl_FragCoord.y);
    vec3 col = backdrop(p, 0.0);
    if (u_overlay == 1) {
        // Glass drawn over the finished UI: the first element is the panel.
        // Outside it we only darken (dim + its shadow); inside, the glass sees
        // the frosted theme background, dimmed like everything else.
        vec3 base = col * (1.0 - u_dim);
        col = base;
        for (int i = 0; i < MAX_EL; i++) {
            if (i >= u_count) break;
            int k = i == 0 ? 0 : int(u_prop[i].w + 0.5) % 10;
            col = u_flat == 1 ? flat_layer(col, p, i, k, 0, i == 0) : layer(col, p, i, k, 0, base, -1);
        }
        float d0 = sdf(p, u_rect[0], radius_of(0));
        float cov0 = clamp(0.5 - d0, 0.0, 1.0);
        float ds = sdf(p - vec2(0.0, 8.0 * u_ppp), u_rect[0], radius_of(0));
        float sh = (light_ui() ? 0.16 : 0.45) * exp(-max(ds, 0.0) / (16.0 * u_ppp));
        float a_out = 1.0 - (1.0 - u_dim) * (1.0 - sh);
        col += (hash2(p) - 0.5) / 255.0;
        o = vec4(clamp(col, 0.0, 1.0) * cov0, cov0 + (1.0 - cov0) * a_out);
        return;
    }
    vec3 pre_track = col;
    int tr = -1;
    for (int i = 0; i < MAX_EL; i++) {
        if (i >= u_count) break;
        int code = int(u_prop[i].w + 0.5);
        int kind = code % 10, cls = min(code / 10, 2);
        if (kind == 2) {
            pre_track = col;
            tr = i;
            col = track(col, p, i);
        } else if (u_flat == 1) {
            col = flat_layer(col, p, i, kind, cls, false);
        } else {
            col = layer(col, p, i, kind, cls, pre_track, tr);
        }
    }
    col += (hash2(p) - 0.5) / 255.0;
    o = vec4(clamp(col, 0.0, 1.0), 1.0);
}
"#;

/// One glass element, in physical pixels.
#[derive(Clone, Copy, Default)]
pub struct El {
    pub center: [f32; 2],
    pub half: [f32; 2],
    pub radius: f32,
    pub strength: f32,
    /// Shadow size in points.
    pub shadow: f32,
    pub kind: Kind,
    pub glow: f32,
    pub press: f32,
    /// Track: fill fraction. Lens / Glass: clarity (0 milky .. 1 clear).
    pub value: f32,
    /// Progress fill 0..1 drawn inside the element (< 0 = none).
    pub fill: f32,
    /// Which part of the window it is (a theme's CSS can colour these apart).
    pub class: Class,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Class {
    #[default]
    Panel = 0,
    Sidebar = 1,
    Player = 2,
}

impl El {
    /// Element from a rect in points.
    #[allow(clippy::too_many_arguments)]
    pub fn rect(r: egui::Rect, ppp: f32, radius: f32, strength: f32, shadow: f32, kind: Kind, glow: f32, press: f32) -> El {
        El {
            center: [r.center().x * ppp, r.center().y * ppp],
            half: [r.width() * 0.5 * ppp, r.height() * 0.5 * ppp],
            radius: radius * ppp,
            strength,
            shadow,
            kind,
            glow,
            press,
            value: 0.0,
            fill: -1.0,
            class: Class::Panel,
        }
    }

    pub fn with_fill(mut self, f: f32) -> El {
        self.fill = f.clamp(0.0, 1.0);
        self
    }

    pub fn with_class(mut self, c: Class) -> El {
        self.class = c;
        self
    }

    pub fn with_value(mut self, v: f32) -> El {
        self.value = v;
        self
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    /// Glass sitting on the background: refracts and frosts it.
    #[default]
    Glass = 0,
    /// Glass stacked on other glass.
    Layered = 1,
    /// Slider track (drawn flat, under its lens).
    Track = 2,
    /// Glass lens that magnifies the track beneath it.
    Lens = 3,
}

#[derive(Clone, Copy)]
pub struct Frame {
    pub res: [f32; 2],
    pub ppp: f32,
    pub time: f32,
    pub theme: i32,
    pub els: [El; MAX_EL],
    pub count: usize,
    pub light: [f32; 2],
    pub pointer: [f32; 3],
    pub tree_rect: [f32; 4],
    /// Overlay pass: drawn over the UI with blending; `dim` darkens what's behind.
    pub overlay: bool,
    pub dim: f32,
    /// Fullscreen Now Playing amount and the album palette for its background.
    pub fs: f32,
    pub palette: [[f32; 3]; 4],
    /// Custom theme: background and accent colours (0..1), light UI.
    pub custom_bg: [f32; 3],
    pub accent: [f32; 3],
    pub light_ui: bool,
    /// The simple flat look (liquid glass off).
    pub flat: bool,
    /// Fills of panels / the sidebar / the player bar set by a theme's CSS
    /// (alpha < 0: the look's own), and the same for the hairline.
    pub fills: [[f32; 4]; 3],
    pub edge: [f32; 4],
    /// Largest panel corner radius and blur behind panels (points).
    pub round: f32,
    pub frost: f32,
}

pub struct GlassRenderer {
    program: glow::Program,
    vao: glow::VertexArray,
    tree_tex: glow::Texture,
    tree_blur_tex: glow::Texture,
    has_tree: bool,
    pub pending_tree: Option<TreeImage>,
    bg_tex: glow::Texture,
    /// Size of the custom theme's picture, when there is one.
    bg_size: Option<[f32; 2]>,
    /// A new picture for the custom theme (`Some(None)` removes it).
    pub pending_bg: Option<Option<BgImage>>,
}

unsafe fn compile(gl: &glow::Context, kind: u32, src: &str) -> glow::Shader {
    let s = gl.create_shader(kind).expect("create shader");
    gl.shader_source(s, src);
    gl.compile_shader(s);
    assert!(gl.get_shader_compile_status(s), "shader: {}", gl.get_shader_info_log(s));
    s
}

impl GlassRenderer {
    pub fn new(gl: &glow::Context) -> Self {
        unsafe {
            let program = gl.create_program().expect("create program");
            let vs = compile(gl, glow::VERTEX_SHADER, VS);
            let fs = compile(gl, glow::FRAGMENT_SHADER, FS);
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            assert!(gl.get_program_link_status(program), "link: {}", gl.get_program_info_log(program));
            gl.detach_shader(program, vs);
            gl.detach_shader(program, fs);
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            let vao = gl.create_vertex_array().expect("vao");
            let make_tex = || {
                let t = gl.create_texture().expect("texture");
                gl.bind_texture(glow::TEXTURE_2D, Some(t));
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                t
            };
            let tree_tex = make_tex();
            let tree_blur_tex = make_tex();
            let bg_tex = make_tex();
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
            Self { program, vao, tree_tex, tree_blur_tex, has_tree: false, pending_tree: None, bg_tex, bg_size: None, pending_bg: None }
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_texture(self.tree_tex);
            gl.delete_texture(self.tree_blur_tex);
            gl.delete_texture(self.bg_tex);
        }
    }

    pub fn paint(&mut self, gl: &glow::Context, f: &Frame) {
        unsafe {
            if let Some(img) = self.pending_tree.take() {
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                for (tex, data) in [(self.tree_tex, &img.rgba), (self.tree_blur_tex, &img.blurred)] {
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        img.width as i32,
                        img.height as i32,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(data)),
                    );
                }
                self.has_tree = true;
            }
            if let Some(bg) = self.pending_bg.take() {
                self.bg_size = None;
                if let Some(img) = bg {
                    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
                    gl.bind_texture(glow::TEXTURE_2D, Some(self.bg_tex));
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        img.width as i32,
                        img.height as i32,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&img.rgba)),
                    );
                    // Smaller copies: what frosted glass shows of the picture.
                    gl.generate_mipmap(glow::TEXTURE_2D);
                    self.bg_size = Some([img.width as f32, img.height as f32]);
                }
            }

            if f.overlay {
                gl.enable(glow::BLEND);
                gl.blend_equation(glow::FUNC_ADD);
                gl.blend_func_separate(glow::ONE, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            } else {
                gl.disable(glow::BLEND);
            }
            gl.disable(glow::SCISSOR_TEST);
            gl.viewport(0, 0, f.res[0] as i32, f.res[1] as i32);
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE2);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.bg_tex));
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tree_blur_tex));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tree_tex));

            let p = self.program;
            let loc = |n: &str| gl.get_uniform_location(p, n);
            gl.uniform_1_i32(loc("u_tree").as_ref(), 0);
            gl.uniform_1_i32(loc("u_tree_blur").as_ref(), 1);
            gl.uniform_1_i32(loc("u_img").as_ref(), 2);
            gl.uniform_1_f32(loc("u_has_img").as_ref(), if self.bg_size.is_some() { 1.0 } else { 0.0 });
            let is = self.bg_size.unwrap_or([1.0, 1.0]);
            gl.uniform_2_f32(loc("u_img_size").as_ref(), is[0], is[1]);
            gl.uniform_3_f32(loc("u_cbg").as_ref(), f.custom_bg[0], f.custom_bg[1], f.custom_bg[2]);
            gl.uniform_3_f32(loc("u_accent").as_ref(), f.accent[0], f.accent[1], f.accent[2]);
            gl.uniform_1_i32(loc("u_light_ui").as_ref(), f.light_ui as i32);
            gl.uniform_1_i32(loc("u_flat").as_ref(), f.flat as i32);
            let fills: Vec<f32> = f.fills.iter().flatten().copied().collect();
            gl.uniform_4_f32_slice(loc("u_cls[0]").as_ref(), &fills);
            gl.uniform_4_f32(loc("u_edge").as_ref(), f.edge[0], f.edge[1], f.edge[2], f.edge[3]);
            gl.uniform_1_f32(loc("u_round").as_ref(), f.round * f.ppp);
            gl.uniform_1_f32(loc("u_frost").as_ref(), f.frost * f.ppp);
            gl.uniform_2_f32(loc("u_res").as_ref(), f.res[0], f.res[1]);
            gl.uniform_1_f32(loc("u_ppp").as_ref(), f.ppp);
            gl.uniform_1_f32(loc("u_time").as_ref(), f.time);
            gl.uniform_1_i32(loc("u_theme").as_ref(), f.theme);
            gl.uniform_1_i32(loc("u_count").as_ref(), f.count.min(MAX_EL) as i32);
            let mut rect = [0.0f32; MAX_EL * 4];
            let mut prop = [0.0f32; MAX_EL * 4];
            let mut fx = [0.0f32; MAX_EL * 4];
            for (i, e) in f.els.iter().enumerate() {
                rect[i * 4..i * 4 + 4].copy_from_slice(&[e.center[0], e.center[1], e.half[0], e.half[1]]);
                prop[i * 4..i * 4 + 4].copy_from_slice(&[e.radius, e.strength, e.shadow, (e.kind as i32 + 10 * e.class as i32) as f32]);
                fx[i * 4..i * 4 + 4].copy_from_slice(&[e.glow, e.press, e.value, e.fill]);
            }
            gl.uniform_4_f32_slice(loc("u_rect[0]").as_ref(), &rect);
            gl.uniform_4_f32_slice(loc("u_prop[0]").as_ref(), &prop);
            gl.uniform_4_f32_slice(loc("u_fx[0]").as_ref(), &fx);
            gl.uniform_2_f32(loc("u_light").as_ref(), f.light[0], f.light[1]);
            gl.uniform_3_f32(loc("u_ptr").as_ref(), f.pointer[0], f.pointer[1], f.pointer[2]);
            let t = f.tree_rect;
            gl.uniform_4_f32(loc("u_tree_rect").as_ref(), t[0], t[1], t[2], t[3]);
            gl.uniform_1_f32(loc("u_has_tree").as_ref(), if self.has_tree { 1.0 } else { 0.0 });
            gl.uniform_1_i32(loc("u_overlay").as_ref(), f.overlay as i32);
            gl.uniform_1_f32(loc("u_dim").as_ref(), f.dim);
            gl.uniform_1_f32(loc("u_fs").as_ref(), f.fs);
            let pal: Vec<f32> = f.palette.iter().flatten().copied().collect();
            gl.uniform_3_f32_slice(loc("u_pal[0]").as_ref(), &pal);

            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            gl.enable(glow::BLEND);
        }
    }
}
