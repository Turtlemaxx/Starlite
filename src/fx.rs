//! Sound settings (sound.json): playback speed, crossfade, the 10-band
//! equalizer and the effects. Everything except speed is off until it's
//! switched on, and with everything off the sound passes through untouched.
//! The processing itself is in `dsp.rs`.

use serde::{Deserialize, Serialize};

pub const SPEED_MIN: f32 = 0.5;
pub const SPEED_MAX: f32 = 2.0;
pub const BASS_MIN: f32 = -12.0;
pub const BASS_MAX: f32 = 24.0;
/// Longest crossfade (seconds); 0 is gapless.
pub const CROSSFADE_MAX: f32 = 12.0;

/// Centre frequencies of the equalizer's bands (Hz).
pub const EQ_FREQS: [f32; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
pub const EQ_GAIN: f32 = 12.0;
pub const Q_MIN: f32 = 0.3;
pub const Q_MAX: f32 = 8.0;
pub const Q_DEFAULT: f32 = 1.41;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct Band {
    /// dB.
    pub gain: f32,
    /// Width: low is wide, high is narrow.
    pub q: f32,
}

impl Default for Band {
    fn default() -> Self {
        Band { gain: 0.0, q: Q_DEFAULT }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct FxPrefs {
    pub speed: f32,
    /// Seconds two songs overlap (0: gapless, one right after the other).
    pub crossfade: f32,

    pub eq_on: bool,
    /// dB before the bands.
    pub preamp: f32,
    pub eq: [Band; 10],

    pub bass_on: bool,
    /// dB at the low end.
    pub bass: f32,
    pub bass_freq: f32,

    pub clarity_on: bool,
    /// dB at the high end.
    pub clarity: f32,
    pub clarity_freq: f32,

    pub surround_on: bool,
    pub surround: f32,
    pub surround_size: f32,

    pub widen_on: bool,
    /// 0 mono, 1 as recorded, 2 extra wide.
    pub widen: f32,

    pub crossfeed_on: bool,
    pub crossfeed: f32,
    pub crossfeed_cut: f32,

    pub reverb_on: bool,
    pub reverb: f32,
    pub reverb_size: f32,
    pub reverb_damp: f32,

    pub comp_on: bool,
    pub comp_threshold: f32,
    pub comp_ratio: f32,
    pub comp_attack: f32,
    pub comp_release: f32,
    pub comp_makeup: f32,

    pub limiter_on: bool,
    pub limiter_ceiling: f32,
    pub limiter_release: f32,

    pub out_on: bool,
    pub out_gain: f32,
    pub pan: f32,
}

impl Default for FxPrefs {
    fn default() -> Self {
        Self {
            speed: 1.0,
            crossfade: 0.0,
            eq_on: false,
            preamp: 0.0,
            eq: [Band::default(); 10],
            bass_on: false,
            bass: 0.0,
            bass_freq: 110.0,
            clarity_on: false,
            clarity: 4.0,
            clarity_freq: 5000.0,
            surround_on: false,
            surround: 0.5,
            surround_size: 0.5,
            widen_on: false,
            widen: 1.4,
            crossfeed_on: false,
            crossfeed: 0.5,
            crossfeed_cut: 700.0,
            reverb_on: false,
            reverb: 0.2,
            reverb_size: 0.5,
            reverb_damp: 0.5,
            comp_on: false,
            comp_threshold: -18.0,
            comp_ratio: 3.0,
            comp_attack: 10.0,
            comp_release: 150.0,
            comp_makeup: 3.0,
            limiter_on: false,
            limiter_ceiling: -1.0,
            limiter_release: 80.0,
            out_on: false,
            out_gain: 0.0,
            pan: 0.0,
        }
    }
}

/// One adjustable value of an effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum P {
    Bass,
    BassFreq,
    Clarity,
    ClarityFreq,
    Surround,
    SurroundSize,
    Widen,
    Crossfeed,
    CrossfeedCut,
    Reverb,
    ReverbSize,
    ReverbDamp,
    CompThreshold,
    CompRatio,
    CompAttack,
    CompRelease,
    CompMakeup,
    LimiterCeiling,
    LimiterRelease,
    OutGain,
    Pan,
}

/// How a value is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Db,
    Hz,
    Percent,
    Ms,
    Ratio,
    Pan,
}

pub struct Param {
    pub p: P,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub unit: Unit,
}

pub struct Effect {
    pub name: &'static str,
    pub about: &'static str,
    pub params: &'static [Param],
}

const fn par(p: P, label: &'static str, min: f32, max: f32, step: f32, unit: Unit) -> Param {
    Param { p, label, min, max, step, unit }
}

/// The effects, in the order the sound goes through them.
pub const EFFECTS: [Effect; 9] = [
    Effect { name: "Bass", about: "Lifts or lowers the low end", params: &[par(P::Bass, "Amount", BASS_MIN, BASS_MAX, 0.5, Unit::Db), par(P::BassFreq, "Up to", 60.0, 250.0, 5.0, Unit::Hz)] },
    Effect { name: "Clarity", about: "Brings out voices and detail", params: &[par(P::Clarity, "Amount", 0.0, 12.0, 0.5, Unit::Db), par(P::ClarityFreq, "From", 2000.0, 10000.0, 100.0, Unit::Hz)] },
    Effect { name: "Stereo widening", about: "Spreads the sound further left and right", params: &[par(P::Widen, "Width", 0.0, 2.0, 0.05, Unit::Percent)] },
    Effect { name: "Crossfeed", about: "Headphones: a little of each side in the other ear, like speakers", params: &[par(P::Crossfeed, "Amount", 0.0, 1.0, 0.05, Unit::Percent), par(P::CrossfeedCut, "Below", 300.0, 2000.0, 50.0, Unit::Hz)] },
    Effect { name: "Headphone surround", about: "Moves the sound out of your head into a room", params: &[par(P::Surround, "Amount", 0.0, 1.0, 0.05, Unit::Percent), par(P::SurroundSize, "Room", 0.0, 1.0, 0.05, Unit::Percent)] },
    Effect {
        name: "Reverb",
        about: "Adds the echo of a room or hall",
        params: &[par(P::Reverb, "Mix", 0.0, 1.0, 0.05, Unit::Percent), par(P::ReverbSize, "Size", 0.0, 1.0, 0.05, Unit::Percent), par(P::ReverbDamp, "Damping", 0.0, 1.0, 0.05, Unit::Percent)],
    },
    Effect {
        name: "Compressor",
        about: "Evens out loud and quiet parts",
        params: &[
            par(P::CompThreshold, "Threshold", -40.0, 0.0, 1.0, Unit::Db),
            par(P::CompRatio, "Ratio", 1.0, 12.0, 0.5, Unit::Ratio),
            par(P::CompAttack, "Attack", 1.0, 100.0, 1.0, Unit::Ms),
            par(P::CompRelease, "Release", 20.0, 1000.0, 10.0, Unit::Ms),
            par(P::CompMakeup, "Make-up", 0.0, 18.0, 0.5, Unit::Db),
        ],
    },
    Effect { name: "Output", about: "Volume and left / right balance after everything else", params: &[par(P::OutGain, "Volume", -18.0, 12.0, 0.5, Unit::Db), par(P::Pan, "Balance", -1.0, 1.0, 0.05, Unit::Pan)] },
    Effect { name: "Limiter", about: "Stops the sound from going over a ceiling", params: &[par(P::LimiterCeiling, "Ceiling", -12.0, 0.0, 0.5, Unit::Db), par(P::LimiterRelease, "Release", 10.0, 500.0, 10.0, Unit::Ms)] },
];

impl FxPrefs {
    fn file() -> std::path::PathBuf {
        crate::library::data_dir().join("sound.json")
    }

    pub fn load() -> Self {
        let text = std::fs::read_to_string(Self::file()).unwrap_or_default();
        let mut p: Self = serde_json::from_str(&text).unwrap_or_default();
        // Files from before the effects had switches: a bass setting meant "on".
        let has_switch = serde_json::from_str::<serde_json::Value>(&text).is_ok_and(|v| v.get("bass_on").is_some());
        if !has_switch && p.bass.abs() > 0.05 {
            p.bass_on = true;
        }
        p.clamped()
    }

    /// Every value inside its range (a hand-edited file can't break the sound).
    pub fn clamped(mut self) -> Self {
        self.speed = fin(self.speed, 1.0).clamp(SPEED_MIN, SPEED_MAX);
        self.crossfade = fin(self.crossfade, 0.0).clamp(0.0, CROSSFADE_MAX);
        self.preamp = fin(self.preamp, 0.0).clamp(-EQ_GAIN, EQ_GAIN);
        for b in &mut self.eq {
            b.gain = fin(b.gain, 0.0).clamp(-EQ_GAIN, EQ_GAIN);
            b.q = fin(b.q, Q_DEFAULT).clamp(Q_MIN, Q_MAX);
        }
        let d = FxPrefs::default();
        for e in &EFFECTS {
            for par in e.params {
                let v = fin(self.get(par.p), d.get(par.p)).clamp(par.min, par.max);
                self.set(par.p, v);
            }
        }
        self
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

    pub fn get(&self, p: P) -> f32 {
        match p {
            P::Bass => self.bass,
            P::BassFreq => self.bass_freq,
            P::Clarity => self.clarity,
            P::ClarityFreq => self.clarity_freq,
            P::Surround => self.surround,
            P::SurroundSize => self.surround_size,
            P::Widen => self.widen,
            P::Crossfeed => self.crossfeed,
            P::CrossfeedCut => self.crossfeed_cut,
            P::Reverb => self.reverb,
            P::ReverbSize => self.reverb_size,
            P::ReverbDamp => self.reverb_damp,
            P::CompThreshold => self.comp_threshold,
            P::CompRatio => self.comp_ratio,
            P::CompAttack => self.comp_attack,
            P::CompRelease => self.comp_release,
            P::CompMakeup => self.comp_makeup,
            P::LimiterCeiling => self.limiter_ceiling,
            P::LimiterRelease => self.limiter_release,
            P::OutGain => self.out_gain,
            P::Pan => self.pan,
        }
    }

    pub fn set(&mut self, p: P, v: f32) {
        let slot = match p {
            P::Bass => &mut self.bass,
            P::BassFreq => &mut self.bass_freq,
            P::Clarity => &mut self.clarity,
            P::ClarityFreq => &mut self.clarity_freq,
            P::Surround => &mut self.surround,
            P::SurroundSize => &mut self.surround_size,
            P::Widen => &mut self.widen,
            P::Crossfeed => &mut self.crossfeed,
            P::CrossfeedCut => &mut self.crossfeed_cut,
            P::Reverb => &mut self.reverb,
            P::ReverbSize => &mut self.reverb_size,
            P::ReverbDamp => &mut self.reverb_damp,
            P::CompThreshold => &mut self.comp_threshold,
            P::CompRatio => &mut self.comp_ratio,
            P::CompAttack => &mut self.comp_attack,
            P::CompRelease => &mut self.comp_release,
            P::CompMakeup => &mut self.comp_makeup,
            P::LimiterCeiling => &mut self.limiter_ceiling,
            P::LimiterRelease => &mut self.limiter_release,
            P::OutGain => &mut self.out_gain,
            P::Pan => &mut self.pan,
        };
        *slot = v;
    }

    /// The switch of effect `i` (in `EFFECTS`).
    pub fn on(&self, i: usize) -> bool {
        [self.bass_on, self.clarity_on, self.widen_on, self.crossfeed_on, self.surround_on, self.reverb_on, self.comp_on, self.out_on, self.limiter_on][i]
    }

    pub fn set_on(&mut self, i: usize, on: bool) {
        *[&mut self.bass_on, &mut self.clarity_on, &mut self.widen_on, &mut self.crossfeed_on, &mut self.surround_on, &mut self.reverb_on, &mut self.comp_on, &mut self.out_on, &mut self.limiter_on][i] = on;
    }

    /// Is any processing switched on (and actually doing something)?
    pub fn processing(&self) -> bool {
        (self.eq_on && (self.preamp.abs() > 0.01 || self.eq.iter().any(|b| b.gain.abs() > 0.01)))
            || (self.bass_on && self.bass.abs() > 0.01)
            || (self.clarity_on && self.clarity > 0.01)
            || self.surround_on
            || (self.widen_on && (self.widen - 1.0).abs() > 0.001)
            || (self.crossfeed_on && self.crossfeed > 0.001)
            || (self.reverb_on && self.reverb > 0.001)
            || self.comp_on
            || self.limiter_on
            || (self.out_on && (self.out_gain.abs() > 0.01 || self.pan.abs() > 0.001))
    }

    /// Nothing changes the sound (the player bar's sliders button lights up otherwise).
    pub fn is_default(&self) -> bool {
        (self.speed - 1.0).abs() < 0.001 && !self.processing()
    }
}

fn fin(v: f32, or: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        or
    }
}

/// A value as text: "+4 dB", "110 Hz", "140 %", "3 : 1", "L 20".
pub fn show(unit: Unit, v: f32) -> String {
    match unit {
        Unit::Db => {
            let r = (v * 10.0).round() / 10.0;
            let s = if r > 0.0 { "+" } else if r < 0.0 { "\u{2212}" } else { "" };
            if (r.abs() - r.abs().round()).abs() < 0.05 {
                format!("{s}{:.0} dB", r.abs())
            } else {
                format!("{s}{:.1} dB", r.abs())
            }
        }
        Unit::Hz if v >= 1000.0 => {
            let k = v / 1000.0;
            if (k - k.round()).abs() < 0.05 {
                format!("{k:.0} kHz")
            } else {
                format!("{k:.1} kHz")
            }
        }
        Unit::Hz => format!("{v:.0} Hz"),
        Unit::Percent => format!("{:.0} %", v * 100.0),
        Unit::Ms => format!("{v:.0} ms"),
        Unit::Ratio => {
            if (v - v.round()).abs() < 0.05 {
                format!("{v:.0} : 1")
            } else {
                format!("{v:.1} : 1")
            }
        }
        Unit::Pan => {
            let n = (v * 100.0).round();
            if n == 0.0 {
                "Centre".into()
            } else if n < 0.0 {
                format!("Left {:.0}", -n)
            } else {
                format!("Right {n:.0}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_off_at_first() {
        let p = FxPrefs::default();
        assert!(p.is_default() && !p.processing());
        assert!((0..EFFECTS.len()).all(|i| !p.on(i)));
    }

    #[test]
    fn switches_and_values_line_up_with_the_table() {
        let mut p = FxPrefs::default();
        for i in 0..EFFECTS.len() {
            p.set_on(i, true);
            assert!(p.on(i), "{}", EFFECTS[i].name);
            for par in EFFECTS[i].params {
                p.set(par.p, par.max);
                assert_eq!(p.get(par.p), par.max);
                let d = FxPrefs::default().get(par.p);
                assert!(d >= par.min && d <= par.max, "{} {}", EFFECTS[i].name, par.label);
            }
        }
        assert!(p.limiter_on && p.out_on && p.comp_on);
    }

    #[test]
    fn old_files_keep_their_bass_and_bad_values_are_fixed() {
        let old: FxPrefs = serde_json::from_str(r#"{"speed": 1.25, "bass": 6.0}"#).unwrap();
        assert_eq!((old.speed, old.bass, old.eq_on), (1.25, 6.0, false));
        let wild = FxPrefs { speed: 9.0, preamp: f32::NAN, comp_ratio: 0.0, ..Default::default() }.clamped();
        assert_eq!((wild.speed, wild.preamp, wild.comp_ratio), (SPEED_MAX, 0.0, 1.0));
    }

    #[test]
    fn values_read_well() {
        assert_eq!(show(Unit::Db, 4.0), "+4 dB");
        assert_eq!(show(Unit::Db, -1.5), "\u{2212}1.5 dB");
        assert_eq!(show(Unit::Db, 0.0), "0 dB");
        assert_eq!(show(Unit::Hz, 110.0), "110 Hz");
        assert_eq!(show(Unit::Hz, 5000.0), "5 kHz");
        assert_eq!(show(Unit::Percent, 1.4), "140 %");
        assert_eq!(show(Unit::Ratio, 3.0), "3 : 1");
        assert_eq!(show(Unit::Pan, -0.2), "Left 20");
        assert_eq!(show(Unit::Pan, 0.0), "Centre");
    }
}
