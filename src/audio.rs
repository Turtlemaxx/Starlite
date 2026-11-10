//! Playback engine. One thread owns the audio device (rodio); songs are
//! loaded on short-lived worker threads (local files are read from disk;
//! YouTube songs play in YouTube's own player instead) and handed over when ready.
//! A generation counter drops loads that were overtaken by a newer request.

use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, Sink, Source as _};

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
    /// Bumped every time a song plays to its end.
    pub finished: u64,
    /// Playback position and length, sampled by the audio thread.
    pub pos_ms: u64,
    pub dur_ms: u64,
    pub pos_at: Instant,
    /// Playback speed (1 = normal).
    pub speed: f32,
}

enum Cmd {
    Begin(u64, Track),
    Ready(u64, Vec<u8>),
    Fail(u64, String),
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
    /// Bass setting, read by the playing song's filter.
    pub bass: crate::fx::BassKnob,
}

impl Audio {
    pub fn new(ctx: egui::Context) -> Audio {
        let shared = Arc::new(Mutex::new(Shared { status: Status::Idle, track: None, finished: 0, pos_ms: 0, dur_ms: 0, pos_at: Instant::now(), speed: 1.0 }));
        let (tx, rx) = channel::<Cmd>();
        let sh = shared.clone();
        let bass = crate::fx::BassKnob::default();
        let knob = bass.clone();
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || {
                let device = OutputStream::try_default();
                let mut sink: Option<Sink> = None;
                let mut current = 0u64;
                let mut volume = 0.7f32;
                let mut speed = 1.0f32;
                // After a seek, rodio can report the old position for a moment;
                // ignore readings far from the target for a short while.
                let mut seek_hold: Option<(u64, Instant)> = None;
                let set = |f: &dyn Fn(&mut Shared)| {
                    f(&mut sh.lock().unwrap());
                    ctx.request_repaint();
                };
                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(Cmd::Begin(g, t)) => {
                            current = g;
                            if let Some(s) = sink.take() {
                                s.stop();
                            }
                            set(&|s| {
                                s.status = Status::Loading;
                                s.pos_ms = 0;
                                s.dur_ms = t.duration_ms;
                                s.pos_at = Instant::now();
                                s.track = Some(t.clone());
                            });
                        }
                        Ok(Cmd::Ready(g, bytes)) if g == current => {
                            let started = (|| -> Result<(Sink, Option<Duration>), String> {
                                let (_, handle) = device.as_ref().map_err(|e| format!("No audio device: {e}"))?;
                                let s = Sink::try_new(handle).map_err(|e| e.to_string())?;
                                let dec = Decoder::new(Cursor::new(bytes)).map_err(|e| format!("Can't decode this song: {e}"))?;
                                let total = rodio::Source::total_duration(&dec);
                                s.set_volume(volume);
                                s.set_speed(speed);
                                s.append(crate::fx::Bass::new(dec.convert_samples::<f32>(), knob.clone()));
                                s.play();
                                Ok((s, total))
                            })();
                            match started {
                                Ok((s, total)) => {
                                    sink = Some(s);
                                    set(&|s| {
                                        s.status = Status::Playing;
                                        // The tags' length is exact; the decoder's is only a
                                        // fallback (it can be thrown off by embedded covers).
                                        if s.dur_ms == 0 {
                                            if let Some(d) = total {
                                                s.dur_ms = d.as_millis() as u64;
                                            }
                                        }
                                    });
                                }
                                Err(e) => set(&|s| s.status = Status::Error(e.clone())),
                            }
                        }
                        Ok(Cmd::Fail(g, e)) if g == current => set(&|s| s.status = Status::Error(e.clone())),
                        Ok(Cmd::Ready(..) | Cmd::Fail(..)) => {} // stale load
                        Ok(Cmd::Pause) => {
                            if let Some(s) = &sink {
                                s.pause();
                                set(&|st| {
                                    if st.status == Status::Playing {
                                        st.status = Status::Paused;
                                    }
                                });
                            }
                        }
                        Ok(Cmd::Resume) => {
                            if let Some(s) = &sink {
                                s.play();
                                set(&|st| {
                                    if st.status == Status::Paused {
                                        st.status = Status::Playing;
                                    }
                                });
                            }
                        }
                        Ok(Cmd::Stop) => {
                            if let Some(s) = sink.take() {
                                s.stop();
                            }
                            current = 0;
                            set(&|s| s.status = Status::Idle);
                        }
                        Ok(Cmd::Seek(ms)) => {
                            if let Some(s) = &sink {
                                let _ = s.try_seek(Duration::from_millis(ms));
                                seek_hold = Some((ms, Instant::now()));
                                set(&|st| {
                                    st.pos_ms = ms;
                                    st.pos_at = Instant::now();
                                });
                            }
                        }
                        Ok(Cmd::Speed(v)) => {
                            speed = v;
                            if let Some(s) = &sink {
                                s.set_speed(v);
                            }
                            set(&|st| st.speed = v);
                        }
                        Ok(Cmd::Volume(v)) => {
                            volume = v;
                            if let Some(s) = &sink {
                                s.set_volume(v);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            let playing = sh.lock().unwrap().status == Status::Playing;
                            if playing && sink.as_ref().is_none_or(|s| s.empty()) {
                                sink = None;
                                set(&|s| {
                                    s.status = Status::Idle;
                                    s.finished += 1;
                                });
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                    // Sample the position for the seek bar / lyrics.
                    if let Some(s) = &sink {
                        let now_ms = s.get_pos().as_millis() as u64;
                        let stale = seek_hold.is_some_and(|(target, at)| at.elapsed() < Duration::from_millis(600) && now_ms.abs_diff(target) > 700);
                        if !stale {
                            seek_hold = None;
                            let mut st = sh.lock().unwrap();
                            st.pos_ms = now_ms;
                            st.pos_at = Instant::now();
                        }
                    }
                }
            })
            .expect("spawn audio thread");
        Audio { tx, shared, gen: AtomicU64::new(0), bass }
    }

    /// Plays a file, or the song at `stream` (a Navidrome URL), which is
    /// downloaded into memory first.
    pub fn play(&self, track: Track, stream: Option<String>) {
        let g = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.tx.send(Cmd::Begin(g, track.clone()));
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = match &track.source {
                Source::File { path } => std::fs::read(path).map_err(|e| format!("Can't open file: {e}")),
                Source::Navidrome { .. } => match &stream {
                    Some(url) => fetch(url),
                    None => Err("Connect to Navidrome to play this song".to_string()),
                },
                // YouTube songs play in YouTube's own embedded player, never here.
                Source::YouTube { .. } => Err("YouTube songs play in the YouTube player".to_string()),
            };
            let _ = tx.send(match res {
                Ok(b) => Cmd::Ready(g, b),
                Err(e) => Cmd::Fail(g, e),
            });
        });
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
