//! The mixer that feeds the sound card. It never ends: it plays the current
//! song, and when a next one is waiting it either starts it the moment the
//! current one's last sample has played (gapless) or fades the two into each
//! other (crossfade). Everything then runs through the equalizer and effects
//! (`dsp.rs`).
//!
//! It lives on the sound card's thread; the audio thread talks to it through
//! a channel and reads its state from a few atomics.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::source::UniformSourceIterator;
use rodio::Source;

use crate::dsp::Chain;
use crate::fx::FxPrefs;

/// The sound settings, shared with the mixer (`version` says when they changed).
#[derive(Default)]
pub struct FxShared {
    pub version: AtomicU64,
    pub prefs: Mutex<FxPrefs>,
}

impl FxShared {
    pub fn set(&self, p: &FxPrefs) {
        *self.prefs.lock().unwrap() = *p;
        self.version.fetch_add(1, Ordering::SeqCst);
    }
}

/// What the mixer reports.
#[derive(Default)]
pub struct State {
    /// Position in the current song (ms).
    pub pos_ms: AtomicU64,
    /// Id of the song that's playing (0: none).
    pub current: AtomicU64,
    /// Id of the last song that played to its end with nothing after it.
    pub ended: AtomicU64,
    /// Id of the last song the mixer moved on to by itself.
    pub advanced: AtomicU64,
}

/// A song, decoded to stereo at the mixer's sample rate.
pub struct Deck {
    pub id: u64,
    src: Box<dyn Source<Item = f32> + Send>,
    rate: u32,
    /// Where counting started (after a seek), ms.
    base_ms: u64,
    frames: u64,
    /// Length in frames, when known.
    total: Option<u64>,
}

impl Deck {
    /// `dur_ms`: the song's length when known (0: ask the decoder).
    pub fn new<S>(id: u64, src: S, dur_ms: u64, rate: u32) -> Deck
    where
        S: Source + Send + 'static,
        S::Item: rodio::Sample + Send,
        f32: rodio::cpal::FromSample<S::Item>,
    {
        let dur = if dur_ms > 0 { Some(dur_ms) } else { src.total_duration().map(|d| d.as_millis() as u64) };
        let src: UniformSourceIterator<S, f32> = UniformSourceIterator::new(src, 2, rate);
        Deck { id, src: Box::new(src), rate, base_ms: 0, frames: 0, total: dur.map(|ms| ms * rate as u64 / 1000) }
    }

    /// The song's length (ms), when known.
    pub fn length_ms(&self) -> Option<u64> {
        self.total.map(|t| t * 1000 / self.rate as u64)
    }

    fn frame(&mut self) -> Option<[f32; 2]> {
        let l = self.src.next()?;
        let r = self.src.next().unwrap_or(l);
        self.frames += 1;
        Some([l, r])
    }

    fn pos_ms(&self) -> u64 {
        self.base_ms + self.frames * 1000 / self.rate as u64
    }

    /// Frames left, when the length is known.
    fn remaining(&self) -> Option<u64> {
        let played = self.base_ms * self.rate as u64 / 1000 + self.frames;
        self.total.map(|t| t.saturating_sub(played))
    }

    fn seek(&mut self, ms: u64) {
        if self.src.try_seek(Duration::from_millis(ms)).is_ok() {
            self.base_ms = ms;
            self.frames = 0;
        }
    }
}

pub enum Cmd {
    /// Play this now (whatever was playing fades out in a blink).
    Play(Deck),
    /// Play this after the current song.
    Next(Deck),
    ClearNext,
    /// Move on to the waiting song now.
    Skip,
    Seek(u64),
    Stop,
    /// Hold everything (silence) or carry on. Commands still get through
    /// while paused, so a seek or a new song takes effect at once.
    Pause(bool),
}

/// A song on its way out.
struct Leaving {
    deck: Deck,
    len: u64,
    i: u64,
    /// Fading against the song coming in (equal power), not just out.
    cross: bool,
}

pub struct Engine {
    rx: Receiver<Cmd>,
    state: Arc<State>,
    fx: Arc<FxShared>,
    fx_seen: u64,
    rate: u32,
    cur: Option<Deck>,
    next: Option<Deck>,
    leaving: Option<Leaving>,
    /// Fade-in of the current song during a crossfade: (length, position).
    entering: Option<(u64, u64)>,
    xfade: u64,
    paused: bool,
    dsp: Chain,
    tick: u32,
    /// The right sample of the frame whose left one just went out.
    held: Option<f32>,
}

impl Engine {
    pub fn new(rx: Receiver<Cmd>, state: Arc<State>, fx: Arc<FxShared>, rate: u32) -> Engine {
        let mut e = Engine { rx, state, fx, fx_seen: u64::MAX, rate, cur: None, next: None, leaving: None, entering: None, xfade: 0, paused: false, dsp: Chain::new(rate), tick: 0, held: None };
        e.poll();
        e
    }

    /// Commands and new sound settings (every few milliseconds).
    fn poll(&mut self) {
        let v = self.fx.version.load(Ordering::Relaxed);
        if v != self.fx_seen {
            if let Ok(p) = self.fx.prefs.try_lock() {
                self.fx_seen = v;
                self.xfade = (p.crossfade.max(0.0) * self.rate as f32) as u64;
                self.dsp.set(&p);
            }
        }
        while let Ok(c) = self.rx.try_recv() {
            match c {
                Cmd::Play(d) => {
                    self.next = None;
                    self.switch(d, false);
                }
                Cmd::Next(d) => self.next = Some(d),
                Cmd::ClearNext => self.next = None,
                Cmd::Skip => {
                    if let Some(d) = self.next.take() {
                        self.switch(d, false);
                    }
                }
                Cmd::Seek(ms) => {
                    if let Some(c) = self.cur.as_mut() {
                        c.seek(ms);
                        self.state.pos_ms.store(ms, Ordering::Relaxed);
                    }
                }
                Cmd::Pause(p) => self.paused = p,
                Cmd::Stop => {
                    self.next = None;
                    self.entering = None;
                    self.leave(self.rate as u64 * 30 / 1000, false);
                    self.state.current.store(0, Ordering::Relaxed);
                }
            }
        }
    }

    /// Sends the current song out over `len` frames.
    fn leave(&mut self, len: u64, cross: bool) {
        self.leaving = self.cur.take().filter(|_| len > 0).map(|deck| Leaving { deck, len, i: 0, cross });
    }

    /// Makes `d` the current song. `cross`: fade it in against the old one
    /// over what's left of that (at most the crossfade); else the old one
    /// just drops away quickly.
    fn switch(&mut self, d: Deck, cross: bool) {
        if cross {
            let len = self.cur.as_ref().and_then(|c| c.remaining()).unwrap_or(0).min(self.xfade);
            self.leave(len, true);
            self.entering = (len > 0).then_some((len, 0));
        } else {
            self.leave(self.rate as u64 * 30 / 1000, false);
            self.entering = None;
        }
        self.state.current.store(d.id, Ordering::Relaxed);
        self.state.pos_ms.store(0, Ordering::Relaxed);
        self.cur = Some(d);
    }

    fn frame(&mut self) -> [f32; 2] {
        self.tick = self.tick.wrapping_add(1);
        if self.tick % 256 == 0 {
            self.poll();
            if let Some(c) = &self.cur {
                self.state.pos_ms.store(c.pos_ms(), Ordering::Relaxed);
            }
        }
        if self.paused {
            return [0.0; 2];
        }
        // Time to bring the next song in?
        if self.xfade > 0 && self.next.is_some() && self.cur.as_ref().is_some_and(|c| c.remaining().is_some_and(|r| r <= self.xfade)) {
            self.advance();
        }
        let mut out = [0.0f32; 2];
        loop {
            let Some(c) = self.cur.as_mut() else { break };
            match c.frame() {
                Some(f) => {
                    out = f;
                    break;
                }
                None => {
                    // Its last sample is out: the next song starts in this very frame.
                    if self.next.is_some() {
                        self.advance();
                    } else {
                        let id = c.id;
                        self.cur = None;
                        self.state.current.store(0, Ordering::Relaxed);
                        self.state.ended.store(id, Ordering::Relaxed);
                    }
                }
            }
        }
        if let Some((len, i)) = self.entering.as_mut() {
            let g = (*i as f32 / *len as f32 * std::f32::consts::FRAC_PI_2).sin();
            out = [out[0] * g, out[1] * g];
            *i += 1;
            if *i >= *len {
                self.entering = None;
            }
        }
        if let Some(lv) = self.leaving.as_mut() {
            match lv.deck.frame().filter(|_| lv.i < lv.len) {
                Some(f) => {
                    let t = lv.i as f32 / lv.len as f32;
                    let g = if lv.cross { (t * std::f32::consts::FRAC_PI_2).cos() } else { 1.0 - t };
                    out = [out[0] + f[0] * g, out[1] + f[1] * g];
                    lv.i += 1;
                }
                None => self.leaving = None,
            }
        }
        self.dsp.process(out)
    }

    /// Moves on to the waiting song by itself (end of the current one).
    fn advance(&mut self) {
        if let Some(d) = self.next.take() {
            let id = d.id;
            self.switch(d, true);
            self.state.advanced.store(id, Ordering::Relaxed);
        }
    }
}

impl Iterator for Engine {
    type Item = f32;
    #[inline]
    fn next(&mut self) -> Option<f32> {
        if let Some(r) = self.held.take() {
            return Some(r);
        }
        let [l, r] = self.frame();
        self.held = Some(r);
        Some(l)
    }
}

impl Source for Engine {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::sync::mpsc::{channel, Sender};

    const RATE: u32 = 8_000;

    /// A song that is `value` on both sides for `secs`.
    fn flat(id: u64, value: f32, secs: f32) -> Deck {
        let n = (RATE as f32 * secs) as usize * 2;
        Deck::new(id, SamplesBuffer::new(2, RATE, vec![value; n]), (secs * 1000.0) as u64, RATE)
    }

    fn engine(crossfade: f32) -> (Engine, Sender<Cmd>, Arc<State>) {
        let (tx, rx) = channel();
        let state = Arc::new(State::default());
        let fx = Arc::new(FxShared::default());
        fx.set(&FxPrefs { crossfade, ..Default::default() });
        (Engine::new(rx, state.clone(), fx, RATE), tx, state)
    }

    /// The left side of the next `secs` of output.
    fn pull(e: &mut Engine, secs: f32) -> Vec<f32> {
        (0..(RATE as f32 * secs) as usize).map(|_| e.frame()[0]).collect()
    }

    #[test]
    fn gapless_puts_the_next_song_right_after_the_last_sample() {
        let (mut e, tx, state) = engine(0.0);
        tx.send(Cmd::Play(flat(1, 0.5, 1.0))).unwrap();
        tx.send(Cmd::Next(flat(2, -0.5, 1.0))).unwrap();
        let out = pull(&mut e, 2.2);
        let first = out.iter().position(|&x| x == 0.5).unwrap();
        // One second of the first song, then the second with no silent frame between.
        let n = RATE as usize;
        assert!(out[first..first + n].iter().all(|&x| x == 0.5));
        assert!(out[first + n..first + 2 * n].iter().all(|&x| x == -0.5));
        assert_eq!(state.advanced.load(Ordering::Relaxed), 2);
        // After the second there's nothing: silence, and it says so.
        assert_eq!(out[first + 2 * n + 400], 0.0);
        assert_eq!(state.ended.load(Ordering::Relaxed), 2);
        assert_eq!(state.current.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn crossfade_overlaps_the_two_songs() {
        let (mut e, tx, state) = engine(0.5);
        tx.send(Cmd::Play(flat(1, 0.4, 2.0))).unwrap();
        tx.send(Cmd::Next(flat(2, 0.4, 2.0))).unwrap();
        let out = pull(&mut e, 3.9);
        let first = out.iter().position(|&x| x != 0.0).unwrap();
        // The second song comes in half a second before the first ends.
        let start = first + (RATE as f32 * 1.5) as usize;
        assert!(out[first + 100..start - 300].iter().all(|&x| x == 0.4));
        let mid = out[start + RATE as usize / 4];
        // Equal power: both at 0.707 in the middle.
        assert!((mid - 0.4 * 2.0 * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01, "{mid}");
        // No dip or silence anywhere across the change.
        assert!(out[first + 100..first + 3 * RATE as usize].iter().all(|&x| x >= 0.39));
        assert_eq!(state.advanced.load(Ordering::Relaxed), 2);
        // 3.5 s of sound in all (2 + 2 − 0.5), then the end.
        assert_eq!(out[first + (RATE as f32 * 3.6) as usize], 0.0);
        assert_eq!(state.ended.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn playing_something_else_replaces_the_song_and_what_was_waiting() {
        let (mut e, tx, state) = engine(1.0);
        tx.send(Cmd::Play(flat(1, 0.5, 2.0))).unwrap();
        tx.send(Cmd::Next(flat(2, 0.3, 2.0))).unwrap();
        pull(&mut e, 0.5);
        tx.send(Cmd::Play(flat(3, -0.2, 1.0))).unwrap();
        let out = pull(&mut e, 0.5);
        assert_eq!(state.current.load(Ordering::Relaxed), 3);
        assert_eq!(out[out.len() - 1], -0.2);
        // Song 2 was dropped: after song 3 it's over.
        pull(&mut e, 1.0);
        assert_eq!(state.ended.load(Ordering::Relaxed), 3);
        assert_eq!(state.advanced.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn skip_and_seek_and_stop() {
        let (mut e, tx, state) = engine(0.0);
        tx.send(Cmd::Play(flat(1, 0.5, 4.0))).unwrap();
        tx.send(Cmd::Next(flat(2, 0.3, 4.0))).unwrap();
        pull(&mut e, 0.2);
        tx.send(Cmd::Seek(3000)).unwrap();
        pull(&mut e, 0.2);
        let p = state.pos_ms.load(Ordering::Relaxed);
        assert!((3000..3400).contains(&p), "{p}");
        tx.send(Cmd::Skip).unwrap();
        let out = pull(&mut e, 0.2);
        assert_eq!((state.current.load(Ordering::Relaxed), out[out.len() - 1]), (2, 0.3));
        tx.send(Cmd::Stop).unwrap();
        let out = pull(&mut e, 0.2);
        assert_eq!((state.current.load(Ordering::Relaxed), out[out.len() - 1]), (0, 0.0));
        // Stopping isn't "the song ended".
        assert_eq!(state.ended.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn paused_is_silent_but_still_listens() {
        let (mut e, tx, state) = engine(0.0);
        tx.send(Cmd::Play(flat(1, 0.5, 4.0))).unwrap();
        pull(&mut e, 0.3);
        tx.send(Cmd::Pause(true)).unwrap();
        let out = pull(&mut e, 0.3);
        assert_eq!(out[out.len() - 1], 0.0);
        let held = state.pos_ms.load(Ordering::Relaxed);
        // A seek while paused lands at once; nothing plays on.
        tx.send(Cmd::Seek(2000)).unwrap();
        pull(&mut e, 0.3);
        assert!(held < 600);
        assert_eq!(state.pos_ms.load(Ordering::Relaxed), 2000);
        tx.send(Cmd::Pause(false)).unwrap();
        let out = pull(&mut e, 0.3);
        assert_eq!(out[out.len() - 1], 0.5);
        assert!(state.pos_ms.load(Ordering::Relaxed) > 2100);
    }

    #[test]
    fn songs_at_other_sample_rates_keep_their_length() {
        let (mut e, tx, _) = engine(0.0);
        // One second of mono at 16 kHz.
        let d = Deck::new(1, SamplesBuffer::new(1, 16_000, vec![0.25f32; 16_000]), 0, RATE);
        tx.send(Cmd::Play(d)).unwrap();
        let out = pull(&mut e, 1.5);
        let n = out.iter().filter(|&&x| x > 0.2).count();
        assert!((n as i64 - RATE as i64).abs() < 200, "{n}");
    }
}
