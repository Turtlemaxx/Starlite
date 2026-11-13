//! Playback. One thread owns the audio device (rodio) and a mixer that never
//! stops (`engine.rs`); songs are loaded on short-lived worker threads (local
//! files are read from disk, server songs downloaded; YouTube songs play in
//! YouTube's own player instead) and handed to the mixer when ready. The song
//! that comes next is loaded ahead of time, so the mixer can start it without
//! a gap or fade into it. A generation counter drops loads that were
//! overtaken by a newer request.

use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, Sink};

use crate::engine::{self, Deck, Engine, FxShared};
use crate::library::{Source, Track};

#[derive(Clone, PartialEq, Debug)]
pub enum Status {
    Idle,
    Loading,
    Playing,
    Paused,
    Error(String),
}

pub struct Shared {
    pub status: Status,
    pub track: Option<Track>,
    /// Bumped every time a song plays to its end with nothing lined up after it.
    pub finished: u64,
    /// Bumped every time the mixer moved on to the lined-up song by itself.
    pub advanced: u64,
    /// Bumped whenever a song starts (by request or by itself).
    pub serial: u64,
    /// Playback position and length, sampled by the audio thread.
    pub pos_ms: u64,
    pub dur_ms: u64,
    pub pos_at: Instant,
    /// Playback speed (1 = normal).
    pub speed: f32,
}

enum Cmd {
    Play(u64, Track, Option<String>),
    Ready(u64, Vec<u8>),
    Fail(u64, String),
    /// Line this song up after the current one: (its id, the song, its link).
    Preload(u64, Track, Option<String>),
    /// (the song it follows, its own id, the song, its bytes)
    NextReady(u64, u64, Track, Vec<u8>),
    ClearNext,
    Pause,
    Resume,
    Stop,
    Volume(f32),
    Speed(f32),
    Seek(u64),
}

pub struct Audio {
    tx: Sender<Cmd>,
    pub shared: Arc<Mutex<Shared>>,
    gen: AtomicU64,
    /// The sound settings the mixer reads.
    fx: Arc<FxShared>,
}

/// The sample rate the sound card runs at (the mixer works at it too, so
/// songs are only converted once).
fn device_rate() -> u32 {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    rodio::cpal::default_host().default_output_device().and_then(|d| d.default_output_config().ok()).map(|c| c.sample_rate().0).filter(|r| (8_000..=384_000).contains(r)).unwrap_or(48_000)
}

fn load(track: &Track, stream: &Option<String>) -> Result<Vec<u8>, String> {
    match &track.source {
        Source::File { path } => std::fs::read(path).map_err(|e| format!("Can't open file: {e}")),
        Source::Navidrome { .. } => match stream {
            Some(url) => fetch(url),
            None => Err("Connect to Navidrome to play this song".to_string()),
        },
        Source::Jellyfin { .. } => match stream {
            Some(url) => fetch(url),
            None => Err("Connect to Jellyfin to play this song".to_string()),
        },
        // YouTube songs play in YouTube's own embedded player, never here.
        Source::YouTube { .. } => Err("YouTube songs play in the YouTube player".to_string()),
    }
}

impl Audio {
    pub fn new(ctx: egui::Context) -> Audio {
        let shared = Arc::new(Mutex::new(Shared { status: Status::Idle, track: None, finished: 0, advanced: 0, serial: 0, pos_ms: 0, dur_ms: 0, pos_at: Instant::now(), speed: 1.0 }));
        let (tx, rx) = channel::<Cmd>();
        let sh = shared.clone();
        let fx = Arc::new(FxShared::default());
        let (fxs, own_tx) = (fx.clone(), tx.clone());
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || {
                let rate = device_rate();
                let state = Arc::new(engine::State::default());
                // The device and the mixer, opened when first needed (and
                // again after a failure).
                let mut out: Option<(OutputStream, Sink, Sender<engine::Cmd>)> = None;
                let open = |volume: f32, speed: f32| -> Result<(OutputStream, Sink, Sender<engine::Cmd>), String> {
                    let (stream, handle) = OutputStream::try_default().map_err(|e| format!("No audio device: {e}"))?;
                    let sink = Sink::try_new(&handle).map_err(|e| e.to_string())?;
                    let (etx, erx) = channel();
                    sink.set_volume(volume);
                    sink.set_speed(speed);
                    sink.append(Engine::new(erx, state.clone(), fxs.clone(), rate));
                    Ok((stream, sink, etx))
                };
                let mut current = 0u64;
                // The song lined up in the mixer: (id, song).
                let mut queued: Option<(u64, Track)> = None;
                let (mut seen_ended, mut seen_advanced) = (0u64, 0u64);
                let mut volume = 0.7f32;
                let mut speed = 1.0f32;
                // After a seek the mixer reports the old position for a moment;
                // ignore readings far from the target for a short while.
                let mut seek_hold: Option<(u64, Instant)> = None;
                let set = |f: &dyn Fn(&mut Shared)| {
                    f(&mut sh.lock().unwrap());
                    ctx.request_repaint();
                };
                let deck = |id: u64, bytes: Vec<u8>, dur_ms: u64| -> Result<Deck, String> {
                    let opus = is_opus(&bytes);
                    let dec = Decoder::new(Cursor::new(bytes)).map_err(|e| {
                        if opus {
                            // A server asked for a format it has no converter for answers with its default.
                            "The server sent Opus, which Starlite can't play. Pick another format in Settings \u{2192} Playback.".to_string()
                        } else {
                            format!("Can't decode this song: {e}")
                        }
                    })?;
                    Ok(Deck::new(id, dec, dur_ms, rate))
                };
                loop {
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(Cmd::Play(g, t, stream)) => {
                            current = g;
                            seek_hold = None;
                            let lined_up = queued.take().filter(|(_, q)| q.id == t.id);
                            match (lined_up, &out) {
                                // It's the song that was waiting: no loading needed.
                                (Some((id, _)), Some((_, _, etx))) => {
                                    current = id;
                                    let _ = etx.send(engine::Cmd::Skip);
                                    let _ = etx.send(engine::Cmd::Pause(false));
                                    set(&|s| {
                                        s.status = Status::Playing;
                                        s.serial += 1;
                                        s.pos_ms = 0;
                                        s.dur_ms = t.duration_ms;
                                        s.pos_at = Instant::now();
                                        s.track = Some(t.clone());
                                    });
                                }
                                _ => {
                                    if let Some((_, _, etx)) = &out {
                                        let _ = etx.send(engine::Cmd::Stop);
                                    }
                                    set(&|s| {
                                        s.status = Status::Loading;
                                        s.pos_ms = 0;
                                        s.dur_ms = t.duration_ms;
                                        s.pos_at = Instant::now();
                                        s.track = Some(t.clone());
                                    });
                                    let tx = own_tx.clone();
                                    std::thread::spawn(move || {
                                        let _ = tx.send(match load(&t, &stream) {
                                            Ok(b) => Cmd::Ready(g, b),
                                            Err(e) => Cmd::Fail(g, e),
                                        });
                                    });
                                }
                            }
                        }
                        Ok(Cmd::Ready(g, bytes)) if g == current => {
                            let dur = sh.lock().unwrap().dur_ms;
                            let started = (|| -> Result<Option<u64>, String> {
                                let d = deck(g, bytes, dur)?;
                                if out.is_none() {
                                    out = Some(open(volume, speed)?);
                                }
                                let (_, _, etx) = out.as_ref().unwrap();
                                // The tags' length is exact; the decoder's is only a
                                // fallback (it can be thrown off by embedded covers).
                                let total = d.length_ms();
                                etx.send(engine::Cmd::Play(d)).map_err(|_| "The audio device stopped".to_string())?;
                                let _ = etx.send(engine::Cmd::Pause(false));
                                Ok(total)
                            })();
                            match started {
                                Ok(total) => set(&|s| {
                                    s.status = Status::Playing;
                                    s.serial += 1;
                                    if s.dur_ms == 0 {
                                        s.dur_ms = total.unwrap_or(0);
                                    }
                                }),
                                Err(e) => {
                                    // Try the device afresh next time.
                                    out = None;
                                    set(&|s| s.status = Status::Error(e.clone()));
                                }
                            }
                        }
                        Ok(Cmd::Fail(g, e)) if g == current => set(&|s| s.status = Status::Error(e.clone())),
                        Ok(Cmd::Ready(..) | Cmd::Fail(..)) => {} // stale load
                        Ok(Cmd::Preload(id, t, stream)) => {
                            let (after, tx) = (current, own_tx.clone());
                            std::thread::spawn(move || {
                                if let Ok(b) = load(&t, &stream) {
                                    let _ = tx.send(Cmd::NextReady(after, id, t, b));
                                }
                            });
                        }
                        Ok(Cmd::NextReady(after, id, t, bytes)) => {
                            let live = matches!(sh.lock().unwrap().status, Status::Playing | Status::Paused);
                            if after == current && live {
                                if let (Ok(d), Some((_, _, etx))) = (deck(id, bytes, t.duration_ms), &out) {
                                    let _ = etx.send(engine::Cmd::Next(d));
                                    queued = Some((id, t));
                                }
                            }
                        }
                        Ok(Cmd::ClearNext) => {
                            queued = None;
                            if let Some((_, _, etx)) = &out {
                                let _ = etx.send(engine::Cmd::ClearNext);
                            }
                        }
                        Ok(Cmd::Pause) => {
                            if let Some((_, _, etx)) = &out {
                                let _ = etx.send(engine::Cmd::Pause(true));
                                set(&|st| {
                                    if st.status == Status::Playing {
                                        st.status = Status::Paused;
                                    }
                                });
                            }
                        }
                        Ok(Cmd::Resume) => {
                            if let Some((_, _, etx)) = &out {
                                let _ = etx.send(engine::Cmd::Pause(false));
                                set(&|st| {
                                    if st.status == Status::Paused {
                                        st.status = Status::Playing;
                                    }
                                });
                            }
                        }
                        Ok(Cmd::Stop) => {
                            if let Some((_, _, etx)) = &out {
                                let _ = etx.send(engine::Cmd::Stop);
                            }
                            current = 0;
                            queued = None;
                            set(&|s| s.status = Status::Idle);
                        }
                        Ok(Cmd::Seek(ms)) => {
                            if let Some((_, _, etx)) = &out {
                                let _ = etx.send(engine::Cmd::Seek(ms));
                                seek_hold = Some((ms, Instant::now()));
                                set(&|st| {
                                    st.pos_ms = ms;
                                    st.pos_at = Instant::now();
                                });
                            }
                        }
                        Ok(Cmd::Speed(v)) => {
                            speed = v;
                            if let Some((_, sink, _)) = &out {
                                sink.set_speed(v);
                            }
                            set(&|st| st.speed = v);
                        }
                        Ok(Cmd::Volume(v)) => {
                            volume = v;
                            if let Some((_, sink, _)) = &out {
                                sink.set_volume(v);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                    // What the mixer did by itself since last time.
                    let adv = state.advanced.load(Ordering::Relaxed);
                    if adv != seen_advanced {
                        seen_advanced = adv;
                        if let Some((id, t)) = queued.take().filter(|(id, _)| *id == adv) {
                            current = id;
                            seek_hold = None;
                            set(&|s| {
                                s.status = Status::Playing;
                                s.advanced += 1;
                                s.serial += 1;
                                s.pos_ms = 0;
                                s.dur_ms = t.duration_ms;
                                s.pos_at = Instant::now();
                                s.track = Some(t.clone());
                            });
                        }
                    }
                    let ended = state.ended.load(Ordering::Relaxed);
                    if ended != seen_ended {
                        seen_ended = ended;
                        let playing = matches!(sh.lock().unwrap().status, Status::Playing | Status::Paused);
                        if ended == current && playing {
                            set(&|s| {
                                s.status = Status::Idle;
                                s.finished += 1;
                            });
                        }
                    }
                    // Sample the position for the seek bar / lyrics.
                    if state.current.load(Ordering::Relaxed) == current && current != 0 {
                        let now_ms = state.pos_ms.load(Ordering::Relaxed);
                        let stale = seek_hold.is_some_and(|(target, at)| at.elapsed() < Duration::from_millis(600) && now_ms.abs_diff(target) > 700);
                        if !stale {
                            seek_hold = None;
                            let mut st = sh.lock().unwrap();
                            if st.pos_ms != now_ms {
                                st.pos_ms = now_ms;
                                st.pos_at = Instant::now();
                            }
                        }
                    }
                }
            })
            .expect("spawn audio thread");
        Audio { tx, shared, gen: AtomicU64::new(0), fx }
    }

    /// Plays a file, or the song at `stream` (a server link), which is
    /// downloaded into memory first.
    pub fn play(&self, track: Track, stream: Option<String>) {
        let g = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.tx.send(Cmd::Play(g, track, stream));
    }

    /// Lines a song up to follow the current one without a gap (or with a
    /// crossfade). Replaces whatever was lined up.
    pub fn preload(&self, track: Track, stream: Option<String>) {
        let g = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.tx.send(Cmd::ClearNext);
        let _ = self.tx.send(Cmd::Preload(g, track, stream));
    }

    pub fn clear_next(&self) {
        let _ = self.tx.send(Cmd::ClearNext);
    }

    /// New sound settings (equalizer, effects, crossfade) for the mixer.
    pub fn set_fx(&self, p: &crate::fx::FxPrefs) {
        self.fx.set(p);
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }
    pub fn resume(&self) {
        let _ = self.tx.send(Cmd::Resume);
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
    pub fn seek(&self, ms: u64) {
        let _ = self.tx.send(Cmd::Seek(ms));
    }

    /// (position, length) in seconds, interpolated between samples.
    pub fn position(&self) -> (f64, f64) {
        let s = self.shared.lock().unwrap();
        let mut pos = s.pos_ms as f64 / 1000.0;
        if s.status == Status::Playing {
            pos += s.pos_at.elapsed().as_secs_f64().min(0.5) * s.speed as f64;
        }
        let dur = s.dur_ms as f64 / 1000.0;
        (if dur > 0.0 { pos.min(dur) } else { pos }, dur)
    }

    pub fn set_speed(&self, v: f32) {
        let _ = self.tx.send(Cmd::Speed(v));
    }

    pub fn set_volume(&self, v: f32) {
        let _ = self.tx.send(Cmd::Volume(v));
    }

    pub fn status(&self) -> Status {
        self.shared.lock().unwrap().status.clone()
    }
    pub fn finished(&self) -> u64 {
        self.shared.lock().unwrap().finished
    }
    /// (how often the mixer moved on by itself, the song it's on now)
    pub fn advanced(&self) -> (u64, Option<Track>) {
        let s = self.shared.lock().unwrap();
        (s.advanced, s.track.clone())
    }
    /// Goes up whenever a song starts.
    pub fn serial(&self) -> u64 {
        self.shared.lock().unwrap().serial
    }
}

/// An Ogg stream carrying Opus (which there's no decoder for).
fn is_opus(bytes: &[u8]) -> bool {
    bytes.starts_with(b"OggS") && bytes.windows(8).take(128).any(|w| w == b"OpusHead")
}

/// Downloads a streamed song (up to 1 GB) into memory.
pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
    fetch_prefix(url, 1 << 30)
}

/// Downloads the first `max` bytes of a streamed song.
pub fn fetch_prefix(url: &str, max: u64) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url).timeout(Duration::from_secs(120)).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("The server couldn't send this song (error {code})"),
        ureq::Error::Transport(t) => format!("Can't reach the server ({})", t.kind()),
    })?;
    if resp.content_type().contains("json") || resp.content_type().contains("xml") {
        return Err("The server couldn't send this song".into());
    }
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(resp.into_reader(), max), &mut buf).map_err(|e| format!("Download failed: {e}"))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_is_told_apart() {
        let mut ogg = b"OggS\0\x02\0\0\0\0\0\0\0\0".to_vec();
        ogg.extend(b"\x01\x13OpusHead\x01\x02");
        assert!(is_opus(&ogg));
        assert!(!is_opus(b"OggS\0\x02\0\0\0\0\0\0\0\0\x01\x1e\x01vorbis"));
        assert!(!is_opus(b"fLaC\0\0\0\x22"));
    }

    /// STARLITE_DECODE_FILES=a.mp3~b.aac~c.flac cargo test -- --ignored streams_decode
    /// Each file must decode to about `STARLITE_DECODE_SECS` seconds of sound.
    #[test]
    #[ignore]
    fn streams_decode() {
        let Ok(files) = std::env::var("STARLITE_DECODE_FILES") else { return };
        let secs: f32 = std::env::var("STARLITE_DECODE_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(20.0);
        for f in files.split('~') {
            let bytes = std::fs::read(f).unwrap();
            let dec = Decoder::new(Cursor::new(bytes)).unwrap_or_else(|e| panic!("{f}: {e}"));
            let (rate, ch) = (rodio::Source::sample_rate(&dec), rodio::Source::channels(&dec));
            let n = dec.count() as f32 / (rate as f32 * ch as f32);
            eprintln!("{f}: {n:.2} s at {rate} Hz, {ch} ch");
            assert!((n - secs).abs() < 0.5, "{f}: {n}");
        }
    }
}
