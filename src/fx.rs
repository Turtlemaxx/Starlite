//! Sound settings from the player bar's sliders: playback speed and bass.
//!
//! Speed changes the playback rate (pitch moves with it, like a record).
//! Bass is a low-shelf filter (around 110 Hz) on Starlite's own playback,
//! with some headroom and a soft limit so a boost doesn't clip. YouTube
//! songs play in YouTube's player, so they get the speed but not the bass.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const SPEED_MIN: f32 = 0.5;
pub const SPEED_MAX: f32 = 2.0;
pub const BASS_MIN: f32 = -12.0;
pub const BASS_MAX: f32 = 24.0;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct FxPrefs {
    pub speed: f32,
    /// dB at the low end.
    pub bass: f32,
}

impl Default for FxPrefs {
    fn default() -> Self {
        Self { speed: 1.0, bass: 0.0 }
    }
}

impl FxPrefs {
    fn file() -> std::path::PathBuf {
        crate::library::data_dir().join("sound.json")
    }
    pub fn load() -> Self {
        let p: Self = std::fs::read_to_string(Self::file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { speed: p.speed.clamp(SPEED_MIN, SPEED_MAX), bass: p.bass.clamp(BASS_MIN, BASS_MAX) }
    }
    pub fn save(&self) {
        let f = Self::file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(f, s);
        }
    }
    pub fn is_default(&self) -> bool {
        (self.speed - 1.0).abs() < 0.001 && self.bass.abs() < 0.05
    }
}

/// The bass setting shared with the audio thread (dB as f32 bits).
#[derive(Clone, Default)]
pub struct BassKnob(Arc<AtomicU32>);

impl BassKnob {
    pub fn set(&self, db: f32) {
        self.0.store(db.to_bits(), Ordering::Relaxed);
    }
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

#[derive(Clone, Copy, Default)]
struct Coefs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// RBJ low shelf at `f0` with `db` gain.
fn low_shelf(db: f32, f0: f32, rate: f32) -> Coefs {
    let a = 10f32.powf(db / 40.0);
    let w0 = std::f32::consts::TAU * f0 / rate;
    let (sin, cos) = w0.sin_cos();
    let s = 0.8;
    let alpha = sin / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
    let sa = 2.0 * a.sqrt() * alpha;
    let a0 = (a + 1.0) + (a - 1.0) * cos + sa;
    Coefs {
        b0: a * ((a + 1.0) - (a - 1.0) * cos + sa) / a0,
        b1: 2.0 * a * ((a - 1.0) - (a + 1.0) * cos) / a0,
        b2: a * ((a + 1.0) - (a - 1.0) * cos - sa) / a0,
        a1: -2.0 * ((a - 1.0) + (a + 1.0) * cos) / a0,
        a2: ((a + 1.0) + (a - 1.0) * cos - sa) / a0,
    }
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

/// The bass filter around a song.
pub struct Bass<S> {
    inner: S,
    knob: BassKnob,
    ch: usize,
    rate: f32,
    /// The gain in use; glides to the knob so changes don't click.
    db: f32,
    c: Coefs,
    state: Vec<[f32; 4]>,
    i: usize,
    n: u32,
}

impl<S: rodio::Source<Item = f32>> Bass<S> {
    pub fn new(inner: S, knob: BassKnob) -> Self {
        let ch = inner.channels().max(1) as usize;
        let rate = inner.sample_rate().max(1) as f32;
        let db = knob.get();
        Bass { c: low_shelf(db, 110.0, rate), inner, knob, ch, rate, db, state: vec![[0.0; 4]; ch], i: 0, n: 0 }
    }
}

impl<S: rodio::Source<Item = f32>> Iterator for Bass<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let x = self.inner.next()?;
        // Every 256 samples, move the gain a little towards the knob.
        self.n += 1;
        if self.n >= 256 {
            self.n = 0;
            let want = self.knob.get();
            if (want - self.db).abs() > 0.001 {
                self.db += (want - self.db).clamp(-0.25, 0.25);
                self.c = low_shelf(self.db, 110.0, self.rate);
            }
        }
        let ch = self.i % self.ch;
        self.i = self.i.wrapping_add(1);
        if self.db.abs() < 0.01 {
            return Some(x);
        }
        let s = &mut self.state[ch];
        let c = self.c;
        let y = c.b0 * x + c.b1 * s[0] + c.b2 * s[1] - c.a1 * s[2] - c.a2 * s[3];
        *s = [x, s[0], y, s[2]];
        // Headroom for a boost, then a soft limit.
        let head = 10f32.powf(-self.db.max(0.0) * 0.35 / 20.0);
        Some(soft_clip(y * head))
    }
}

impl<S: rodio::Source<Item = f32>> rodio::Source for Bass<S> {
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }
    fn channels(&self) -> u16 {
        self.ch as u16
    }
    fn sample_rate(&self) -> u32 {
        self.rate as u32
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.state.iter_mut().for_each(|s| *s = [0.0; 4]);
        self.i = 0;
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    fn rms_through(db: f32, freq: f32) -> f32 {
        let rate = 44_100u32;
        let tone: Vec<f32> = (0..rate).map(|i| (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin() * 0.25).collect();
        let knob = BassKnob::default();
        knob.set(db);
        let out: Vec<f32> = Bass::new(SamplesBuffer::new(1, rate, tone), knob).skip(rate as usize / 2).collect();
        (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt()
    }

    #[test]
    fn boost_lifts_the_lows_not_the_highs() {
        let flat_low = rms_through(0.0, 60.0);
        let boosted_low = rms_through(10.0, 60.0);
        let boosted_high = rms_through(10.0, 3000.0);
        let flat_high = rms_through(0.0, 3000.0);
        assert!(boosted_low > flat_low * 1.5, "{boosted_low} vs {flat_low}");
        // The highs only get the headroom cut, a few dB down.
        assert!(boosted_high < flat_high && boosted_high > flat_high * 0.4, "{boosted_high} vs {flat_high}");
    }

    #[test]
    fn cut_lowers_the_lows() {
        assert!(rms_through(-10.0, 60.0) < rms_through(0.0, 60.0) * 0.6);
    }

    #[test]
    fn loud_input_never_clips() {
        let rate = 44_100u32;
        let tone: Vec<f32> = (0..rate).map(|i| (std::f32::consts::TAU * 50.0 * i as f32 / rate as f32).sin()).collect();
        let knob = BassKnob::default();
        knob.set(BASS_MAX);
        assert!(Bass::new(SamplesBuffer::new(1, rate, tone), knob).all(|x| x.abs() <= 1.0));
    }
}
