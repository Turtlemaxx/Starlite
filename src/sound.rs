//! UI sounds, synthesized at startup (no audio files): the click (Classic:
//! a short glassy "tick"; Soft: a rounded, woody tap), slider ticks and the
//! shelf's page turn. The click can also be your own sound file, or off.

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle};

const RATE: u32 = 48_000;

pub struct Sound {
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    /// The click in use: channels, sample rate, samples (empty: off).
    click: (u16, u32, Vec<f32>),
    page: Vec<f32>,
}

/// The click sound to use.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Default)]
pub enum ClickSound {
    #[default]
    Classic,
    Soft,
    /// A sound file you picked.
    Custom,
    Off,
}

impl ClickSound {
    pub const ALL: [ClickSound; 4] = [ClickSound::Classic, ClickSound::Soft, ClickSound::Custom, ClickSound::Off];
    pub fn label(self) -> &'static str {
        match self {
            ClickSound::Classic => "Classic",
            ClickSound::Soft => "Soft",
            ClickSound::Custom => "Your own",
            ClickSound::Off => "Off",
        }
    }
}

/// The longest a click file may play (seconds).
const CUSTOM_MAX_S: f32 = 2.0;

/// Reads a sound file for the click: at most two seconds, levelled so it's
/// about as loud as the built-in ones.
pub fn load_click(path: &std::path::Path) -> Result<(u16, u32, Vec<f32>), String> {
    use rodio::Source;
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read it ({e})"))?;
    let dec = rodio::Decoder::new(std::io::Cursor::new(bytes)).map_err(|_| "That isn't a sound file Starlite can play".to_string())?;
    let (ch, rate) = (dec.channels().max(1), dec.sample_rate().max(1));
    let max = (CUSTOM_MAX_S * rate as f32) as usize * ch as usize;
    let mut s: Vec<f32> = dec.convert_samples::<f32>().take(max).collect();
    let peak = s.iter().fold(0f32, |m, x| m.max(x.abs()));
    if s.is_empty() || peak < 1e-4 {
        return Err("That sound is silent".into());
    }
    // A soft fade at the end if it was cut short.
    let fade = (rate as usize / 100 * ch as usize).min(s.len());
    let n = s.len();
    for (i, x) in s[n - fade..].iter_mut().enumerate() {
        *x *= 1.0 - i as f32 / fade as f32;
    }
    let k = 0.22 / peak;
    s.iter_mut().for_each(|x| *x *= k);
    Ok((ch, rate, s))
}

/// Soft: a rounded, woody tap (a low partial that drops a little in pitch,
/// a gentle second one, a quiet thump).
fn synth_soft() -> Vec<f32> {
    let n = (RATE as f32 * 0.08) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let attack = (t / 0.0012).min(1.0);
            let f = 640.0 * (1.0 - 0.18 * (t / 0.08));
            let body = (std::f32::consts::TAU * f * t).sin() * (-t * 75.0).exp();
            let over = 0.22 * (std::f32::consts::TAU * f * 2.4 * t).sin() * (-t * 140.0).exp();
            let thump = 0.35 * (std::f32::consts::TAU * 170.0 * t).sin() * (-t * 60.0).exp();
            (body + over + thump) * attack * 0.12
        })
        .collect()
}

fn synth_click() -> Vec<f32> {
    let n = (RATE as f32 * 0.05) as usize;
    let mut seed: u32 = 0x1234_5678;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32 - 0.5) * (-t * 1400.0).exp();
            let tone = (std::f32::consts::TAU * 2350.0 * t).sin() * (-t * 110.0).exp()
                + 0.35 * (std::f32::consts::TAU * 3900.0 * t).sin() * (-t * 170.0).exp();
            (tone * 0.55 + noise * 0.6) * 0.16
        })
        .collect()
}

/// Turning a shelf page: a soft airy swish (noise through a filter that
/// opens up) under a quick, mellow two-note chime going up.
fn synth_page() -> Vec<f32> {
    let n = (RATE as f32 * 0.42) as usize;
    let mut seed: u32 = 0x9e37_79b9;
    let mut lp = 0.0f32;
    let mut lp2 = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let white = seed as f32 / u32::MAX as f32 - 0.5;
            // Band of noise sweeping upwards: low-pass with a rising cutoff, minus a slower one.
            let cut = 0.02 + 0.10 * (t / 0.16).min(1.0);
            lp += (white - lp) * cut;
            lp2 += (lp - lp2) * 0.015;
            let swish = (lp - lp2) * (t / 0.025).min(1.0) * (-t * 16.0).exp() * 2.2;
            let note = |f: f32, at: f32| {
                let u = t - at;
                if u < 0.0 {
                    return 0.0;
                }
                let env = (u / 0.004).min(1.0) * (-u * 14.0).exp();
                ((std::f32::consts::TAU * f * u).sin() + 0.18 * (std::f32::consts::TAU * f * 2.0 * u).sin()) * env
            };
            let chime = note(1046.5, 0.0) * 0.55 + note(1568.0, 0.055) * 0.45;
            (swish * 0.5 + chime * 0.32) * 0.2
        })
        .collect()
}

impl Sound {
    pub fn new() -> Self {
        let (stream, handle) = match OutputStream::try_default() {
            Ok((s, h)) => (Some(s), Some(h)),
            Err(_) => (None, None),
        };
        Self { _stream: stream, handle, click: (1, RATE, synth_click()), page: synth_page() }
    }

    /// Picks the click sound. A file that can't be used leaves the click as
    /// it was and says why.
    pub fn set_click(&mut self, kind: ClickSound, file: Option<&std::path::Path>) -> Result<(), String> {
        self.click = match kind {
            ClickSound::Classic => (1, RATE, synth_click()),
            ClickSound::Soft => (1, RATE, synth_soft()),
            ClickSound::Off => (1, RATE, Vec::new()),
            ClickSound::Custom => load_click(file.ok_or("Choose a sound file first")?)?,
        };
        Ok(())
    }

    pub fn click(&self) {
        let (ch, rate, s) = &self.click;
        if let (Some(h), false) = (&self.handle, s.is_empty()) {
            let _ = h.play_raw(SamplesBuffer::new(*ch, *rate, s.clone()));
        }
    }

    /// A slider moving one step: a tiny soft tick, higher for higher values
    /// (`at` 0..1 along the slider).
    pub fn tick(&self, at: f32) {
        let f = 900.0 * 2f32.powf(at.clamp(0.0, 1.0) * 1.3);
        let n = (RATE as f32 * 0.03) as usize;
        let s: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let env = (t / 0.0015).min(1.0) * (-t * 190.0).exp();
                ((std::f32::consts::TAU * f * t).sin() + 0.25 * (std::f32::consts::TAU * f * 2.01 * t).sin()) * env * 0.07
            })
            .collect();
        self.play(&s);
    }

    /// Turning a page on the CD shelf.
    pub fn page(&self) {
        self.play(&self.page);
    }

    fn play(&self, s: &[f32]) {
        if let Some(h) = &self.handle {
            let _ = h.play_raw(SamplesBuffer::new(1, RATE, s.to_vec()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_soft_click_is_short_and_quiet() {
        let s = synth_soft();
        assert!(s.len() < RATE as usize / 10);
        let peak = s.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.05 && peak < 0.3, "{peak}");
    }

    #[test]
    fn own_click_files_are_levelled_and_cut_short() {
        let dir = std::env::temp_dir().join(format!("starlite-click-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Five seconds of a loud tone as a 16-bit WAV.
        let rate = 22_050u32;
        let n = rate as usize * 5;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend(&(36 + n as u32 * 2).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(&16u32.to_le_bytes());
        wav.extend(&1u16.to_le_bytes());
        wav.extend(&1u16.to_le_bytes());
        wav.extend(&rate.to_le_bytes());
        wav.extend(&(rate * 2).to_le_bytes());
        wav.extend(&2u16.to_le_bytes());
        wav.extend(&16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(&(n as u32 * 2).to_le_bytes());
        for i in 0..n {
            let v = ((std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin() * 30_000.0) as i16;
            wav.extend(&v.to_le_bytes());
        }
        let f = dir.join("loud.wav");
        std::fs::write(&f, wav).unwrap();
        let (ch, r, s) = load_click(&f).unwrap();
        assert_eq!((ch, r), (1, rate));
        assert_eq!(s.len(), (CUSTOM_MAX_S * rate as f32) as usize);
        let peak = s.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((peak - 0.22).abs() < 0.01, "{peak}");
        std::fs::write(dir.join("junk.wav"), b"not a sound").unwrap();
        assert!(load_click(&dir.join("junk.wav")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn page_sound_is_soft_and_short() {
        let s = super::synth_page();
        let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.02 && peak < 0.35, "peak {peak}");
        assert!(s.len() < super::RATE as usize / 2);
        assert!(s[s.len() - 200..].iter().all(|x| x.abs() < 0.01), "fades out");
    }
}
