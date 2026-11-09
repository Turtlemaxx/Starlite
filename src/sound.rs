//! UI click sound, synthesized at startup (no audio files): a short, soft
//! glassy "tick" — two decaying partials plus a tiny noise transient.

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle};

const RATE: u32 = 48_000;

pub struct Sound {
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    click: Vec<f32>,
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

impl Sound {
    pub fn new() -> Self {
        let (stream, handle) = match OutputStream::try_default() {
            Ok((s, h)) => (Some(s), Some(h)),
            Err(_) => (None, None),
        };
        Self { _stream: stream, handle, click: synth_click() }
    }

    pub fn click(&self) {
        if let Some(h) = &self.handle {
            let _ = h.play_raw(SamplesBuffer::new(1, RATE, self.click.clone()));
        }
    }
}
