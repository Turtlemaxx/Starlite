//! Song previews for the CD shelf: about 15 seconds of a song, from its
//! chorus when the timed lyrics show one, else from where the singing
//! starts, else from its loudest stretch. Previews play on their own output
//! (the main player is only turned down meanwhile), and their sound is split
//! into a few frequency bands for the equalizer.
//!
//! YouTube songs have no preview: they only ever play in YouTube's own
//! visible player.

use std::collections::HashMap;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, Sink, Source as _};

use crate::library::{Source, Track};
use crate::meta::Lyrics;

pub const BANDS: usize = 7;
const CENTERS: [f32; BANDS] = [60.0, 160.0, 400.0, 1000.0, 2500.0, 6000.0, 12000.0];
pub const LENGTH_MS: u64 = 15_000;
const FADE_IN: f32 = 0.6;
const FADE_OUT: f32 = 1.6;
const STOP_FADE: f32 = 0.25;
/// How long a preview waits for an online lyrics lookup (to find the chorus).
const LYRICS_WAIT: Duration = Duration::from_millis(900);

/// What the equalizer reads: each band's loudness (RMS, as f32 bits).
#[derive(Default)]
pub struct Levels {
    bands: [AtomicU32; BANDS],
}

impl Levels {
    pub fn get(&self) -> [f32; BANDS] {
        std::array::from_fn(|i| f32::from_bits(self.bands[i].load(Ordering::Relaxed)))
    }
    fn set(&self, v: [f32; BANDS]) {
        for (b, x) in self.bands.iter().zip(v) {
            b.store(x.to_bits(), Ordering::Relaxed);
        }
    }
}

/// A preview's sound, ready to play (already at its start).
type Clip = Box<dyn rodio::Source<Item = f32> + Send>;

enum Cmd {
    /// generation, the sound.
    Play(u64, Clip),
    Stop,
    Volume(f32),
}

pub struct Preview {
    tx: Sender<Cmd>,
    pub levels: Arc<Levels>,
    gen: Arc<AtomicU64>,
    /// Track id of the preview being loaded or played.
    pub current: Option<String>,
    /// Set by the audio thread while a preview is audible.
    playing: Arc<AtomicBool>,
    /// Where each song's preview starts (ms), once worked out.
    starts: Arc<Mutex<HashMap<String, u64>>>,
}

impl Preview {
    pub fn new() -> Preview {
        let (tx, rx) = channel::<Cmd>();
        let levels = Arc::new(Levels::default());
        let playing = Arc::new(AtomicBool::new(false));
        let (lv, pl) = (levels.clone(), playing.clone());
        std::thread::Builder::new()
            .name("preview".into())
            .spawn(move || {
                // Opened on first use, so an app that never previews holds no second output.
                let mut device: Option<Result<(OutputStream, rodio::OutputStreamHandle), String>> = None;
                let mut sink: Option<(Sink, Arc<AtomicBool>)> = None;
                let mut fading: Vec<(Sink, Instant)> = Vec::new();
                let mut volume = 0.7f32;
                let mut latest = 0u64;
                loop {
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(Cmd::Play(g, clip)) => {
                            if g < latest {
                                continue;
                            }
                            latest = g;
                            if let Some((s, stop)) = sink.take() {
                                stop.store(true, Ordering::Relaxed);
                                fading.push((s, Instant::now()));
                            }
                            // A sound device that failed to open is tried again next time.
                            if device.as_ref().is_some_and(|d| d.is_err()) {
                                device = None;
                            }
                            let dev = device.get_or_insert_with(|| OutputStream::try_default().map_err(|e| e.to_string()));
                            let Ok((_, handle)) = dev.as_ref() else { continue };
                            let Ok(s) = Sink::try_new(handle) else { continue };
                            let stop = Arc::new(AtomicBool::new(false));
                            s.set_volume(volume);
                            s.append(Tap::new(clip, lv.clone(), stop.clone()));
                            s.play();
                            sink = Some((s, stop));
                        }
                        Ok(Cmd::Stop) => {
                            if let Some((s, stop)) = sink.take() {
                                stop.store(true, Ordering::Relaxed);
                                fading.push((s, Instant::now()));
                            }
                        }
                        Ok(Cmd::Volume(v)) => {
                            volume = v;
                            if let Some((s, _)) = &sink {
                                s.set_volume(v);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                    fading.retain(|(s, at)| !s.empty() && at.elapsed() < Duration::from_millis(600));
                    if sink.as_ref().is_some_and(|(s, _)| s.empty()) {
                        sink = None;
                    }
                    let audible = sink.is_some() || !fading.is_empty();
                    pl.store(audible, Ordering::Relaxed);
                    if !audible {
                        lv.set([0.0; BANDS]);
                    }
                }
            })
            .expect("spawn preview thread");
        Preview { tx, levels, gen: Arc::new(AtomicU64::new(0)), current: None, playing, starts: Default::default() }
    }

    /// Starts previewing `track` (loads in the background). `stream`: a
    /// server's preview link (with a `{secs}` or `{ticks}` start in it). `online`: may look lyrics up on lrclib.net.
    pub fn play(&mut self, track: &Track, stream: Option<String>, online: bool) {
        if self.current.as_deref() == Some(track.id.as_str()) {
            return;
        }
        self.current = Some(track.id.clone());
        let g = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        if track.youtube_id().is_some() {
            let _ = self.tx.send(Cmd::Stop);
            return;
        }
        let (t, tx, gen, starts) = (track.clone(), self.tx.clone(), self.gen.clone(), self.starts.clone());
        std::thread::spawn(move || {
            let known = starts.lock().unwrap().get(&t.id).copied();
            // Look the lyrics up alongside the download, but never wait long
            // for them: a slow lrclib.net used to hold the preview back until
            // the mouse had already moved on. The lookup still finishes (and
            // is cached), so the next hover finds the chorus.
            let lyrics = known.is_none().then(|| {
                let (ltx, lrx) = channel();
                let lt = t.clone();
                std::thread::spawn(move || {
                    let _ = ltx.send(song_lyrics(&lt, online).and_then(|l| highlight_ms(&l)));
                });
                lrx
            });
            let asked = Instant::now();
            // The chorus from the lyrics (not waiting long), else `fallback`.
            // Only a real answer is remembered; a late one is asked again next time.
            let pick = |fallback: &dyn Fn() -> u64| -> u64 {
                match (known, &lyrics) {
                    (Some(s), _) => s,
                    (None, Some(lrx)) => match lrx.recv_timeout(LYRICS_WAIT.saturating_sub(asked.elapsed())) {
                        Ok(found) => {
                            let s = found.unwrap_or_else(fallback);
                            starts.lock().unwrap().insert(t.id.clone(), s);
                            s
                        }
                        Err(_) => fallback(),
                    },
                    (None, None) => fallback(),
                }
            };
            // Leave room for the whole preview before the song ends.
            let fit = |start: u64| if t.duration_ms > LENGTH_MS { start.min(t.duration_ms - LENGTH_MS) } else { 0 };
            let clip: Clip = match (&t.source, &stream) {
                (Source::File { path }, _) => {
                    let Ok(bytes) = std::fs::read(path) else { return };
                    if gen.load(Ordering::SeqCst) != g {
                        return;
                    }
                    let start = fit(pick(&|| loudest_ms(&bytes).unwrap_or(30_000)));
                    let Ok(mut dec) = Decoder::new(Cursor::new(bytes)) else { return };
                    // Get to the start here, not on the sound card's thread.
                    if dec.try_seek(Duration::from_millis(start)).is_err() {
                        skip_ms(&mut dec, start);
                    }
                    Box::new(dec.convert_samples::<f32>().take_duration(Duration::from_millis(LENGTH_MS)))
                }
                (Source::Navidrome { .. } | Source::Jellyfin { .. }, Some(url)) => {
                    // The server starts the stream at the chorus (timeOffset)
                    // and the preview plays while the rest arrives: no
                    // waiting for the song from its beginning.
                    let start = fit(pick(&|| guess_ms(t.duration_ms)));
                    if gen.load(Ordering::SeqCst) != g {
                        return;
                    }
                    // Whole seconds (Jellyfin counts in ticks of 100 ns); the rest is skipped below.
                    let secs = start / 1000;
                    let url = url.replace("{secs}", &secs.to_string()).replace("{ticks}", &(secs * 10_000_000).to_string());
                    // 160 kbps MP3 is 20 bytes a millisecond.
                    let Ok(live) = Live::open(&url, (LENGTH_MS + 3_000) * 20 + 64 * 1024, gen.clone(), g) else { return };
                    // A few seconds in hand first, so playback never waits on the network.
                    live.wait_for(64 * 1024);
                    let Ok(mut dec) = Decoder::new(live) else { return };
                    skip_ms(&mut dec, start % 1000);
                    Box::new(dec.convert_samples::<f32>().take_duration(Duration::from_millis(LENGTH_MS)))
                }
                _ => return,
            };
            if gen.load(Ordering::SeqCst) == g {
                let _ = tx.send(Cmd::Play(g, clip));
            }
        });
    }

    pub fn stop(&mut self) {
        if self.current.take().is_some() {
            self.gen.fetch_add(1, Ordering::SeqCst);
            let _ = self.tx.send(Cmd::Stop);
        }
    }

    pub fn set_volume(&self, v: f32) {
        let _ = self.tx.send(Cmd::Volume(v));
    }

    /// Is a preview audible (or fading out)?
    pub fn audible(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

/// The song's own timed lyrics, else lrclib.net's (when allowed).
fn song_lyrics(t: &Track, online: bool) -> Option<Lyrics> {
    let local = match &t.source {
        Source::File { path } => crate::meta::lyrics(path),
        _ => None,
    };
    if local.as_ref().is_some_and(|l| l.synced) {
        return local;
    }
    online.then(|| crate::lyrics::online(t)).flatten().filter(|l| l.synced)
}

/// Where a preview should start, from timed lyrics: the chorus (the block of
/// lines that comes back most), else where the singing starts.
pub fn highlight_ms(ly: &Lyrics) -> Option<u64> {
    if !ly.synced {
        return None;
    }
    let lines: Vec<(u64, String)> = ly
        .lines
        .iter()
        .filter_map(|l| {
            let n: String = l.text.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
            let n = n.split_whitespace().collect::<Vec<_>>().join(" ");
            (!n.is_empty()).then(|| (l.at_ms.unwrap_or(0), n))
        })
        .collect();
    let first = lines.first()?.0;
    let mut best: Option<(usize, usize)> = None; // (score, index)
    for i in 0..lines.len() {
        if lines[i].1.len() < 6 {
            continue;
        }
        let mut occurrences = 0;
        let mut run = 0;
        for j in i + 1..lines.len() {
            if lines[j].1 != lines[i].1 {
                continue;
            }
            occurrences += 1;
            let mut k = 1;
            while i + k < j && j + k < lines.len() && lines[i + k].1 == lines[j + k].1 {
                k += 1;
            }
            run = run.max(k);
        }
        // A real chorus: at least two lines that come back together.
        if run >= 2 {
            let score = run * (occurrences + 1);
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, i));
            }
        }
    }
    let at = best.map(|(_, i)| lines[i].0).unwrap_or(first);
    // Start a breath before the line.
    Some(at.saturating_sub(400))
}

/// Decodes (and drops) the first `ms` of a song.
fn skip_ms<R: Read + Seek + Send + Sync + 'static>(dec: &mut Decoder<R>, ms: u64) {
    let n = ms * dec.sample_rate() as u64 * dec.channels() as u64 / 1000;
    let _ = dec.by_ref().take(n as usize).count();
}

/// A download that can be read while it's still arriving.
#[derive(Default)]
struct Growing {
    /// The bytes so far, and whether the download is over.
    data: Mutex<(Vec<u8>, bool)>,
    more: Condvar,
}

/// Reads a `Growing` download; waits for bytes that haven't come yet (up to
/// a few seconds, then it counts as the end).
struct Live {
    g: Arc<Growing>,
    pos: u64,
}

impl Live {
    /// Starts downloading up to `max` bytes; stops early when the preview
    /// generation moves on.
    fn open(url: &str, max: u64, gen: Arc<AtomicU64>, g: u64) -> Result<Live, String> {
        let resp = ureq::get(url).timeout(Duration::from_secs(30)).call().map_err(|e| e.to_string())?;
        if resp.content_type().contains("json") || resp.content_type().contains("xml") {
            return Err("not audio".into());
        }
        let grow = Arc::new(Growing::default());
        let gw = grow.clone();
        std::thread::spawn(move || {
            let mut r = resp.into_reader();
            let mut buf = vec![0u8; 16 * 1024];
            let mut total = 0u64;
            while total < max && gen.load(Ordering::SeqCst) == g {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        gw.data.lock().unwrap().0.extend_from_slice(&buf[..n]);
                        gw.more.notify_all();
                        total += n as u64;
                    }
                }
            }
            gw.data.lock().unwrap().1 = true;
            gw.more.notify_all();
        });
        Ok(Live { g: grow, pos: 0 })
    }

    /// Waits until `want` bytes are in (or the download is over, or stalls); how many there are.
    fn wait_for(&self, want: u64) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut d = self.g.data.lock().unwrap();
        while (d.0.len() as u64) < want && !d.1 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            d = self.g.more.wait_timeout(d, left).unwrap().0;
        }
        d.0.len() as u64
    }
}

impl Read for Live {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let len = self.wait_for(self.pos + 1);
        if self.pos >= len || out.is_empty() {
            return Ok(0);
        }
        let d = self.g.data.lock().unwrap();
        let n = out.len().min((len - self.pos) as usize);
        out[..n].copy_from_slice(&d.0[self.pos as usize..self.pos as usize + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Live {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let target = match to {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => self.wait_for(u64::MAX) as i64 + d,
        };
        if target < 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

/// Where a song without timed lyrics probably gets going, when only part of
/// it is downloaded: about a third in (usually past the intro and verse).
fn guess_ms(duration_ms: u64) -> u64 {
    (duration_ms * 3 / 10).clamp(20_000, 75_000)
}

/// The start of the song's loudest ~10 s (between 10 % and 80 % of it), for
/// songs without timed lyrics.
pub fn loudest_ms(bytes: &[u8]) -> Option<u64> {
    let dec = Decoder::new(Cursor::new(bytes.to_vec())).ok()?;
    let (ch, rate) = (dec.channels().max(1) as usize, dec.sample_rate().max(1) as usize);
    let block = rate * ch; // one second
    let mut secs: Vec<f32> = Vec::new();
    let (mut acc, mut n) = (0f32, 0usize);
    for s in dec.convert_samples::<f32>() {
        acc += s * s;
        n += 1;
        if n == block {
            secs.push(acc / n as f32);
            acc = 0.0;
            n = 0;
        }
        if secs.len() > 1200 {
            break;
        }
    }
    let win = 10;
    if secs.len() < win + 4 {
        return None;
    }
    let (lo, hi) = (secs.len() / 10, (secs.len() * 8 / 10).saturating_sub(win));
    (lo..=hi.max(lo)).max_by(|&a, &b| secs[a..a + win].iter().sum::<f32>().total_cmp(&secs[b..b + win].iter().sum::<f32>())).map(|s| s as u64 * 1000)
}

/// A band-pass filter (RBJ cookbook, 0 dB peak).
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn band(f0: f32, rate: f32) -> Biquad {
        let f0 = f0.min(rate * 0.45);
        let w0 = std::f32::consts::TAU * f0 / rate;
        let alpha = w0.sin() / (2.0 * 1.1);
        let a0 = 1.0 + alpha;
        Biquad { b0: alpha / a0, b2: -alpha / a0, a1: -2.0 * w0.cos() / a0, a2: (1.0 - alpha) / a0, ..Default::default() }
    }
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Passes the preview through with its fades, measuring the bands.
struct Tap<S> {
    inner: S,
    ch: u16,
    rate: u32,
    samples: u64,
    total_frames: u64,
    acc: f32,
    filters: [Biquad; BANDS],
    energy: [f32; BANDS],
    n: u32,
    block: u32,
    levels: Arc<Levels>,
    stop: Arc<AtomicBool>,
    stop_at: Option<u64>,
}

impl<S: rodio::Source<Item = f32>> Tap<S> {
    fn new(inner: S, levels: Arc<Levels>, stop: Arc<AtomicBool>) -> Self {
        let (ch, rate) = (inner.channels().max(1), inner.sample_rate().max(1));
        Tap {
            ch,
            rate,
            samples: 0,
            total_frames: LENGTH_MS * rate as u64 / 1000,
            acc: 0.0,
            filters: CENTERS.map(|f| Biquad::band(f, rate as f32)),
            energy: [0.0; BANDS],
            n: 0,
            block: (rate / 60).max(1),
            levels,
            stop,
            stop_at: None,
            inner,
        }
    }
}

impl<S: rodio::Source<Item = f32>> Iterator for Tap<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let s = self.inner.next()?;
        let frame = self.samples / self.ch as u64;
        let rate = self.rate as f32;
        let t = frame as f32 / rate;
        let left = self.total_frames.saturating_sub(frame) as f32 / rate;
        let mut g = (t / FADE_IN).min(1.0) * (left / FADE_OUT).clamp(0.0, 1.0);
        if self.stop.load(Ordering::Relaxed) {
            let at = *self.stop_at.get_or_insert(frame);
            let since = (frame - at) as f32 / rate;
            if since > STOP_FADE {
                return None;
            }
            g *= 1.0 - since / STOP_FADE;
        }
        // Ease the fades (sounds smoother than a straight line).
        let g = g * g * (3.0 - 2.0 * g);
        let out = s * g;
        self.acc += out;
        self.samples += 1;
        if self.samples % self.ch as u64 == 0 {
            let mono = self.acc / self.ch as f32;
            self.acc = 0.0;
            for (f, e) in self.filters.iter_mut().zip(self.energy.iter_mut()) {
                let y = f.run(mono);
                *e += y * y;
            }
            self.n += 1;
            if self.n >= self.block {
                let n = self.n as f32;
                self.levels.set(self.energy.map(|e| (e / n).sqrt()));
                self.energy = [0.0; BANDS];
                self.n = 0;
            }
        }
        Some(out)
    }
}

impl<S: rodio::Source<Item = f32>> rodio::Source for Tap<S> {
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }
    fn channels(&self) -> u16 {
        self.ch
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Band loudness in dB, with the highs lifted (music has much less energy
/// up there, and the bars should all have something to do).
pub fn band_db(levels: [f32; BANDS]) -> [f32; BANDS] {
    const TILT: [f32; BANDS] = [-3.0, 0.0, 2.0, 5.0, 8.0, 11.0, 14.0];
    std::array::from_fn(|i| 20.0 * (levels[i] + 1e-6).log10() + TILT[i])
}

/// Equalizer bar heights (0..1) relative to the recent peak `peak_db`, so
/// quiet and loud songs both move the bars over their whole height.
pub fn bar_heights(db: [f32; BANDS], peak_db: f32) -> [f32; BANDS] {
    const RANGE: f32 = 26.0;
    db.map(|d| ((d - (peak_db - RANGE)) / RANGE).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chorus_is_the_block_that_comes_back() {
        let lrc = "[00:05.00] verse one\n[00:09.00] verse two goes here\n[00:20.00] this is the chorus\n[00:24.00] sing it loud now\n\
                   [00:30.00] another verse line\n[00:34.00] and one more\n[00:45.00] this is the chorus\n[00:49.00] sing it loud now\n[00:55.00] outro";
        let ly = crate::meta::parse(lrc).unwrap();
        assert_eq!(highlight_ms(&ly), Some(19_600));
    }

    #[test]
    fn without_a_chorus_it_starts_with_the_singing() {
        let ly = crate::meta::parse("[00:00.00] \n[00:12.50] first words\n[00:16.00] then more").unwrap();
        assert_eq!(highlight_ms(&ly), Some(12_100));
        assert_eq!(highlight_ms(&crate::meta::parse("plain words").unwrap()), None);
    }

    #[test]
    fn a_tone_lights_its_band() {
        let rate = 44_100u32;
        let tone: Vec<f32> = (0..rate).map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / rate as f32).sin() * 0.5).collect();
        let src = rodio::buffer::SamplesBuffer::new(1, rate, tone);
        let levels = Arc::new(Levels::default());
        let tap = Tap::new(src, levels.clone(), Arc::new(AtomicBool::new(false)));
        let _ = tap.take(rate as usize / 2).count();
        let l = levels.get();
        let loudest = (0..BANDS).max_by(|&a, &b| l[a].total_cmp(&l[b])).unwrap();
        assert_eq!(CENTERS[loudest], 1000.0, "{l:?}");
    }

    /// Needs a sound device and the internet (lrclib.net):
    /// STARLITE_PREVIEW_FILE=<audio file> cargo test -- --ignored preview_starts_quickly
    #[test]
    #[ignore]
    fn preview_starts_quickly() {
        let Ok(f) = std::env::var("STARLITE_PREVIEW_FILE") else { return };
        let mut t = crate::library::file_track(std::path::Path::new(&f));
        // A song lrclib.net won't know, so the lookup runs its full course.
        t.id = format!("test:{}", std::process::id());
        t.title = format!("Nothing Like This {}", std::process::id());
        let mut p = Preview::new();
        p.set_volume(0.0);
        let at = Instant::now();
        p.play(&t, None, true);
        while !p.audible() && at.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let took = at.elapsed();
        p.stop();
        eprintln!("preview audible after {took:?}");
        // What it used to wait for: the whole lookup.
        t.title = format!("Nothing Like That {}", std::process::id());
        let at = Instant::now();
        let _ = crate::lyrics::online(&t);
        eprintln!("a full lyrics lookup took {:?}", at.elapsed());
        assert!(took < Duration::from_millis(1600), "{took:?}");
    }

    /// STARLITE_PREVIEW_FILE=<mp3> cargo test -- --ignored cut_downloads_still_decode
    #[test]
    #[ignore]
    fn cut_downloads_still_decode() {
        let Ok(f) = std::env::var("STARLITE_PREVIEW_FILE") else { return };
        let bytes = std::fs::read(f).unwrap();
        let full = Decoder::new(Cursor::new(bytes.clone())).unwrap();
        let (rate, ch) = (full.sample_rate() as u64, full.channels() as u64);
        let total = full.count() as u64;
        let mut dec = Decoder::new(Cursor::new(bytes[..bytes.len() / 2].to_vec())).unwrap();
        let skip = (rate * ch * 10) as usize; // 10 s, decoded through like a partial preview
        assert_eq!(dec.by_ref().take(skip).count(), skip);
        let rest = dec.count() as u64 + skip as u64;
        eprintln!("full {total} samples, half the bytes gave {rest}");
        assert!(rest > total * 2 / 5 && rest < total * 3 / 5, "{rest} of {total}");
    }

    /// Needs a Navidrome login in STARLITE_DATA_DIR and a sound device:
    /// cargo test -- --ignored navidrome_previews_start_quickly
    #[test]
    #[ignore]
    fn navidrome_previews_start_quickly() {
        let nd = crate::navidrome::Navidrome::load();
        let pls = nd.playlists().unwrap();
        let mut songs = Vec::new();
        for pl in pls.iter().take(3) {
            songs.extend(nd.playlist_tracks(&pl.id).unwrap().into_iter().take(3));
        }
        let mut p = Preview::new();
        p.set_volume(0.0);
        for t in songs.iter().take(6) {
            let Source::Navidrome { song_id, .. } = &t.source else { continue };
            let at = Instant::now();
            p.play(t, nd.preview_url(song_id), true);
            while !p.audible() && at.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(5));
            }
            let start = p.starts.lock().unwrap().get(&t.id).copied();
            eprintln!("{:>5} ms  (starts at {:?} s of {} s)", at.elapsed().as_millis(), start.map(|s| s / 1000), t.duration_ms / 1000);
            std::thread::sleep(Duration::from_millis(1500));
            // Real sound came through (the equalizer moves), not silence.
            let level: f32 = p.levels.get().iter().sum();
            assert!(level > 0.001, "silent preview for {}", t.title);
            p.stop();
            while p.audible() {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// A live stream reads like the whole file once it has all arrived.
    #[test]
    fn live_reads_while_arriving() {
        let grow = Arc::new(Growing::default());
        let gw = grow.clone();
        std::thread::spawn(move || {
            for chunk in [b"hello ".as_slice(), b"wide ", b"world"] {
                std::thread::sleep(Duration::from_millis(20));
                gw.data.lock().unwrap().0.extend_from_slice(chunk);
                gw.more.notify_all();
            }
            gw.data.lock().unwrap().1 = true;
            gw.more.notify_all();
        });
        let mut live = Live { g: grow, pos: 0 };
        let mut s = String::new();
        live.read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello wide world");
        assert_eq!(live.seek(SeekFrom::End(-5)).unwrap(), 11);
        let mut w = String::new();
        live.read_to_string(&mut w).unwrap();
        assert_eq!(w, "world");
    }

    #[test]
    fn previews_fade_in() {
        let rate = 8_000u32;
        let src = rodio::buffer::SamplesBuffer::new(1, rate, vec![1.0f32; rate as usize * 2]);
        let tap = Tap::new(src, Arc::new(Levels::default()), Arc::new(AtomicBool::new(false)));
        let out: Vec<f32> = tap.collect();
        assert!(out[0] < 0.01 && out[(rate as f32 * FADE_IN) as usize + 10] > 0.99);
    }
}
