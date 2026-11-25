use crate::{composition::MAX_DEPTH, model::Scene, render::Renderer};
use std::{collections::HashMap, path::Path};

pub(crate) struct VideoSpan<'a> {
    pub id: String,
    pub file: &'a Path,
    pub start: f64,
    pub end: f64,
    pub offset: f64,
    pub duration: f64,
    pub speed: f64,
}

#[derive(Clone, Copy)]
struct Window {
    start: f64,
    end: f64,
    time: f64,
    speed: f64,
}

impl Window {
    fn clip(self, start: f64, end: f64) -> Option<Self> {
        let local_end = self.time + (self.end - self.start) * self.speed;
        let start = start.max(self.time);
        let end = end.min(local_end);
        (end > start).then_some(Self {
            start: self.start + (start - self.time) / self.speed,
            end: self.start + (end - self.time) / self.speed,
            time: start,
            speed: self.speed,
        })
    }
}

pub(crate) fn video_spans<'a>(
    project: &Scene,
    renderer: &'a Renderer,
) -> Result<Vec<VideoSpan<'a>>, String> {
    let mut spans = Spans {
        project,
        renderer,
        contains_video: HashMap::new(),
        result: Vec::new(),
        windows: 0,
    };
    spans.visit(
        project,
        renderer,
        Window {
            start: 0.0,
            end: project.duration,
            time: 0.0,
            speed: 1.0,
        },
        "",
        0,
    )?;
    Ok(spans.result)
}

struct Spans<'a, 'b> {
    project: &'b Scene,
    renderer: &'a Renderer,
    contains_video: HashMap<String, bool>,
    result: Vec<VideoSpan<'a>>,
    windows: usize,
}

impl<'a> Spans<'a, '_> {
    fn has_video(&mut self, scene: &Scene, depth: usize) -> bool {
        if depth >= MAX_DEPTH {
            return false;
        }
        if let Some(&has_video) = self.contains_video.get(&scene.project_id) {
            return has_video;
        }
        let has_video = scene.shapes.iter().any(|shape| {
            shape.kind == "video"
                || shape
                    .composition
                    .as_ref()
                    .and_then(|playback| self.project.composition(&playback.source))
                    .is_some_and(|source| self.has_video(source, depth + 1))
        });
        self.contains_video
            .insert(scene.project_id.clone(), has_video);
        has_video
    }

    fn visit(
        &mut self,
        scene: &Scene,
        renderer: &'a Renderer,
        window: Window,
        path: &str,
        depth: usize,
    ) -> Result<(), String> {
        if depth >= MAX_DEPTH {
            return Ok(());
        }
        self.windows += 1;
        if self.windows > 4096 {
            return Err("Composition playback has too many video loop segments.".into());
        }
        for shape in &scene.shapes {
            let start = shape.clip_start();
            let end = shape.out_time.unwrap_or(scene.duration);
            let Some(clip) = window.clip(start, end) else {
                continue;
            };
            if let Some(file) = renderer.video(&shape.id).filter(|_| shape.kind == "video") {
                self.result.push(VideoSpan {
                    id: format!("{path}{}/{}", shape.id, clip.start.to_bits()),
                    file,
                    start: clip.start,
                    end: clip.end,
                    offset: shape
                        .extra
                        .get("mediaStart")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0)
                        + clip.time
                        - start,
                    duration: shape
                        .extra
                        .get("mediaDuration")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0),
                    speed: clip.speed,
                });
                continue;
            }
            let Some(playback) = &shape.composition else {
                continue;
            };
            let Some(source) = self.project.composition(&playback.source) else {
                continue;
            };
            let Some(renderer) = self.renderer.composition_renderer(&playback.source) else {
                continue;
            };
            let speed = clip.speed * playback.speed;
            if source.duration <= 0.0
                || !source.duration.is_finite()
                || speed <= 0.0
                || !speed.is_finite()
                || !self.has_video(source, depth + 1)
            {
                continue;
            }
            let mut time = playback.time(clip.time - start, source.duration);
            let mut cursor = clip.start;
            let path = format!("{path}{}/", shape.id);
            while cursor < clip.end && time < source.duration {
                let next = (cursor + (source.duration - time) / speed).min(clip.end);
                if next <= cursor {
                    break;
                }
                let child = Window {
                    start: cursor,
                    end: next,
                    time,
                    speed,
                };
                self.visit(source, renderer, child, &path, depth + 1)?;
                if next == clip.end || !playback.looping {
                    break;
                }
                cursor = next;
                time = 0.0;
            }
        }
        Ok(())
    }
}

pub(crate) fn tempo_filter(mut speed: f64) -> String {
    if !speed.is_finite() || speed <= 0.0 || (speed - 1.0).abs() < 0.000001 {
        return String::new();
    }
    let mut filters = Vec::new();
    while speed > 2.0 {
        filters.push("atempo=2,".to_owned());
        speed /= 2.0;
    }
    while speed < 0.5 {
        filters.push("atempo=0.5,".to_owned());
        speed *= 2.0;
    }
    filters.push(format!("atempo={speed:.8},"));
    filters.concat()
}
