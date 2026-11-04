//! Video-layer sound during editor playback. FFmpeg decodes on worker threads; the device callback only mixes PCM.

use crate::{media, model::Scene, render::Renderer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::collections::{HashMap, HashSet, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOOKAHEAD: f64 = 2.0;
const STALE: Duration = Duration::from_millis(400);

#[derive(Clone, Debug, PartialEq)]
struct Clip {
    id: String,
    path: PathBuf,
    start: f64,
    end: f64,
    media_start: f64,
    media_duration: f64,
}

impl Clip {
    fn source_at(&self, time: f64) -> f64 {
        let offset = (time - self.start).max(0.0) + self.media_start;
        if self.media_duration > 0.0 {
            offset.rem_euclid(self.media_duration)
        } else {
            offset
        }
    }
}

fn clips(scene: &Scene, renderer: &Renderer) -> Vec<Clip> {
    scene
        .shapes
        .iter()
        .filter(|shape| shape.kind == "video")
        .filter_map(|shape| {
            let path = renderer.video(&shape.id)?.to_owned();
            Some(Clip {
                id: shape.id.clone(),
                path,
                start: shape.clip_start(),
                end: shape.out_time.unwrap_or(scene.duration).min(scene.duration),
                media_start: shape
                    .extra
                    .get("mediaStart")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                media_duration: shape
                    .extra
                    .get("mediaDuration")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            })
        })
        .collect()
}

fn signature(clips: &[Clip]) -> u64 {
    let mut hash = DefaultHasher::new();
    for clip in clips {
        clip.id.hash(&mut hash);
        clip.path.hash(&mut hash);
        for value in [clip.start, clip.end, clip.media_start, clip.media_duration] {
            value.to_bits().hash(&mut hash);
        }
    }
    hash.finish()
}

fn near(clip: &Clip, time: f64) -> bool {
    clip.end > time && clip.start <= time + LOOKAHEAD
}

fn frames_at(time: f64, origin: f64, rate: u32) -> u64 {
    ((time - origin).max(0.0) * rate as f64).round() as u64
}

struct Voice {
    next: u64,
    end: u64,
    chunks: Receiver<Vec<f32>>,
    buffer: VecDeque<f32>,
    owed: usize,
    done: bool,
}

impl Voice {
    fn pull(&mut self, wanted: usize) {
        while self.buffer.len() < wanted {
            match self.chunks.try_recv() {
                Ok(chunk) => {
                    let skip = self.owed.min(chunk.len());
                    self.owed -= skip;
                    self.buffer.extend(&chunk[skip..]);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.done = true;
                    break;
                }
            }
        }
    }

    fn skip(&mut self, samples: usize) {
        let buffered = samples.min(self.buffer.len());
        self.buffer.drain(..buffered);
        self.owed += samples - buffered;
    }
}

#[derive(Default)]
struct Mix {
    channels: usize,
    running: bool,
    voices: Vec<Voice>,
    played: u64,
    seen: Option<Instant>,
    stalled: bool,
}

impl Mix {
    fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        if !self.running || self.stalled {
            return;
        }
        if self.seen.is_some_and(|seen| seen.elapsed() > STALE) {
            self.stalled = true;
            return;
        }
        let channels = self.channels.max(1);
        let frames = (out.len() / channels) as u64;
        let from = self.played;
        for voice in &mut self.voices {
            if voice.next < from {
                voice.skip((from - voice.next) as usize * channels);
                voice.next = from;
            }
            let last = voice.end.min(from + frames);
            if voice.next >= last {
                continue;
            }
            let wanted = (last - voice.next) as usize * channels;
            voice.pull(wanted);
            let have = wanted.min(voice.buffer.len());
            let offset = (voice.next - from) as usize * channels;
            for (slot, sample) in out[offset..offset + have]
                .iter_mut()
                .zip(voice.buffer.drain(..have))
            {
                *slot += sample;
            }
            voice.owed += wanted - have;
            voice.next = last;
        }
        self.played += frames;
        self.voices.retain(|voice| voice.end > self.played);
        // The device's PCM range is bounded. This is a final sample clip, not per-voice normalization.
        for sample in out {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

struct Output {
    stream: cpal::Stream,
    rate: u32,
    channels: usize,
    errored: Arc<AtomicBool>,
}

enum Probe {
    Pending(Receiver<Result<media::Info, String>>),
    Ready(Option<media::Info>),
}

/// Session-scoped audio playback. `notice` is returned once for device or FFmpeg failure.
#[derive(Default)]
pub struct Sound {
    mix: Arc<Mutex<Mix>>,
    output: Option<Output>,
    probes: HashMap<PathBuf, Probe>,
    started: HashSet<String>,
    decoders: Vec<Arc<AtomicBool>>,
    decoder_errors: Vec<Receiver<String>>,
    origin: f64,
    signature: u64,
    active: bool,
    failed: bool,
    warned: bool,
    notice: Option<String>,
}

impl Sound {
    fn mix(&self) -> std::sync::MutexGuard<'_, Mix> {
        self.mix.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fail(&mut self, message: String) {
        self.pause();
        self.failed = true;
        self.output = None;
        if !self.warned {
            self.notice = Some(message);
            self.warned = true;
        }
    }

    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    pub fn pause(&mut self) {
        self.active = false;
        self.mix().running = false;
        self.mix().voices.clear();
        for cancel in self.decoders.drain(..) {
            cancel.store(true, Ordering::Relaxed);
        }
        self.decoder_errors.clear();
        self.started.clear();
        if let Some(output) = &self.output {
            let _ = output.stream.pause();
        }
    }

    fn probe(&mut self, path: &Path) {
        if self.probes.contains_key(path) {
            return;
        }
        let (send, recv) = mpsc::channel();
        let path = path.to_owned();
        let worker_path = path.clone();
        std::thread::spawn(move || {
            let _ = send.send(media::probe(&worker_path));
        });
        self.probes.insert(path, Probe::Pending(recv));
    }

    fn poll_probes(&mut self) {
        let mut missing_ffmpeg = false;
        for state in self.probes.values_mut() {
            let result = match state {
                Probe::Pending(recv) => recv.try_recv().ok(),
                Probe::Ready(_) => None,
            };
            if let Some(result) = result {
                missing_ffmpeg |= result
                    .as_ref()
                    .err()
                    .is_some_and(|e| e.contains("FFprobe couldn't start"));
                *state = Probe::Ready(result.ok());
            }
        }
        if missing_ffmpeg {
            self.fail("FFmpeg is unavailable. Video sound will be silent.".into());
        }
    }

    fn open(&mut self) -> Result<(), String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("No sound output is connected")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        let channels = config.channels as usize;
        self.mix().channels = channels;
        let mix = self.mix.clone();
        let errored = Arc::new(AtomicBool::new(false));
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, config.clone(), mix, errored.clone()),
            SampleFormat::I16 => build::<i16>(&device, config.clone(), mix, errored.clone()),
            SampleFormat::I32 => build::<i32>(&device, config.clone(), mix, errored.clone()),
            SampleFormat::U16 => build::<u16>(&device, config.clone(), mix, errored.clone()),
            other => return Err(format!("Unsupported sound output sample format: {other}")),
        }?;
        self.output = Some(Output {
            stream,
            rate: config.sample_rate,
            channels,
            errored,
        });
        Ok(())
    }

    fn restart(&mut self, clips: &[Clip], time: f64) {
        self.pause();
        if self.failed {
            return;
        }
        self.origin = time;
        self.signature = signature(clips);
        self.active = true;
        {
            let mut mix = self.mix();
            mix.played = 0;
            mix.seen = Some(Instant::now());
            mix.stalled = false;
            mix.running = true;
        }
        self.schedule(clips, time);
        if self.output.is_some() {
            let result = self.output.as_ref().unwrap().stream.play();
            if let Err(error) = result {
                self.fail(format!("Video sound is unavailable: {error}"));
            }
        }
    }

    fn schedule(&mut self, clips: &[Clip], time: f64) {
        let Some((rate, channels)) = self.output.as_ref().map(|o| (o.rate, o.channels)) else {
            return;
        };
        for clip in clips {
            if self.started.contains(&clip.id) || !near(clip, time) {
                continue;
            }
            let Some(Probe::Ready(Some(info))) = self.probes.get(&clip.path) else {
                continue;
            };
            if !info.sound {
                continue;
            }
            let begin = clip.start.max(time);
            let mut source_clip = clip.clone();
            if source_clip.media_duration <= 0.0 {
                source_clip.media_duration = info.duration;
            }
            let source = source_clip.source_at(begin);
            let length = clip.end - begin;
            let (chunks, cancel, errors) =
                decode(clip.path.clone(), source, length, rate, channels);
            self.mix().voices.push(Voice {
                next: frames_at(begin, self.origin, rate),
                end: frames_at(clip.end, self.origin, rate),
                chunks,
                buffer: VecDeque::new(),
                owed: 0,
                done: false,
            });
            self.decoders.push(cancel);
            self.decoder_errors.push(errors);
            self.started.insert(clip.id.clone());
        }
    }

    /// Reconcile after the UI has processed actions. Returns false if playback must stop after a hidden pane.
    pub fn update(
        &mut self,
        scene: &Scene,
        renderer: Option<&Renderer>,
        time: f64,
        playing: bool,
    ) -> bool {
        if !playing {
            if self.active {
                self.pause();
            }
            return true;
        }
        if self.failed {
            return true;
        }
        if self
            .output
            .as_ref()
            .is_some_and(|o| o.errored.load(Ordering::Relaxed))
        {
            self.fail("Video sound output stopped. Playback will be silent.".into());
            return true;
        }
        let Some(renderer) = renderer else {
            return true;
        };
        let clips = clips(scene, renderer);
        for clip in &clips {
            if near(clip, time) {
                self.probe(&clip.path);
            }
        }
        self.poll_probes();
        if self.failed {
            return true;
        }
        if self
            .decoder_errors
            .iter()
            .any(|recv| recv.try_recv().is_ok())
        {
            self.fail("FFmpeg is unavailable. Video sound will be silent.".into());
            return true;
        }
        let stalled = self.mix().stalled;
        if stalled {
            self.pause();
            return false;
        }
        let changed = signature(&clips) != self.signature;
        let sound_ready = self.output.is_none() && clips.iter().filter(|clip| near(clip, time)).any(|clip| {
            matches!(self.probes.get(&clip.path), Some(Probe::Ready(Some(info))) if info.sound)
        });
        let drift = self.output.as_ref().is_some_and(|o| {
            let mix = self.mix();
            mix.played > 0 && (self.origin + mix.played as f64 / o.rate as f64 - time).abs() > 0.2
        });
        if !self.active || changed || drift || sound_ready {
            if sound_ready {
                if let Err(error) = self.open() {
                    self.fail(format!("Video sound is unavailable: {error}"));
                    return true;
                }
            }
            self.restart(&clips, time);
        } else {
            self.mix().seen = Some(Instant::now());
            self.schedule(&clips, time);
        }
        true
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        self.pause();
    }
}

fn decode(
    path: PathBuf,
    source: f64,
    length: f64,
    rate: u32,
    channels: usize,
) -> (Receiver<Vec<f32>>, Arc<AtomicBool>, Receiver<String>) {
    let (send, recv) = mpsc::sync_channel(32);
    let (error_send, error_recv) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    std::thread::spawn(move || {
        let child = media::command(media::ffmpeg())
            .args([
                "-v",
                "error",
                "-stream_loop",
                "-1",
                "-ss",
                &format!("{source:.6}"),
                "-i",
            ])
            .arg(path)
            .args([
                "-t",
                &format!("{length:.6}"),
                "-vn",
                "-ac",
                &channels.to_string(),
                "-ar",
                &rate.to_string(),
                "-f",
                "f32le",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            let _ = error_send.send("FFmpeg couldn't start".into());
            return;
        };
        if let Some(mut stdout) = child.stdout.take() {
            let mut bytes = vec![0u8; 8192];
            let mut remainder = Vec::new();
            while !worker_cancel.load(Ordering::Relaxed) {
                let Ok(n) = stdout.read(&mut bytes) else {
                    break;
                };
                if n == 0 {
                    break;
                }
                remainder.extend_from_slice(&bytes[..n]);
                let complete = remainder.len() / 4 * 4;
                let samples = remainder[..complete]
                    .chunks_exact(4)
                    .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
                    .collect::<Vec<_>>();
                remainder.drain(..complete);
                let mut pending = samples;
                loop {
                    if worker_cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    match send.try_send(pending) {
                        Ok(()) => break,
                        Err(mpsc::TrySendError::Full(chunk)) => {
                            pending = chunk;
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(mpsc::TrySendError::Disconnected(_)) => {
                            worker_cancel.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            }
        }
        if worker_cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
        }
        let _ = child.wait();
    });
    (recv, cancel, error_recv)
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mix: Arc<Mutex<Mix>>,
    errored: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let mut scratch = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                scratch.resize(data.len(), 0.0);
                if let Ok(mut mix) = mix.lock() {
                    mix.fill(&mut scratch);
                } else {
                    scratch.fill(0.0);
                }
                for (output, sample) in data.iter_mut().zip(&scratch) {
                    *output = T::from_sample(*sample);
                }
            },
            move |_error| {
                errored.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn voice(next: u64, end: u64, data: Vec<f32>) -> Voice {
        let (tx, chunks) = mpsc::channel();
        tx.send(data).unwrap();
        Voice {
            next,
            end,
            chunks,
            buffer: VecDeque::new(),
            owed: 0,
            done: false,
        }
    }
    #[test]
    fn mixes_voices_at_their_timeline_frames() {
        let mut mix = Mix {
            channels: 1,
            running: true,
            voices: vec![voice(0, 3, vec![0.2; 3]), voice(2, 4, vec![0.3; 2])],
            ..Mix::default()
        };
        let mut out = [0.0; 4];
        mix.fill(&mut out);
        assert_eq!(out, [0.2, 0.2, 0.5, 0.3]);
    }
    #[test]
    fn late_voice_skips_samples_it_missed() {
        let (tx, rx) = mpsc::channel();
        let mut mix = Mix {
            channels: 1,
            running: true,
            voices: vec![Voice {
                next: 0,
                end: 8,
                chunks: rx,
                buffer: VecDeque::new(),
                owed: 0,
                done: false,
            }],
            ..Mix::default()
        };
        let mut out = [0.0; 2];
        mix.fill(&mut out);
        tx.send(vec![1.0, 2.0, 0.3, 0.4]).unwrap();
        mix.fill(&mut out);
        assert_eq!(out, [0.3, 0.4]);
    }
    /// The real path: FFmpeg decodes a quiet tone and the default output device plays it for about a second.
    #[test]
    #[ignore = "plays a quiet tone through the sound card"]
    fn a_video_sound_reaches_the_output_device() {
        let dir = std::env::temp_dir().join(format!("forma-sound-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("tone.mp4");
        let made = media::command(media::ffmpeg())
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=gray:s=64x64:r=10:d=2",
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:d=2",
                "-af",
                "volume=0.05",
                "-shortest",
            ])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
            .arg(&file)
            .status()
            .unwrap();
        assert!(made.success());
        let mut sound = Sound::default();
        sound.open().expect("an output device");
        let (rate, channels) = {
            let output = sound.output.as_ref().unwrap();
            (output.rate, output.channels)
        };
        let (chunks, cancel, _errors) = decode(file.clone(), 0.0, 1.5, rate, channels);
        {
            let mut mix = sound.mix();
            mix.running = true;
            mix.seen = None;
            mix.voices.push(Voice {
                next: 0,
                end: (rate as f64 * 1.5) as u64,
                chunks,
                buffer: VecDeque::new(),
                owed: 0,
                done: false,
            });
        }
        sound.output.as_ref().unwrap().stream.play().unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        let (played, owed) = {
            let mix = sound.mix();
            (mix.played, mix.voices.first().map_or(0, |v| v.owed))
        };
        cancel.store(true, Ordering::Relaxed);
        sound.pause();
        drop(sound);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(played > rate as u64, "the device took {played} frames");
        // Only FFmpeg's start-up should come late: well under half a second of it.
        assert!(
            owed < rate as usize * channels / 2,
            "{owed} samples came too late"
        );
    }

    #[test]
    fn clip_selection_and_loop_offset() {
        let clip = Clip {
            id: "a".into(),
            path: PathBuf::new(),
            start: 2.0,
            end: 5.0,
            media_start: 1.5,
            media_duration: 2.0,
        };
        assert_eq!(clip.source_at(2.0), 1.5);
        assert_eq!(clip.source_at(3.0), 0.5);
        assert_eq!(clip.source_at(4.5), 0.0);
        assert!(!near(&clip, -1.0));
        assert!(near(&clip, 0.0));
        assert!(near(&clip, 4.0));
        assert!(!near(&clip, 5.0));
        assert_eq!(frames_at(clip.start, 1.0, 100), 100);
        assert_eq!(frames_at(clip.end, 1.0, 100), 400);
        assert_eq!(frames_at(3.0, 3.0, 100), 0);
    }
}
