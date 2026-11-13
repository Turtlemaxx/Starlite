//! The sound processing between a song and the speakers: the equalizer and
//! the effects, in this order: equalizer, bass, clarity, stereo widening,
//! crossfeed, headphone surround, reverb, compressor, output volume /
//! balance, limiter. It works on stereo frames at one fixed sample rate.
//!
//! With everything off, samples pass through bit for bit.

use std::collections::VecDeque;
use std::f32::consts::TAU;

use crate::fx::{FxPrefs, EQ_FREQS};

fn db(v: f32) -> f32 {
    10f32.powf(v / 20.0)
}

/// A second-order filter (transposed direct form II). It works in double
/// precision: bands near the bottom (31 Hz at 48 kHz and up) have
/// coefficients so close together that single precision adds noise.
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    c: [f64; 5],
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn set(&mut self, c: [f64; 5]) {
        self.c = c;
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let [b0, b1, b2, a1, a2] = self.c;
        let x = x as f64;
        let y = b0 * x + self.z1;
        self.z1 = b1 * x - a1 * y + self.z2;
        self.z2 = b2 * x - a2 * y;
        y as f32
    }
}

/// RBJ cookbook filters, as [b0, b1, b2, a1, a2] (a0 = 1).
pub fn peaking(gain_db: f32, f0: f32, q: f32, rate: f32) -> [f64; 5] {
    let a = 10f64.powf(gain_db as f64 / 40.0);
    let w0 = std::f64::consts::TAU * (f0.min(rate * 0.45) / rate) as f64;
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * q.max(0.05) as f64);
    let a0 = 1.0 + alpha / a;
    [(1.0 + alpha * a) / a0, -2.0 * cos / a0, (1.0 - alpha * a) / a0, -2.0 * cos / a0, (1.0 - alpha / a) / a0]
}

fn shelf(gain_db: f32, f0: f32, rate: f32, high: bool) -> [f64; 5] {
    let a = 10f64.powf(gain_db as f64 / 40.0);
    let w0 = std::f64::consts::TAU * (f0.min(rate * 0.45) / rate) as f64;
    let (sin, cos) = w0.sin_cos();
    let s = 0.8;
    let alpha = sin / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
    let sa = 2.0 * a.sqrt() * alpha;
    // The high shelf is the low shelf with the cosine terms flipped.
    let k = if high { -1.0 } else { 1.0 };
    let a0 = (a + 1.0) + k * (a - 1.0) * cos + sa;
    [
        a * ((a + 1.0) - k * (a - 1.0) * cos + sa) / a0,
        k * 2.0 * a * ((a - 1.0) - k * (a + 1.0) * cos) / a0,
        a * ((a + 1.0) - k * (a - 1.0) * cos - sa) / a0,
        -k * 2.0 * ((a - 1.0) + k * (a + 1.0) * cos) / a0,
        ((a + 1.0) + k * (a - 1.0) * cos - sa) / a0,
    ]
}

/// How loud (dB) a filter makes a tone at `freq`.
fn response_db(c: [f64; 5], freq: f32, rate: f32) -> f32 {
    // |H| at e^jw, worked out with complex numbers (the closed formula
    // cancels itself out at low frequencies).
    let w = std::f64::consts::TAU * (freq / rate) as f64;
    let (s1, c1) = w.sin_cos();
    let (s2, c2) = (2.0 * w).sin_cos();
    let [b0, b1, b2, a1, a2] = c;
    let (nr, ni) = (b0 + b1 * c1 + b2 * c2, -(b1 * s1 + b2 * s2));
    let (dr, di) = (1.0 + a1 * c1 + a2 * c2, -(a1 * s1 + a2 * s2));
    (10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di).max(1e-300)).max(1e-300).log10()) as f32
}

/// The equalizer's curve at `freq` (dB, preamp included), for drawing it.
pub fn eq_response(p: &FxPrefs, freq: f32) -> f32 {
    let rate = 48_000.0;
    p.preamp + p.eq.iter().zip(EQ_FREQS).filter(|(b, _)| b.gain.abs() > 0.01).map(|(b, f)| response_db(peaking(b.gain, f, b.q, rate), freq, rate)).sum::<f32>()
}

/// Soft limit: untouched below 0.85, eased into ±1 above.
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.85 {
        x
    } else {
        x.signum() * (0.85 + 0.15 * ((a - 0.85) / 0.15).tanh())
    }
}

#[derive(Clone, Copy, Default)]
struct OnePole {
    k: f32,
    y: f32,
}

impl OnePole {
    fn set(&mut self, cutoff: f32, rate: f32) {
        self.k = 1.0 - (-TAU * cutoff / rate).exp();
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        self.y += (x - self.y) * self.k;
        self.y
    }
}

/// A ring buffer to read the past from.
struct Delay {
    buf: Vec<f32>,
    at: usize,
}

impl Delay {
    fn new(len: usize) -> Delay {
        Delay { buf: vec![0.0; len.max(2)], at: 0 }
    }
    #[inline]
    fn push(&mut self, x: f32) {
        self.buf[self.at] = x;
        self.at = (self.at + 1) % self.buf.len();
    }
    /// The sample pushed `back` pushes ago (1 = the latest).
    #[inline]
    fn tap(&self, back: usize) -> f32 {
        let n = self.buf.len();
        self.buf[(self.at + n - back.clamp(1, n)) % n]
    }
}

/// One side of a Freeverb: eight damped combs into four all-passes.
struct Verb {
    combs: Vec<(Delay, f32)>,
    alls: Vec<Delay>,
}

impl Verb {
    fn new(rate: f32, spread: usize) -> Verb {
        let k = rate / 44_100.0;
        let len = |n: usize| ((n + spread) as f32 * k) as usize;
        Verb {
            combs: [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617].iter().map(|&n| (Delay::new(len(n)), 0.0)).collect(),
            alls: [556, 441, 341, 225].iter().map(|&n| Delay::new(len(n))).collect(),
        }
    }
    #[inline]
    fn run(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let mut out = 0.0;
        for (d, store) in &mut self.combs {
            let y = d.tap(d.buf.len());
            *store = y * (1.0 - damp) + *store * damp;
            d.push(x + *store * feedback);
            out += y;
        }
        for d in &mut self.alls {
            let y = d.tap(d.buf.len());
            d.push(out + y * 0.5);
            out = y - out;
        }
        out
    }
}

/// The whole chain. `set` takes new settings; `process` runs one frame.
pub struct Chain {
    rate: f32,
    p: FxPrefs,
    active: bool,
    pre: f32,
    eq: [[Biquad; 2]; 10],
    eq_used: [bool; 10],
    bass: [Biquad; 2],
    bass_head: f32,
    clarity: [Biquad; 2],
    clarity_head: f32,
    // Crossfeed.
    cf_lp: [OnePole; 2],
    cf_delay: [Delay; 2],
    cf_back: usize,
    // Headphone surround.
    su_lp: [OnePole; 3],
    su_delay: [Delay; 3],
    su_back: [usize; 3],
    // Reverb.
    verb: [Verb; 2],
    // Compressor: gain reduction in dB (0 or below).
    comp_env: f32,
    comp_k: [f32; 2],
    // Limiter.
    lim_delay: [Delay; 2],
    lim_look: usize,
    lim_need: VecDeque<(u64, f32)>,
    lim_n: u64,
    lim_gain: f32,
    lim_k: [f32; 2],
    out: [f32; 2],
}

impl Chain {
    pub fn new(rate: u32) -> Chain {
        let r = rate.max(8000) as f32;
        let ms = |v: f32| (r * v / 1000.0) as usize + 2;
        let mut c = Chain {
            rate: r,
            p: FxPrefs::default(),
            active: false,
            pre: 1.0,
            eq: [[Biquad::default(); 2]; 10],
            eq_used: [false; 10],
            bass: [Biquad::default(); 2],
            bass_head: 1.0,
            clarity: [Biquad::default(); 2],
            clarity_head: 1.0,
            cf_lp: [OnePole::default(); 2],
            cf_delay: [Delay::new(ms(1.0)), Delay::new(ms(1.0))],
            cf_back: 1,
            su_lp: [OnePole::default(); 3],
            su_delay: [Delay::new(ms(40.0)), Delay::new(ms(40.0)), Delay::new(ms(40.0))],
            su_back: [1; 3],
            verb: [Verb::new(r, 0), Verb::new(r, 23)],
            comp_env: 0.0,
            comp_k: [1.0; 2],
            lim_delay: [Delay::new(ms(5.0)), Delay::new(ms(5.0))],
            lim_look: (r * 0.005) as usize,
            lim_need: VecDeque::new(),
            lim_n: 0,
            lim_gain: 1.0,
            lim_k: [1.0; 2],
            out: [1.0; 2],
        };
        c.set(&FxPrefs::default());
        c
    }

    /// Takes new settings (filters keep their state, so changes don't click).
    pub fn set(&mut self, p: &FxPrefs) {
        let r = self.rate;
        self.p = *p;
        self.active = p.processing();
        self.pre = db(p.preamp);
        for (i, band) in p.eq.iter().enumerate() {
            self.eq_used[i] = p.eq_on && band.gain.abs() > 0.01;
            let c = peaking(band.gain, EQ_FREQS[i], band.q, r);
            self.eq[i][0].set(c);
            self.eq[i][1].set(c);
        }
        let c = shelf(p.bass, p.bass_freq, r, false);
        self.bass.iter_mut().for_each(|b| b.set(c));
        // Headroom for a boost, so it doesn't just run into the limit.
        self.bass_head = db(-p.bass.max(0.0) * 0.35);
        let c = shelf(p.clarity, p.clarity_freq, r, true);
        self.clarity.iter_mut().for_each(|b| b.set(c));
        self.clarity_head = db(-p.clarity.max(0.0) * 0.25);
        self.cf_lp.iter_mut().for_each(|l| l.set(p.crossfeed_cut, r));
        self.cf_back = ((r * 0.0003) as usize).max(1);
        // Surround: the side signal about 15 ms late, each side's echo in the other ear.
        let size = 0.6 + 0.9 * p.surround_size;
        self.su_back = [(r * 0.015 * size) as usize, (r * 0.009 * size) as usize, (r * 0.013 * size) as usize].map(|n| n.max(1));
        self.su_lp[0].set(5000.0, r);
        self.su_lp[1].set(3200.0, r);
        self.su_lp[2].set(3200.0, r);
        let k = |ms: f32| 1.0 - (-1.0 / (r * ms.max(0.1) / 1000.0)).exp();
        self.comp_k = [k(p.comp_attack), k(p.comp_release)];
        self.lim_k = [k(1.2), k(p.limiter_release)];
        let (g, pan) = (db(p.out_gain), p.pan.clamp(-1.0, 1.0));
        self.out = [g * (1.0 - pan).min(1.0), g * (1.0 + pan).min(1.0)];
    }

    /// One stereo frame through everything that's switched on.
    #[inline]
    pub fn process(&mut self, f: [f32; 2]) -> [f32; 2] {
        if !self.active {
            return f;
        }
        let p = self.p;
        let [mut l, mut r] = f;
        if p.eq_on {
            l *= self.pre;
            r *= self.pre;
            for i in 0..10 {
                if self.eq_used[i] {
                    l = self.eq[i][0].run(l);
                    r = self.eq[i][1].run(r);
                }
            }
        }
        if p.bass_on && p.bass.abs() > 0.01 {
            l = self.bass[0].run(l) * self.bass_head;
            r = self.bass[1].run(r) * self.bass_head;
        }
        if p.clarity_on && p.clarity > 0.01 {
            l = self.clarity[0].run(l) * self.clarity_head;
            r = self.clarity[1].run(r) * self.clarity_head;
        }
        if p.widen_on {
            let (m, s) = ((l + r) * 0.5, (l - r) * 0.5 * p.widen);
            l = m + s;
            r = m - s;
        }
        if p.crossfeed_on && p.crossfeed > 0.001 {
            // Each ear also hears the other side: quieter, duller and a touch late.
            let a = p.crossfeed * 0.6;
            self.cf_delay[0].push(self.cf_lp[0].run(l));
            self.cf_delay[1].push(self.cf_lp[1].run(r));
            let (fl, fr) = (self.cf_delay[1].tap(self.cf_back), self.cf_delay[0].tap(self.cf_back));
            let norm = 1.0 / (1.0 + a * 0.6);
            l = (l + a * fl) * norm;
            r = (r + a * fr) * norm;
        }
        if p.surround_on {
            let a = p.surround;
            self.su_delay[0].push(self.su_lp[0].run((l - r) * 0.5));
            self.su_delay[1].push(self.su_lp[1].run(l));
            self.su_delay[2].push(self.su_lp[2].run(r));
            let amb = self.su_delay[0].tap(self.su_back[0]);
            let (el, er) = (self.su_delay[2].tap(self.su_back[1]), self.su_delay[1].tap(self.su_back[2]));
            let norm = 1.0 / (1.0 + 0.35 * a);
            l = (l + a * (0.55 * amb + 0.30 * el)) * norm;
            r = (r + a * (-0.55 * amb + 0.30 * er)) * norm;
        }
        if p.reverb_on && p.reverb > 0.001 {
            let x = (l + r) * 0.015;
            let (fb, damp) = (0.70 + 0.28 * p.reverb_size, p.reverb_damp * 0.4);
            let (wl, wr) = (self.verb[0].run(x, fb, damp), self.verb[1].run(x, fb, damp));
            let (wet, dry) = (p.reverb * 3.0, 1.0 - p.reverb * 0.35);
            l = l * dry + wl * wet;
            r = r * dry + wr * wet;
        }
        if p.comp_on {
            let level = 20.0 * (l.abs().max(r.abs()) + 1e-9).log10();
            let over = level - p.comp_threshold;
            let slope = 1.0 / p.comp_ratio.max(1.0) - 1.0;
            // A soft knee 6 dB wide.
            let want = if over <= -3.0 {
                0.0
            } else if over < 3.0 {
                slope * (over + 3.0) * (over + 3.0) / 12.0
            } else {
                slope * over
            };
            let k = if want < self.comp_env { self.comp_k[0] } else { self.comp_k[1] };
            self.comp_env += (want - self.comp_env) * k;
            let g = db(self.comp_env + p.comp_makeup);
            l *= g;
            r *= g;
        }
        if p.out_on {
            l *= self.out[0];
            r *= self.out[1];
        }
        if p.limiter_on {
            // Looks 5 ms ahead, so the gain is already down when a peak arrives.
            let ceil = db(p.limiter_ceiling);
            let need = (ceil / l.abs().max(r.abs()).max(1e-9)).min(1.0);
            self.lim_n += 1;
            while self.lim_need.back().is_some_and(|&(_, v)| v >= need) {
                self.lim_need.pop_back();
            }
            self.lim_need.push_back((self.lim_n, need));
            while self.lim_need.front().is_some_and(|&(n, _)| n + self.lim_look as u64 <= self.lim_n) {
                self.lim_need.pop_front();
            }
            let target = self.lim_need.front().map_or(1.0, |&(_, v)| v);
            let k = if target < self.lim_gain { self.lim_k[0] } else { self.lim_k[1] };
            self.lim_gain += (target - self.lim_gain) * k;
            self.lim_delay[0].push(l);
            self.lim_delay[1].push(r);
            let back = self.lim_look + 1;
            l = (self.lim_delay[0].tap(back) * self.lim_gain).clamp(-ceil, ceil);
            r = (self.lim_delay[1].tap(back) * self.lim_gain).clamp(-ceil, ceil);
        } else {
            l = soft_clip(l);
            r = soft_clip(r);
        }
        [l, r]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn tone(freq: f32, amp: f32, secs: f32) -> Vec<[f32; 2]> {
        (0..(RATE as f32 * secs) as usize).map(|i| [(TAU * freq * i as f32 / RATE as f32).sin() * amp; 2]).collect()
    }

    fn run(p: &FxPrefs, input: &[[f32; 2]]) -> Vec<[f32; 2]> {
        let mut c = Chain::new(RATE);
        c.set(p);
        input.iter().map(|&f| c.process(f)).collect()
    }

    /// Loudness of the second half (after filters have settled).
    fn rms(x: &[[f32; 2]], ch: usize) -> f32 {
        let h = &x[x.len() / 2..];
        (h.iter().map(|f| f[ch] * f[ch]).sum::<f32>() / h.len() as f32).sqrt()
    }

    #[test]
    fn with_everything_off_nothing_changes() {
        let input: Vec<[f32; 2]> = (0..2000).map(|i| [(i as f32 * 0.37).sin(), (i as f32 * 0.11).cos() * 1.2]).collect();
        assert_eq!(run(&FxPrefs::default(), &input), input);
        // Switched on but set to "nothing" also leaves the sound alone.
        let p = FxPrefs { eq_on: true, bass_on: true, out_on: true, ..Default::default() };
        assert_eq!(run(&p, &input), input);
    }

    #[test]
    fn an_equalizer_band_moves_its_own_frequency() {
        let mut p = FxPrefs { eq_on: true, ..Default::default() };
        p.eq[5].gain = 9.0; // 1 kHz
        let (flat, up) = (rms(&tone(1000.0, 0.1, 0.5), 0), rms(&run(&p, &tone(1000.0, 0.1, 0.5)), 0));
        assert!((20.0 * (up / flat).log10() - 9.0).abs() < 0.3, "{}", 20.0 * (up / flat).log10());
        let far = rms(&run(&p, &tone(60.0, 0.1, 0.5)), 0);
        assert!((20.0 * (far / flat).log10()).abs() < 0.5);
        // Also right at the bottom, where the maths gets delicate.
        let mut low = FxPrefs { eq_on: true, ..Default::default() };
        low.eq[0].gain = 6.0; // 31 Hz
        assert!((eq_response(&low, 31.0) - 6.0).abs() < 0.05);
        assert!(eq_response(&low, 20.0) > 2.0 && eq_response(&low, 20.0) < 6.0);
        let (flat31, up31) = (rms(&tone(31.0, 0.1, 1.0), 0), rms(&run(&low, &tone(31.0, 0.1, 1.0)), 0));
        assert!((20.0 * (up31 / flat31).log10() - 6.0).abs() < 0.4, "{}", 20.0 * (up31 / flat31).log10());
        // The curve that's drawn says the same.
        assert!((eq_response(&p, 1000.0) - 9.0).abs() < 0.05);
        assert!(eq_response(&p, 60.0).abs() < 0.3);
        p.preamp = -3.0;
        assert!((eq_response(&p, 1000.0) - 6.0).abs() < 0.05);
        // A narrower band (higher Q) reaches less far.
        let wide = eq_response(&p, 2000.0);
        p.eq[5].q = 6.0;
        assert!(eq_response(&p, 2000.0) < wide - 1.0);
    }

    #[test]
    fn bass_lifts_the_lows_and_clarity_the_highs() {
        let p = FxPrefs { bass_on: true, bass: 10.0, ..Default::default() };
        let (low, high) = (tone(60.0, 0.1, 0.5), tone(3000.0, 0.1, 0.5));
        assert!(rms(&run(&p, &low), 0) > rms(&low, 0) * 1.5);
        let h = rms(&run(&p, &high), 0) / rms(&high, 0);
        assert!(h < 1.0 && h > 0.4, "{h}");
        let p = FxPrefs { bass_on: true, bass: -10.0, ..Default::default() };
        assert!(rms(&run(&p, &low), 0) < rms(&low, 0) * 0.6);
        let p = FxPrefs { clarity_on: true, clarity: 8.0, ..Default::default() };
        let top = tone(9000.0, 0.1, 0.5);
        assert!(rms(&run(&p, &top), 0) > rms(&top, 0) * 1.3);
        assert!(rms(&run(&p, &low), 0) < rms(&low, 0));
    }

    #[test]
    fn loud_bass_never_clips() {
        let p = FxPrefs { bass_on: true, bass: crate::fx::BASS_MAX, ..Default::default() };
        assert!(run(&p, &tone(50.0, 1.0, 1.0)).iter().all(|f| f[0].abs() <= 1.0));
    }

    #[test]
    fn width_goes_from_mono_to_wide() {
        let input: Vec<[f32; 2]> = (0..4000).map(|i| [(i as f32 * 0.05).sin() * 0.3, (i as f32 * 0.021).sin() * 0.3]).collect();
        let mono = run(&FxPrefs { widen_on: true, widen: 0.0, ..Default::default() }, &input);
        assert!(mono.iter().all(|f| (f[0] - f[1]).abs() < 1e-6));
        let side = |x: &[[f32; 2]]| x.iter().map(|f| (f[0] - f[1]).abs()).sum::<f32>();
        let wide = run(&FxPrefs { widen_on: true, widen: 1.8, ..Default::default() }, &input);
        assert!(side(&wide) > side(&input) * 1.5);
    }

    #[test]
    fn crossfeed_and_surround_reach_the_other_ear() {
        let left: Vec<[f32; 2]> = tone(300.0, 0.3, 0.3).iter().map(|f| [f[0], 0.0]).collect();
        let cf = run(&FxPrefs { crossfeed_on: true, ..Default::default() }, &left);
        assert!(rms(&cf, 1) > 0.01 && rms(&cf, 1) < rms(&cf, 0) * 0.6);
        let su = run(&FxPrefs { surround_on: true, ..Default::default() }, &left);
        assert!(rms(&su, 1) > 0.01);
    }

    #[test]
    fn reverb_rings_on_after_the_sound_stops() {
        let mut input = tone(440.0, 0.4, 0.2);
        input.extend(vec![[0.0; 2]; RATE as usize / 2]);
        let out = run(&FxPrefs { reverb_on: true, reverb: 0.4, ..Default::default() }, &input);
        let tail = &out[(RATE as f32 * 0.3) as usize..(RATE as f32 * 0.5) as usize];
        assert!(tail.iter().any(|f| f[0].abs() > 0.002));
        assert!(out.iter().all(|f| f[0].abs() <= 1.0));
    }

    #[test]
    fn the_compressor_turns_loud_parts_down() {
        let p = FxPrefs { comp_on: true, comp_threshold: -20.0, comp_ratio: 4.0, comp_makeup: 0.0, ..Default::default() };
        let (loud, quiet) = (tone(500.0, 0.8, 0.5), tone(500.0, 0.02, 0.5));
        // 0.8 is 18 dB over the threshold: 4:1 leaves 4.5 dB of it, 13.5 dB less.
        let cut = 20.0 * (rms(&run(&p, &loud), 0) / rms(&loud, 0)).log10();
        assert!(cut < -11.0 && cut > -15.0, "{cut}");
        assert!((rms(&run(&p, &quiet), 0) / rms(&quiet, 0) - 1.0).abs() < 0.02);
    }

    #[test]
    fn the_limiter_holds_its_ceiling() {
        let p = FxPrefs { limiter_on: true, limiter_ceiling: -6.0, ..Default::default() };
        let out = run(&p, &tone(200.0, 1.0, 0.5));
        let ceil = db(-6.0);
        assert!(out.iter().all(|f| f[0].abs() <= ceil + 1e-5));
        // It turns down rather than chops: the peak sits right at the ceiling.
        let peak = out[out.len() / 2..].iter().fold(0f32, |m, f| m.max(f[0].abs()));
        assert!(peak > ceil * 0.9, "{peak}");
        // Quiet sound comes through as it was (5 ms later).
        let quiet = tone(200.0, 0.1, 0.2);
        let out = run(&p, &quiet);
        let look = (RATE as f32 * 0.005) as usize;
        assert!((out[5000][0] - quiet[5000 - look][0]).abs() < 1e-4);
    }

    #[test]
    fn output_volume_and_balance() {
        let p = FxPrefs { out_on: true, out_gain: -6.0, pan: -1.0, ..Default::default() };
        let out = run(&p, &tone(400.0, 0.4, 0.1));
        assert!((rms(&out, 0) / (0.4 / 2f32.sqrt()) - db(-6.0)).abs() < 0.01);
        assert!(rms(&out, 1) < 1e-6);
    }
}
