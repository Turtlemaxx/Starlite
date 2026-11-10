//! UI click sound, synthesized at startup (no audio files): a short, soft
//! glassy "tick" — two decaying partials plus a tiny noise transient.

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle};

const RATE: u32 = 48_000;

pub struct Sound {
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    click: Vec<f32>,
    page: Vec<f32>,
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
        Self { _stream: stream, handle, click: synth_click(), page: synth_page() }
    }

    pub fn click(&self) {
        self.play(&self.click);
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
    #[test]
    fn page_sound_is_soft_and_short() {
        let s = super::synth_page();
        let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.02 && peak < 0.35, "peak {peak}");
        assert!(s.len() < super::RATE as usize / 2);
        assert!(s[s.len() - 200..].iter().all(|x| x.abs() < 0.01), "fades out");
    }
}
