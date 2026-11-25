//! Video layers in the editor: while paused, each shows the exact frame at the playhead, read on a thread of its own
//! (its poster shows until the first one arrives); while playing, each reads a stream of frames from where it was, as
//! StreamCuts' preview does. Frames are decoded at the size the layer is shown.

use crate::media;
use crate::model::{Scene, Shape};
use crate::motion_blur;
use crate::render::{self, Renderer};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, channel, sync_channel};
use tiny_skia::Pixmap;

const FPS: f64 = 30.0;

/// Which frame a layer shows: its frame number in the file and the size it was decoded at.
type Key = (i64, u32, u32);

#[derive(Default)]
pub struct Videos {
    /// What each layer shows now.
    pub frames: render::Frames,
    /// Goes up whenever `frames` changes, so the preview knows to draw again.
    pub generation: u64,
    shown: HashMap<String, Key>,
    stills: HashMap<String, (Key, Receiver<Option<Pixmap>>)>,
    streams: HashMap<String, Stream>,
    shutter: HashMap<String, ShutterPreview>,
}

/// Frames from one FFmpeg run: frame `n` belongs at scene time `from + n / 30`.
struct Stream {
    from: f64,
    read: u64,
    size: (u32, u32),
    frames: Receiver<Pixmap>,
    ahead: Option<Pixmap>,
}

impl Videos {
    /// Brings the shown frames up to `time`, for a preview drawn at `scale` pixels per scene pixel. Returns whether
    /// it is still waiting for frames (the editor should draw again soon).
    pub fn update(
        &mut self,
        scene: &Scene,
        renderer: &Renderer,
        time: f64,
        playing: bool,
        scale: f32,
    ) -> bool {
        let showing: Vec<_> = scene
            .shapes
            .iter()
            .filter(|s| s.kind == "video" && s.visible(time, scene.duration))
            .collect();
        let before = self.frames.len();
        self.frames
            .retain(|id, _| showing.iter().any(|s| &s.id == id));
        self.shown
            .retain(|id, _| showing.iter().any(|s| &s.id == id));
        self.streams
            .retain(|id, _| playing && showing.iter().any(|s| &s.id == id));
        if self.frames.len() != before {
            self.generation += 1;
        }
        let mut waiting = false;
        for shape in showing {
            let (Some(file), Some(duration)) = (
                renderer.video(&shape.id),
                shape.extra.get("mediaDuration").and_then(|d| d.as_f64()),
            ) else {
                continue;
            };
            let pose = scene.render_pose(shape, time);
            let size = (
                (pose.w * scale).round().clamp(2.0, 1920.0) as u32,
                (pose.h * scale).round().clamp(2.0, 1920.0) as u32,
            );
            let at = shape.media_time(time, duration);
            if playing {
                let fresh = self.streams.get(&shape.id).is_some_and(|s| {
                    s.size == size && (s.from + s.read as f64 / FPS - time).abs() < 0.3
                });
                if !fresh {
                    self.streams.insert(
                        shape.id.clone(),
                        Stream::start(file.to_owned(), at, time, size),
                    );
                }
                if let Some(frame) = self.streams.get_mut(&shape.id).unwrap().due(time) {
                    self.frames.insert(shape.id.clone(), frame);
                    self.generation += 1;
                }
                waiting = true;
                continue;
            }
            let key = ((at * FPS).round() as i64, size.0, size.1);
            if let Some((asked, still)) = self.stills.get(&shape.id) {
                match still.try_recv() {
                    Ok(frame) => {
                        let asked = *asked;
                        self.stills.remove(&shape.id);
                        if let Some(frame) = frame {
                            self.frames.insert(shape.id.clone(), frame);
                            self.shown.insert(shape.id.clone(), asked);
                            self.generation += 1;
                        }
                    }
                    Err(TryRecvError::Empty) => {
                        waiting = true;
                        continue;
                    }
                    Err(TryRecvError::Disconnected) => {
                        self.stills.remove(&shape.id);
                    }
                }
            }
            // One still at a time per layer: when it arrives, the next asks for wherever the playhead is by then.
            if self.shown.get(&shape.id) != Some(&key) && !self.stills.contains_key(&shape.id) {
                let (send, receive) = channel();
                let file = file.to_owned();
                std::thread::spawn(move || {
                    let frame =
                        media::Frames::start(&file, key.0 as f64 / FPS, FPS as u32, key.1, key.2)
                            .and_then(|mut f| f.next())
                            .ok();
                    let _ = send.send(frame);
                });
                self.stills.insert(shape.id.clone(), (key, receive));
                waiting = true;
            }
        }
        self.update_shutter(scene, renderer, time, scale) || waiting
    }

    fn update_shutter(
        &mut self,
        scene: &Scene,
        renderer: &Renderer,
        time: f64,
        scale: f32,
    ) -> bool {
        let fps = motion_blur::DEFAULT_FPS;
        let exposure = scene
            .motion_blur_preview
            .then(|| motion_blur::exposure(scene, time, fps))
            .unwrap_or_default();
        let mut showing = HashSet::new();
        let mut waiting = false;
        for shape in scene
            .shapes
            .iter()
            .filter(|shape| shape.kind == "video" && shape.motion_blur)
        {
            let visible: Vec<_> = exposure
                .iter()
                .copied()
                .filter(|&t| shape.visible(t, scene.duration))
                .collect();
            let (Some(&first), Some(&last), Some(file), Some(duration)) = (
                visible.first(),
                visible.last(),
                renderer.video(&shape.id),
                shape
                    .extra
                    .get("mediaDuration")
                    .and_then(|value| value.as_f64()),
            ) else {
                continue;
            };
            let (w, h) = visible
                .iter()
                .map(|&t| scene.render_pose(shape, t))
                .fold((2.0_f32, 2.0_f32), |(w, h), pose| {
                    (w.max(pose.w * scale), h.max(pose.h * scale))
                });
            let source = ShutterSource {
                file: file.to_owned(),
                duration,
                size: (
                    w.round().clamp(2.0, 1920.0) as u32,
                    h.round().clamp(2.0, 1920.0) as u32,
                ),
                clip_start: shape.clip_start(),
                media_start: shape.media_time(shape.clip_start(), duration),
            };
            showing.insert(shape.id.as_str());
            let start = media::ShutterReader::step(source.clip_start, first, fps);
            let end = media::ShutterReader::step(source.clip_start, last, fps);
            let preview = self
                .shutter
                .entry(shape.id.clone())
                .or_insert_with(|| ShutterPreview::start(source.clone(), shape.clone()));
            if preview.source != source {
                *preview = ShutterPreview::start(source, shape.clone());
                self.generation += 1;
            }
            let result = preview.update((start, end));
            waiting |= result.0;
            if result.1 {
                self.generation += 1;
            }
        }
        let before = self.shutter.len();
        self.shutter.retain(|id, _| showing.contains(id.as_str()));
        if before != self.shutter.len() {
            self.generation += 1;
        }
        waiting
    }

    pub(crate) fn sample(&self, time: f64) -> Option<render::SampleFrames> {
        if self.shutter.is_empty() {
            return None;
        }
        Some(
            self.shutter
                .iter()
                .filter_map(|(id, preview)| {
                    let step = media::ShutterReader::step(
                        preview.source.clip_start,
                        time,
                        motion_blur::DEFAULT_FPS,
                    );
                    Some((id.clone(), preview.pixels.get(&step)?.clone()))
                })
                .collect(),
        )
    }
}

#[derive(Clone, PartialEq)]
struct ShutterSource {
    file: PathBuf,
    duration: f64,
    size: (u32, u32),
    clip_start: f64,
    media_start: f64,
}

type ShutterRange = (i64, i64);
type ShutterPictures = HashMap<i64, Arc<Pixmap>>;

struct ShutterPreview {
    source: ShutterSource,
    request: SyncSender<ShutterRange>,
    response: Receiver<ShutterPictures>,
    pending: bool,
    requested: Option<ShutterRange>,
    pixels: ShutterPictures,
}

impl ShutterPreview {
    fn start(source: ShutterSource, shape: Shape) -> Self {
        let (request, incoming) = sync_channel::<ShutterRange>(1);
        let (outgoing, response) = channel();
        let worker = source.clone();
        std::thread::spawn(move || {
            let mut reader = None;
            let fps = motion_blur::DEFAULT_FPS;
            while let Ok((start, end)) = incoming.recv() {
                let mut pictures = HashMap::new();
                for step in start..=end {
                    let time = worker.clip_start + step as f64 / f64::from(fps.max(60));
                    let source = (worker.file.as_path(), worker.duration, worker.size);
                    match media::ShutterReader::at(&mut reader, source, &shape, time, fps) {
                        Ok(picture) => {
                            pictures.insert(step, picture);
                        }
                        Err(_) => {
                            reader = None;
                            break;
                        }
                    }
                }
                if outgoing.send(pictures).is_err() {
                    break;
                }
            }
        });
        Self {
            source,
            request,
            response,
            pending: false,
            requested: None,
            pixels: HashMap::new(),
        }
    }

    fn update(&mut self, range: ShutterRange) -> (bool, bool) {
        let mut received = false;
        if self.pending {
            match self.response.try_recv() {
                Ok(pictures) => {
                    self.pixels = pictures;
                    self.pending = false;
                    received = true;
                }
                Err(TryRecvError::Empty) => return (true, false),
                Err(TryRecvError::Disconnected) => return (false, false),
            }
        }
        if self.requested != Some(range) {
            self.pending = self.request.try_send(range).is_ok();
            self.requested = Some(range);
        }
        (self.pending, received)
    }
}

impl Stream {
    fn start(file: std::path::PathBuf, at: f64, from: f64, size: (u32, u32)) -> Self {
        let (send, frames): (SyncSender<Pixmap>, _) = sync_channel(3);
        std::thread::spawn(move || {
            let Ok(mut reader) = media::Frames::start(&file, at, FPS as u32, size.0, size.1) else {
                return;
            };
            // Ends when the editor drops the stream (the send fails) or the video does.
            while let Ok(frame) = reader.next() {
                if send.send(frame).is_err() {
                    break;
                }
            }
        });
        Self {
            from,
            read: 0,
            size,
            frames,
            ahead: None,
        }
    }

    /// The newest frame that is due at `time`, if a new one is.
    fn due(&mut self, time: f64) -> Option<Pixmap> {
        let mut newest = None;
        loop {
            let Some(frame) = self.ahead.take().or_else(|| self.frames.try_recv().ok()) else {
                break;
            };
            if self.from + self.read as f64 / FPS > time + 0.5 / FPS {
                self.ahead = Some(frame);
                break;
            }
            self.read += 1;
            newest = Some(frame);
        }
        newest
    }
}
