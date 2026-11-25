//! Frame-accurate software rendering of a scene, the way Electron Forma's canvas paints it (`src/render/paint.js`).
//! Exports and the editor's preview both draw through it, so what the editor shows is what gets exported.

use crate::compositing::{Blend, MatteLinks};
use crate::effect_render::{self, Region};
use crate::effects::Sample;
use crate::mask_render::{LayerPixels, MaskCache};
use crate::model::{MAX_BLUR, Scene, Shape};
use crate::motion_blur;
use crate::spatial::{Camera, Plane, depth_order};
use crate::text_layout::TextCache;
use crate::text_render::{TextInk, TextPaths};
use crate::transform::LayerFrame;
use crate::vector_render::{GeometryCache, VectorInk};
use crate::{assets, svg, text};
use canopy_ui::Place;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tiny_skia::{
    Color, FillRule, IntRect, IntSize, Paint, Path as Outline, PathBuilder, Pixmap, PixmapPaint,
    Point, Rect, Transform,
};

/// Video layers' frames at one moment, by layer id.
pub type Frames = HashMap<String, Pixmap>;
pub(crate) type SampleFrames = HashMap<String, Arc<Pixmap>>;
type ShutterSample<'a> = (f64, Option<&'a SampleFrames>);

pub(crate) type TextHits = HashMap<String, Vec<Rect>>;

/// What the editor sees (see `Renderer::view`): the scene point at the top-left, pixels to a scene pixel, and the
/// size in pixels.
pub type View = ((f32, f32), f32, (u32, u32));

pub struct Renderer {
    /// Pictures, SVGs (drawn once), and video posters, by layer id.
    images: HashMap<String, Pixmap>,
    /// Video layers' files.
    videos: HashMap<String, PathBuf>,
    /// Picture layers whose picture couldn't be read, and why. Exports stop on them; the editor leaves them out.
    missing: HashMap<String, String>,
    /// What each media layer was read from (hashed), so the editor knows when to read again.
    sources: Vec<(String, Option<u64>)>,
    masks: Mutex<MaskCache>,
    effects: Mutex<effect_render::Cache>,
    geometry: Mutex<GeometryCache>,
    text: Mutex<(TextCache, TextPaths)>,
    text_hits: Mutex<TextHits>,
    compositions: HashMap<String, Renderer>,
    nested_video: Mutex<HashMap<String, NestedVideo>>,
    preview_videos: Mutex<HashMap<u64, (u64, crate::videos::Videos)>>,
    preview_pending: AtomicBool,
    preview_epoch: AtomicU64,
    layer_pixels: Mutex<crate::preview_cache::Cache<LayerSample, Arc<LayerPixels>>>,
}

struct NestedVideo {
    info: crate::media::Info,
    reader: Option<crate::media::ShutterReader>,
    size: (u32, u32),
}

type CompositionFrames = Mutex<HashMap<(String, u64, u32, u32), Arc<Pixmap>>>;

#[derive(Clone, Copy)]
struct CompositionRender<'a> {
    renderer: &'a Renderer,
    project: &'a Scene,
    pixels: &'a CompositionFrames,
    depth: usize,
    fps: u32,
    instance: u64,
}

impl CompositionRender<'_> {
    fn enter(self, layer: &str) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (self.instance, layer).hash(&mut hash);
        Self { depth: self.depth + 1, instance: hash.finish(), ..self }
    }
}

#[derive(Clone, Copy)]
struct RenderFrame<'a> {
    renderer: &'a Renderer,
    scene: &'a Scene,
    exposure_scene: Option<&'a Scene>,
    time: f64,
    view: Transform,
    scale: f32,
    size: (u32, u32),
    strict: bool,
    cache_pixels: bool,
    videos: &'a Frames,
    sampled_videos: Option<&'a SampleFrames>,
    compositions: CompositionRender<'a>,
}

#[derive(Clone, PartialEq)]
struct LayerSample {
    id: String,
    pose: crate::model::Pose,
    transform: Transform,
    space: Transform,
    view: Transform,
    size: (u32, u32),
    color: String,
    time: Option<u64>,
    projection: Option<(Camera, [[f32; 3]; 3])>,
}

impl LayerSample {
    fn of(active: &RenderFrame<'_>, shape: &Shape, frame: LayerFrame) -> Option<Self> {
        if !active.cache_pixels
            || matches!(shape.kind.as_str(), "video" | "composition")
            || shape
                .text_layout
                .as_ref()
                .is_some_and(|text| text.path.is_some())
        {
            return None;
        }
        let animated = shape
            .vector_times()
            .chain(shape.mask_times())
            .chain(shape.text_times())
            .chain(shape.effect_times())
            .next()
            .is_some()
            || shape.expressions.iter().any(|expression| {
                !matches!(
                    expression.lane,
                    crate::keyframes::Lane::Pose(_)
                        | crate::keyframes::Lane::Color
                        | crate::keyframes::Lane::Spatial(_)
                )
            });
        Some(Self {
            id: shape.id.clone(),
            pose: frame.pose,
            transform: frame.transform,
            space: frame.space,
            view: active.view,
            size: active.size,
            color: shape.color_at(active.time),
            time: animated.then(|| active.time.to_bits()),
            projection: None,
        })
    }
}

struct MatteFrame {
    links: MatteLinks,
    cache: HashMap<usize, Option<Arc<LayerPixels>>>,
    bytes: usize,
}

/// What a layer puts on the canvas, in canvas pixels.
enum Ink<'a> {
    Fill(Outline, Paint<'static>),
    Vector(VectorInk),
    Text(TextInk),
    Picture(PicturePixels<'a>, PixmapPaint, Transform),
}

enum PicturePixels<'a> {
    Borrowed(&'a Pixmap),
    Composition(Arc<Pixmap>),
}

impl PicturePixels<'_> {
    fn pixmap(&self) -> &Pixmap {
        match self {
            Self::Borrowed(pixels) => pixels,
            Self::Composition(pixels) => pixels,
        }
    }
}

impl Ink<'_> {
    fn group_opacity(&self) -> f32 {
        match self {
            Ink::Vector(ink) => ink.opacity,
            Ink::Text(ink) => ink.opacity,
            _ => 1.0,
        }
    }

    fn bounds(&self) -> Option<Rect> {
        match self {
            Ink::Fill(outline, _) => Some(outline.bounds()),
            Ink::Vector(ink) => ink.bounds(),
            Ink::Text(ink) => ink.bounds(),
            Ink::Picture(pixmap, _, transform) => {
                let pixmap = pixmap.pixmap();
                let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
                let mut corners = [
                    Point::from_xy(0.0, 0.0),
                    Point::from_xy(w, 0.0),
                    Point::from_xy(w, h),
                    Point::from_xy(0.0, h),
                ];
                transform.map_points(&mut corners);
                Rect::from_points(&corners)
            }
        }
    }

    fn paint(&self, target: &mut Pixmap, offset: Transform) {
        match self {
            Ink::Fill(outline, paint) => {
                target.fill_path(outline, paint, FillRule::Winding, offset, None)
            }
            Ink::Vector(ink) => ink.paint(target, offset),
            Ink::Text(ink) => ink.paint(target, offset),
            Ink::Picture(pixmap, paint, transform) => target.draw_pixmap(
                0,
                0,
                pixmap.pixmap().as_ref(),
                paint,
                transform.post_concat(offset),
                None,
            ),
        }
    }
}

impl Renderer {
    pub fn new(scene: &Scene, place: &Place) -> Self {
        let mut images = HashMap::new();
        let mut missing = HashMap::new();
        let mut videos = HashMap::new();
        for shape in scene.shapes.iter().filter(|s| s.kind == "video") {
            if let Some(file) = assets::stored(shape, "assetData", place)
                .ok()
                .flatten()
                .and_then(|url| assets::library_file(&url, place))
            {
                videos.insert(shape.id.clone(), file);
            }
        }
        for shape in &scene.shapes {
            // Video layers show their poster until video frames are drawn natively.
            let read = match shape.kind.as_str() {
                "image" | "video" => picture(shape, place),
                "svg" => svg_picture(shape, place),
                _ => continue,
            };
            match read {
                Ok(pixmap) => {
                    images.insert(shape.id.clone(), pixmap);
                }
                Err(error) => {
                    missing.insert(shape.id.clone(), error);
                }
            }
        }
        Self {
            images,
            videos,
            missing,
            sources: sources(scene),
            masks: Mutex::default(),
            effects: Mutex::default(),
            geometry: Mutex::default(),
            text: Mutex::default(),
            text_hits: Mutex::default(),
            compositions: scene.compositions.iter().map(|comp| {
                (comp.project_id.clone(), Self::new(comp, place))
            }).collect(),
            nested_video: Mutex::default(),
            preview_videos: Mutex::default(),
            preview_pending: AtomicBool::default(),
            preview_epoch: AtomicU64::default(),
            layer_pixels: Mutex::new(crate::preview_cache::Cache::new(
                crate::preview_cache::LAYER_BYTES,
            )),
        }
    }

    pub(crate) fn invalidate_preview_pixels(&self) {
        self.layer_pixels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .invalidate();
        for renderer in self.compositions.values() {
            renderer.invalidate_preview_pixels();
        }
    }

    fn has_video(&self) -> bool {
        !self.videos.is_empty() || self.has_nested_video()
    }

    pub(crate) fn has_nested_video(&self) -> bool {
        self.compositions.values().any(Self::has_video)
    }

    pub(crate) fn text_hits_snapshot(&self) -> TextHits {
        self.text_hits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn restore_text_hits(&self, hits: TextHits) {
        *self
            .text_hits
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = hits;
    }

    /// Whether this renderer has read the pictures `scene` shows.
    pub fn matches(&self, scene: &Scene) -> bool {
        self.sources == sources(scene)
            && self.compositions.len() == scene.compositions.len()
            && scene.compositions.iter().all(|comp| self.compositions.get(&comp.project_id)
                .is_some_and(|renderer| renderer.matches(comp)))
    }

    /// Whether every layer can be exported: none is left out of an export.
    pub fn check(&self, scene: &Scene) -> Result<(), String> {
        scene.validate_compositions()?;
        self.check_composition(scene, self, scene, &mut std::collections::HashSet::new())
    }

    fn check_composition(&self, scene: &Scene, root: &Renderer, project: &Scene, seen: &mut std::collections::HashSet<String>) -> Result<(), String> {
        if !seen.insert(scene.project_id.clone()) {
            return Ok(());
        }
        for shape in &scene.shapes {
            self.exportable(shape)?;
            if let Some(playback) = &shape.composition {
                let child = project.composition(&playback.source).ok_or_else(|| format!("{}: missing source composition", shape.name))?;
                let renderer = root.compositions.get(&playback.source).ok_or("Missing composition renderer")?;
                renderer.check_composition(child, root, project, seen)?;
            }
        }
        Ok(())
    }

    fn exportable(&self, shape: &Shape) -> Result<(), String> {
        if shape.kind == "video" {
            return match self.videos.get(&shape.id) {
                Some(file) if file.is_file() => Ok(()),
                _ => Err(format!("{}: the video file is missing", shape.name)),
            };
        }
        if let Some(error) = self.missing.get(&shape.id) {
            return Err(format!("{}: {error}", shape.name));
        }
        if !self.draws(shape) {
            return Err(format!("Export of {} layers is coming later.", shape.kind));
        }
        Ok(())
    }

    /// A video layer's file.
    pub fn video(&self, id: &str) -> Option<&Path> {
        self.videos.get(id).map(PathBuf::as_path)
    }

    pub(crate) fn composition_renderer(&self, id: &str) -> Option<&Self> {
        self.compositions.get(id)
    }

    fn nested_frames(&self, scene: &Scene, time: f64, output: (u32, u32), fps: u32) -> Result<Frames, String> {
        let mut videos = self.nested_video.lock().unwrap_or_else(PoisonError::into_inner);
        let mut frames = Frames::new();
        for shape in scene.shapes.iter().filter(|s| s.kind == "video" && s.visible(time, scene.duration)) {
            let path = self.video(&shape.id).ok_or_else(|| format!("{}: missing video file", shape.name))?;
            if !videos.contains_key(&shape.id) {
                videos.insert(shape.id.clone(), NestedVideo { info: crate::media::probe(path)?, reader: None, size: (0, 0) });
            }
            let video = videos.get_mut(&shape.id).ok_or("Missing video reader")?;
            let size = limit_decode_size(output, video.info);
            if video.size != size {
                video.size = size;
                video.reader = None;
            }
            let pixels = crate::media::ShutterReader::at(&mut video.reader, (path, video.info.duration, size), shape, time, fps)?;
            frames.insert(shape.id.clone(), pixels.as_ref().clone());
        }
        Ok(frames)
    }

    pub(crate) fn preview_is_waiting(&self) -> bool {
        self.preview_pending.load(Ordering::Relaxed)
    }

    fn nested_preview(&self, scene: &Scene, time: f64, scale: f32, composition: CompositionRender<'_>) -> Frames {
        if self.videos.is_empty() {
            return Frames::default();
        }
        let mut instances = self.preview_videos.lock().unwrap_or_else(PoisonError::into_inner);
        let epoch = composition.renderer.preview_epoch.load(Ordering::Relaxed);
        instances.retain(|_, (last, _)| epoch.wrapping_sub(*last) <= 2);
        if instances.len() >= 32 && !instances.contains_key(&composition.instance) {
            return Frames::default();
        }
        let (last, videos) = instances.entry(composition.instance).or_default();
        *last = epoch;
        if videos.update(scene, self, time, false, scale) {
            composition.renderer.preview_pending.store(true, Ordering::Relaxed);
        }
        videos.frames.clone()
    }

    fn nested_preview_sample(&self, composition: CompositionRender<'_>, time: f64) -> Option<SampleFrames> {
        self.preview_videos.lock().unwrap_or_else(PoisonError::into_inner).get(&composition.instance)?.1.sample(time)
    }

    pub(crate) fn cached_frame(&self, scene: &Scene, time: f64, size: (u32, u32), transparent: bool) -> Result<Pixmap, String> {
        let fps = motion_blur::DEFAULT_FPS;
        let frames = self.nested_frames(scene, time, size, fps)?;
        self.export_frame(scene, time, size, fps, &frames, |sample| {
            self.nested_frames(scene, sample, size, fps).map(|frames| {
                Some(frames.into_iter().map(|(id, pixels)| (id, Arc::new(pixels))).collect())
            })
        }, transparent)
    }

    /// Whether the editor's preview leaves this layer out (the editor marks where it is).
    pub fn leaves_out(&self, shape: &Shape) -> bool {
        !self.draws(shape)
    }

    pub(crate) fn template_preview(&self, scene: &Scene, time: f64, size: (u32, u32)) -> Result<(Pixmap, bool), String> {
        let fps = motion_blur::DEFAULT_FPS;
        let cache = CompositionFrames::default();
        let composition = self.composition_render(scene, &cache, fps);
        self.preview_pending.store(false, Ordering::Relaxed);
        self.preview_epoch.fetch_add(1, Ordering::Relaxed);
        let frames = self.nested_preview(scene, time, size.0 as f32 / scene.width, composition);
        let shutter = motion_blur::exposure(scene, time, fps);
        let pixels = if shutter.is_empty() {
            self.draw_surface(scene, time, size, false, &frames, None, composition, true)?
        } else {
            average(&shutter, |time_sample| {
                let sampled = self.nested_preview_sample(composition, time_sample);
                self.draw_surface(scene, time, size, false, &frames, Some((time_sample, sampled.as_ref())), composition, true)
            })?
        };
        Ok((pixels, self.preview_is_waiting()))
    }

    fn draws(&self, shape: &Shape) -> bool {
        matches!(
            shape.kind.as_str(),
            "circle"
                | "rect"
                | "triangle"
                | "diamond"
                | "arrow"
                | "text"
                | "path"
                | "adjustment"
                | "composition"
                | "camera"
        ) || (matches!(shape.kind.as_str(), "image" | "video" | "svg")
            && self.images.contains_key(&shape.id))
    }

    /// The frame at `t`, `width` × `height` pixels, for export, with video layers showing `frames`: a visible layer it
    /// can't draw is an error, never left out.
    pub fn frame(
        &self,
        scene: &Scene,
        t: f64,
        (width, height): (u32, u32),
        frames: &Frames,
    ) -> Result<Pixmap, String> {
        self.exposed_frame(
            scene,
            t,
            (width, height),
            motion_blur::DEFAULT_FPS,
            frames,
            |_| Ok(None),
        )
    }

    pub(crate) fn exposed_frame(
        &self,
        scene: &Scene,
        time: f64,
        size: (u32, u32),
        fps: u32,
        frames: &Frames,
        read_sample: impl FnMut(f64) -> Result<Option<SampleFrames>, String>,
    ) -> Result<Pixmap, String> {
        self.export_frame(scene, time, size, fps, frames, read_sample, false)
    }

    pub(crate) fn export_frame(
        &self,
        scene: &Scene,
        time: f64,
        size: (u32, u32),
        fps: u32,
        frames: &Frames,
        mut read_sample: impl FnMut(f64) -> Result<Option<SampleFrames>, String>,
        transparent: bool,
    ) -> Result<Pixmap, String> {
        let pixels = CompositionFrames::default();
        let compositions = self.composition_render(scene, &pixels, fps);
        let times = motion_blur::exposure(scene, time, fps);
        if times.is_empty() {
            return self.draw_surface(scene, time, size, true, frames, None, compositions, transparent);
        }
        average(&times, |sample| {
            let videos = read_sample(sample)?;
            self.draw_surface(
                scene,
                time,
                size,
                true,
                frames,
                Some((sample, videos.as_ref())),
                compositions,
                transparent,
            )
        })
    }

    /// The frame at `t` at `scale` times the scene's size, for the editor: layers it can't draw are left out, and
    /// video layers without one of `frames` show their poster.
    pub fn preview(&self, scene: &Scene, t: f64, scale: f32, frames: &Frames) -> Option<Pixmap> {
        let size = (
            (scene.width * scale).round().max(1.0) as u32,
            (scene.height * scale).round().max(1.0) as u32,
        );
        let times = if scene.motion_blur_preview {
            motion_blur::exposure(scene, t, motion_blur::DEFAULT_FPS)
        } else {
            Vec::new()
        };
        if times.is_empty() {
            self.draw(scene, t, size, false, frames, None).ok()
        } else {
            average(&times, |sample| {
                self.draw(scene, t, size, false, frames, Some((sample, None)))
            })
            .ok()
        }
    }

    /// The editor's view of the scene and its surroundings (Electron's pasteboard): `size` pixels whose top-left is
    /// at `origin` in the scene, `scale` pixels to a scene pixel. Only the frame is filled with the background, and
    /// layers outside it still show, unless `clip` (while playing, Electron clips them to the frame). Layers it can't
    /// draw are left out, and video layers without one of `frames` show their poster.
    pub fn view(
        &self,
        scene: &Scene,
        t: f64,
        view: View,
        clip: bool,
        frames: &Frames,
    ) -> Option<Pixmap> {
        self.invalidate_preview_pixels();
        self.view_with_samples(scene, t, view, clip, frames, |_| None)
    }

    pub(crate) fn view_with_samples(
        &self,
        scene: &Scene,
        t: f64,
        view: View,
        clip: bool,
        frames: &Frames,
        read_sample: impl Fn(f64) -> Option<SampleFrames>,
    ) -> Option<Pixmap> {
        self.preview_pending.store(false, Ordering::Relaxed);
        self.preview_epoch.fetch_add(1, Ordering::Relaxed);
        let canvas = self.draw_view(scene, t, view, clip, frames, None)?;
        if !scene.motion_blur_preview {
            return Some(canvas);
        }
        let times = motion_blur::exposure(scene, t, motion_blur::DEFAULT_FPS);
        if times.is_empty() {
            return Some(canvas);
        }
        average(&times, |sample| {
            let videos = read_sample(sample);
            self.draw_view(
                scene,
                t,
                view,
                clip,
                frames,
                Some((sample, videos.as_ref())),
            )
            .ok_or("Scene is too large to render".to_owned())
        })
        .ok()
    }

    fn draw_view(
        &self,
        scene: &Scene,
        t: f64,
        (origin, scale, size): View,
        clip: bool,
        frames: &Frames,
        sample: Option<ShutterSample<'_>>,
    ) -> Option<Pixmap> {
        let mut canvas = Pixmap::new(size.0.max(1), size.1.max(1))?;
        let (left, top) = (-origin.0 * scale, -origin.1 * scale);
        let frame = Rect::from_xywh(left, top, scene.width * scale, scene.height * scale)?;
        let [r, g, b] = rgb(&scene.bg);
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(r, g, b, 255));
        if !scene.transparent_background {
            canvas.fill_rect(frame, &paint, Transform::identity(), None);
        }
        let view = Transform::from_row(scale, 0.0, 0.0, scale, left, top);
        let mut hits = self
            .text_hits
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if sample.is_none() {
            hits.clear();
        }
        self.layers(
            &mut canvas,
            scene,
            t,
            view,
            scale,
            false,
            frames,
            sample,
            sample.is_none().then_some(&mut hits),
            self.composition_render(scene, &CompositionFrames::default(), motion_blur::DEFAULT_FPS),
        )
        .ok()?;
        if clip {
            // Everything outside the frame goes back to clear.
            let width = canvas.width() as i32;
            let (x0, y0) = (frame.left().round() as i32, frame.top().round() as i32);
            let (x1, y1) = (frame.right().round() as i32, frame.bottom().round() as i32);
            let (x0, x1) = (x0.clamp(0, width) as usize, x1.clamp(0, width) as usize);
            let clear = tiny_skia::PremultipliedColorU8::TRANSPARENT;
            for (y, row) in canvas
                .pixels_mut()
                .chunks_exact_mut(width as usize)
                .enumerate()
            {
                if (y as i32) < y0 || (y as i32) >= y1 {
                    row.fill(clear);
                } else {
                    row[..x0].fill(clear);
                    row[x1.max(x0)..].fill(clear);
                }
            }
        }
        Some(canvas)
    }

    pub(crate) fn text_hit(&self, layer: &str, x: f32, y: f32) -> bool {
        self.text_hits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(layer)
            .is_some_and(|boxes| {
                boxes
                    .iter()
                    .any(|r| r.left() <= x && x <= r.right() && r.top() <= y && y <= r.bottom())
            })
    }

    pub(crate) fn text_bounds(&self, layer: &str) -> Option<Rect> {
        self.text_hits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(layer)?
            .iter()
            .copied()
            .reduce(crate::text_render::union)
    }

    fn draw(
        &self,
        scene: &Scene,
        t: f64,
        (width, height): (u32, u32),
        strict: bool,
        frames: &Frames,
        sample: Option<ShutterSample<'_>>,
    ) -> Result<Pixmap, String> {
        self.draw_surface(scene, t, (width, height), strict, frames, sample,
            self.composition_render(scene, &CompositionFrames::default(), motion_blur::DEFAULT_FPS), false)
    }

    fn composition_render<'a>(&'a self, scene: &'a Scene, pixels: &'a CompositionFrames, fps: u32) -> CompositionRender<'a> {
        CompositionRender { renderer: self, project: scene, pixels, depth: 0, fps, instance: 0 }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_surface(
        &self, scene: &Scene, t: f64, (width, height): (u32, u32), strict: bool, frames: &Frames,
        sample: Option<ShutterSample<'_>>, compositions: CompositionRender<'_>, transparent: bool,
    ) -> Result<Pixmap, String> {
        let mut canvas =
            Pixmap::new(width.max(1), height.max(1)).ok_or("Scene is too large to render")?;
        let [r, g, b] = rgb(&scene.bg);
        if !transparent && !scene.transparent_background {
            canvas.fill(Color::from_rgba8(r, g, b, 255));
        }
        // Like Electron's export, which scales the canvas by width and height separately.
        let (scale_x, scale_y) = (width as f32 / scene.width, height as f32 / scene.height);
        let view = Transform::from_scale(scale_x, scale_y);
        self.layers(
            &mut canvas,
            scene,
            t,
            view,
            scale_x,
            strict,
            frames,
            sample,
            None,
            compositions,
        )?;
        Ok(canvas)
    }

    /// Paints the layers showing at `t` onto `canvas` through `view` (`scale` canvas pixels to a scene pixel).
    #[allow(clippy::too_many_arguments)]
    fn layers(
        &self,
        canvas: &mut Pixmap,
        scene: &Scene,
        t: f64,
        view: Transform,
        scale: f32,
        strict: bool,
        frames: &Frames,
        sample: Option<ShutterSample<'_>>,
        mut text_hits: Option<&mut TextHits>,
        compositions: CompositionRender<'_>,
    ) -> Result<(), String> {
        let resolved = crate::expressions::at_time(scene, t);
        let exposure = sample.map(|(time, _)| crate::expressions::at_time(scene, time));
        let frame = RenderFrame {
            renderer: self,
            scene: &resolved,
            exposure_scene: exposure.as_deref(),
            time: t,
            view,
            scale,
            size: (canvas.width(), canvas.height()),
            strict,
            cache_pixels: text_hits.is_some(),
            videos: frames,
            sampled_videos: None,
            compositions,
        };
        let mut mattes = MatteLinks::new(scene).map(|links| MatteFrame {
            links,
            cache: HashMap::new(),
            bytes: 0,
        });
        let camera = Camera::at(frame.scene, t);
        let exposure_camera = sample
            .zip(frame.exposure_scene)
            .map(|((time, _), exposed)| Camera::at(exposed, time));
        let camera_for = |shape: &Shape| {
            if shape.motion_blur {
                exposure_camera.unwrap_or(camera)
            } else {
                camera
            }
        };
        let depths: Vec<_> = frame
            .scene
            .shapes
            .iter()
            .enumerate()
            .map(|(index, shape)| {
                if !shape.is_3d() || shape.is_camera() {
                    return None;
                }
                let active = frame.sampled(shape, sample);
                let shape = &active.scene.shapes[index];
                Some(camera_for(shape).depth(active.plane(shape).center()))
            })
            .collect();
        let order = depth_order(&depths);
        let mut parallel = HashMap::new();
        // Work in small batches so completed layer tiles cannot accumulate for an entire scene.
        for batch in order.chunks(8) {
            parallel.clear();
            let independent: Vec<_> = batch
                .iter()
                .copied()
                .filter(|&index| {
                    if mattes
                        .as_ref()
                        .is_some_and(|m| m.links.hidden[index] || m.links.sources[index].is_some())
                    {
                        return false;
                    }
                    let shape = &frame.scene.shapes[index];
                    let active = frame.sampled(shape, sample);
                    let shape = &active.scene.shapes[index];
                    active.independent_pixels(shape)
                })
                .collect();
            if independent.len() > 1 {
                let results = independent
                    .par_iter()
                    .map(|&i| {
                        let shape = &frame.scene.shapes[i];
                        let active = frame.sampled(shape, sample);
                        let shape = &active.scene.shapes[i];
                        Ok::<_, String>((i, active.layer_pixels(shape, camera_for(shape), None)?))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                parallel.extend(results);
            }
            for &index in batch {
                let shape = &frame.scene.shapes[index];
                if shape.is_camera()
                    || mattes
                        .as_ref()
                        .is_some_and(|mattes| mattes.links.hidden[index])
                {
                    continue;
                }
                let active = frame.sampled(shape, sample);
                let shape = &active.scene.shapes[index];
                let source = mattes
                    .as_ref()
                    .and_then(|mattes| mattes.links.sources[index]);
                if source.is_none() && active.paint_precomposed_stack(canvas, shape)? {
                    continue;
                }
                if shape.kind == "adjustment" {
                    let matte = match (&mut mattes, source) {
                        (Some(mattes), Some(source)) => mattes.source(&frame, source, sample)?,
                        _ => None,
                    };
                    active.adjust(canvas, shape, matte.as_deref());
                    continue;
                }
                let layer = if let Some(layer) = parallel.remove(&index) {
                    layer
                } else if shape.is_3d() {
                    active.layer_pixels(shape, camera_for(shape), None)?
                } else {
                    let Some((ink, layer_frame)) = active.visible_ink(shape)? else {
                        continue;
                    };
                    if let (Ink::Text(ink), Some(hits)) = (&ink, text_hits.as_deref_mut()) {
                        hits.insert(shape.id.clone(), ink.hit_boxes());
                    }
                    let blur = active.layer_blur(shape, layer_frame);
                    let effects = shape.sampled_effects(active.time);
                    if blur <= 0.0
                        && effects.is_empty()
                        && shape.blend() == Blend::Normal
                        && shape.masks.is_empty()
                        && source.is_none()
                        && ink.group_opacity() >= 1.0
                    {
                        ink.paint(canvas, Transform::identity());
                        continue;
                    }
                    active.layer_pixels(shape, camera_for(shape), Some((ink, layer_frame)))?
                };
                let Some(mut layer) = layer else {
                    continue;
                };
                if let (Some(mattes), Some(source), Some(matte)) =
                    (&mut mattes, source, &shape.track_matte)
                {
                    let pixels = mattes.source(&frame, source, sample)?;
                    Arc::make_mut(&mut layer).apply_matte(pixels.as_deref(), matte.mode);
                }
                if shape.blend() == Blend::Normal && (!shape.masks.is_empty() || source.is_some()) {
                    layer.paint_over(canvas);
                } else {
                    canvas.draw_pixmap(
                        layer.area.x(),
                        layer.area.y(),
                        layer.pixmap.as_ref(),
                        &PixmapPaint {
                            blend_mode: shape.blend().skia(),
                            ..PixmapPaint::default()
                        },
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn still_text(
        &self,
        scene: &Scene,
        shape: &Shape,
        time: f64,
    ) -> Result<Option<LayerPixels>, String> {
        let videos = Frames::default();
        let pixels = CompositionFrames::default();
        let frame = RenderFrame {
            renderer: self,
            scene,
            exposure_scene: None,
            time,
            view: Transform::identity(),
            scale: 1.0,
            size: (scene.width.round() as u32, scene.height.round() as u32),
            strict: false,
            cache_pixels: false,
            videos: &videos,
            sampled_videos: None,
            compositions: self.composition_render(scene, &pixels, motion_blur::DEFAULT_FPS),
        };
        Ok(frame.visible_ink(shape)?.and_then(|(ink, layer)| {
            frame.raster(shape, layer, &ink, frame.layer_blur(shape, layer))
        }))
    }

    /// A layer at `pose` in `color`: shapes and text as outlines, pictures as pixmaps. Each is drawn around its center,
    /// turned by its rotation, then placed (Electron translates, rotates, then draws at the origin).
    fn ink<'a>(
        &'a self,
        shape: &Shape,
        frame: LayerFrame,
        color: &str,
        view: Transform,
        video_pixels: Option<&'a Pixmap>,
        time: f64,
    ) -> Option<Ink<'a>> {
        let pose = frame.pose;
        let place = frame.transform.post_concat(view);
        if let Some(pixmap) = video_pixels.or_else(|| self.images.get(&shape.id)) {
            return Some(picture_ink(PicturePixels::Borrowed(pixmap), frame, view));
        }
        if let Some(vector) = &shape.vector
            && shape.is_vector()
            && (vector.fill.is_some() || vector.stroke.is_some() || vector.trim.is_some()
                || shape.kind == "path" && vector.source_shape.is_none())
        {
            let mut cache = self.geometry.lock().unwrap_or_else(PoisonError::into_inner);
            return VectorInk::new(shape, vector, frame, place, time, color, &mut cache).map(Ink::Vector);
        }
        let outline = match shape.kind.as_str() {
            "text" => text::outline(text::content(shape), pose.w, pose.h)?,
            "path" => shape_outline(shape.vector.as_ref()?.source_shape.as_deref()?, pose.w / 2.0, pose.h / 2.0)?,
            kind => shape_outline(kind, pose.w / 2.0, pose.h / 2.0)?,
        };
        Some(Ink::Fill(outline.transform(place)?, fill(color, pose.o)))
    }
}

fn picture_ink(pixels: PicturePixels<'_>, frame: LayerFrame, view: Transform) -> Ink<'_> {
    let pixmap = pixels.pixmap();
    let pose = frame.pose;
    let paint = PixmapPaint {
        opacity: (pose.o / 100.0).clamp(0.0, 1.0),
        quality: tiny_skia::FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    let transform = Transform::from_scale(pose.w / pixmap.width() as f32, pose.h / pixmap.height() as f32)
        .post_translate(-pose.w / 2.0, -pose.h / 2.0)
        .post_concat(frame.transform.post_concat(view));
    Ink::Picture(pixels, paint, transform)
}

impl<'a> RenderFrame<'a> {
    fn paint_precomposed_stack(&self, canvas: &mut Pixmap, shape: &Shape) -> Result<bool, String> {
        let Some(playback) = &shape.composition else { return Ok(false) };
        if shape.is_3d() || !playback.preserve_stack || playback.offset != 0.0 || playback.speed != 1.0 || playback.looping
            || shape.clip_start() != 0.0 || !shape.visible(self.time, self.scene.duration)
            || shape.blend() != Blend::Normal || !shape.masks.is_empty() || blur(shape) > 0.0
            || shape.effects.iter().any(|effect| effect.enabled)
        {
            return Ok(false);
        }
        let project = self.compositions.project;
        let Some(source) = project.composition(&playback.source) else { return Ok(false) };
        let frame = self.scene.layer_frame(shape, self.time);
        if frame.pose.o != 100.0 || !source.transparent_background
            || (source.width, source.height) != (self.scene.width, self.scene.height)
            || (frame.pose.w, frame.pose.h) != (source.width, source.height)
            || frame.transform != Transform::from_translate(source.width / 2.0, source.height / 2.0)
            || source.motion_blur.is_some() && source.shapes.iter().any(|layer| layer.motion_blur)
            || self.compositions.depth + 1 >= crate::composition::MAX_DEPTH
        {
            return Ok(false);
        }
        let Some(renderer) = self.compositions.renderer.compositions.get(&playback.source) else {
            return Ok(false);
        };
        // The unchanged layer shares its original backdrop, pixel grid and blend order.
        let time = playback.time(self.time, source.duration);
        let nested = self.compositions.enter(&shape.id);
        let frames = if self.strict { renderer.nested_frames(source, time, self.size, nested.fps)? }
            else { renderer.nested_preview(source, time, self.scale, nested) };
        renderer.layers(canvas, source, time, self.view, self.scale, self.strict, &frames, None, None,
            nested)?;
        Ok(true)
    }

    fn visible_ink(&self, shape: &Shape) -> Result<Option<(Ink<'_>, LayerFrame)>, String> {
        if !shape.visible(self.time, self.scene.duration) {
            return Ok(None);
        }
        let frame = self.scene.layer_frame(shape, self.time);
        if frame.pose.o <= 0.0 {
            return Ok(None);
        }
        self.check_ink(shape)?;
        if let Some(playback) = &shape.composition {
            let result = self.composition_pixels(shape, playback, frame);
            return match result {
                Ok(pixels) => Ok(Some((picture_ink(PicturePixels::Composition(pixels), frame, self.view), frame))),
                Err(error) if self.strict => Err(error),
                Err(_) => Ok(None),
            };
        }
        Ok(self.ink_at(shape, frame).map(|ink| (ink, frame)))
    }

    fn check_ink(&self, shape: &Shape) -> Result<(), String> {
        if self.strict {
            self.renderer.exportable(shape)?;
            if shape.kind == "video" && self.video_pixels(shape).is_none() {
                return Err(format!("{}: no video frame", shape.name));
            }
        }
        Ok(())
    }

    fn composition_pixels(&self, shape: &Shape, playback: &crate::composition::Playback, frame: LayerFrame) -> Result<Arc<Pixmap>, String> {
        let composition = self.compositions;
        let missing = || format!("{}: missing source composition", shape.name);
        let source = composition.project.composition(&playback.source).ok_or_else(missing)?;
        if composition.depth >= crate::composition::MAX_DEPTH {
            return Err(format!("{}: compositions are nested too deeply", shape.name));
        }
        let renderer = composition.renderer.compositions.get(&playback.source).ok_or_else(missing)?;
        let time = playback.time(self.time - shape.clip_start(), source.duration);
        let (sx, sy) = crate::transform::axis_scales(frame.transform.post_concat(self.view));
        let size = (
            (frame.pose.w * sx).round().clamp(1.0, 8192.0) as u32,
            (frame.pose.h * sy).round().clamp(1.0, 8192.0) as u32,
        );
        let scale = (16_777_216.0 / (size.0 as f64 * size.1 as f64)).sqrt().min(1.0);
        let size = (((size.0 as f64 * scale) as u32).max(1), ((size.1 as f64 * scale) as u32).max(1));
        let key = (playback.source.clone(), time.to_bits(), size.0, size.1);
        if let Some(pixels) = composition
            .pixels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Ok(Arc::clone(pixels));
        }
        let nested = composition.enter(&shape.id);
        let frames = if self.strict { renderer.nested_frames(source, time, size, composition.fps)? }
            else { renderer.nested_preview(source, time, size.0 as f32 / source.width, nested) };
        let shutter = if self.strict || source.motion_blur_preview {
            motion_blur::exposure(source, time, composition.fps)
        } else {
            Vec::new()
        };
        let pixels = Arc::new(if shutter.is_empty() {
            renderer.draw_surface(source, time, size, self.strict, &frames, None, nested, false)?
        } else {
            average(&shutter, |sample| {
                let sampled = if self.strict {
                    Some(renderer.nested_frames(source, sample, size, composition.fps)?
                        .into_iter().map(|(id, pixels)| (id, Arc::new(pixels))).collect())
                } else { renderer.nested_preview_sample(nested, sample) };
                renderer.draw_surface(source, time, size, self.strict, &frames, Some((sample, sampled.as_ref())), nested, false)
            })?
        });
        let mut cache = composition
            .pixels
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // A draw can contain many differently sized instances of the same composition.
        if cache.len() < 8 && u64::from(size.0) * u64::from(size.1) <= 4_194_304 {
            cache.insert(key, Arc::clone(&pixels));
        }
        Ok(pixels)
    }

    fn ink_at(&self, shape: &Shape, frame: LayerFrame) -> Option<Ink<'_>> {
        if TextInk::used_by(shape) {
            let mut geometry = self
                .renderer
                .geometry
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut text = self
                .renderer
                .text
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let (cache, paths) = &mut *text;
            let ink = TextInk::new(
                self.scene,
                shape,
                frame,
                self.view,
                &shape.color_at(self.time),
                self.time,
                cache,
                &mut geometry,
                paths,
            );
            return ink.map(Ink::Text);
        }
        self.renderer.ink(
            shape,
            frame,
            &shape.color_at(self.time),
            self.view,
            self.video_pixels(shape),
            self.time,
        )
    }

    fn video_pixels(&self, shape: &Shape) -> Option<&Pixmap> {
        self.sampled_videos
            .and_then(|videos| videos.get(&shape.id))
            .map(Arc::as_ref)
            .or_else(|| self.videos.get(&shape.id))
    }

    fn sampled(self, shape: &Shape, sample: Option<ShutterSample<'a>>) -> Self {
        match sample {
            Some((time, videos)) if shape.motion_blur => Self {
                time,
                scene: self.exposure_scene.unwrap_or(self.scene),
                sampled_videos: videos,
                ..self
            },
            _ => self,
        }
    }

    fn plane<'b>(&'b self, shape: &'b Shape) -> Plane<'b> {
        Plane {
            scene: self.scene,
            shape,
            frame: self.scene.layer_frame(shape, self.time),
            time: self.time,
        }
    }

    fn independent_pixels(&self, shape: &Shape) -> bool {
        !shape.is_camera()
            && shape.kind != "adjustment"
            && shape.composition.is_none()
            && shape.visible(self.time, self.scene.duration)
            && (shape.is_3d()
                || (!TextInk::used_by(shape)
                    && (blur(shape) > 0.0
                        || shape.blend() != Blend::Normal
                        || shape.effects.iter().any(|effect| effect.enabled)
                        || !shape.masks.is_empty())))
    }

    fn layer_pixels(
        &self,
        shape: &Shape,
        camera: Camera,
        existing: Option<(Ink<'_>, LayerFrame)>,
    ) -> Result<Option<Arc<LayerPixels>>, String> {
        if shape.is_camera() || !shape.visible(self.time, self.scene.duration) {
            return Ok(None);
        }
        let frame = existing.as_ref().map_or_else(
            || self.scene.layer_frame(shape, self.time),
            |(_, frame)| *frame,
        );
        if frame.pose.o <= 0.0 {
            return Ok(None);
        }
        let key = LayerSample::of(self, shape, frame);
        let plane = shape.is_3d().then_some(Plane {
            scene: self.scene,
            shape,
            frame,
            time: self.time,
        });
        let projection = plane.zip(key.as_ref()).map(|(plane, key)| LayerSample {
            projection: Some((
                camera,
                [
                    plane.world([0.0, 0.0, 0.0]),
                    plane.world([128.0, 0.0, 0.0]),
                    plane.world([0.0, 128.0, 0.0]),
                ],
            )),
            ..key.clone()
        });
        if let Some(cached) = self.cached_pixels(projection.as_ref()) {
            return Ok(Some(cached));
        }
        let local = plane.is_some()
            && shape.composition.is_none()
            && shape.masks.is_empty()
            && shape.effects.iter().all(|effect| !effect.enabled)
            && shape
                .text_layout
                .as_ref()
                .is_none_or(|text| text.path.is_none());
        let (flat, flat_frame) = if local {
            let scale = self.scale * crate::transform::max_scale(frame.transform);
            // Tiny differences in the length of a rotated basis must not replace the flat tile.
            let scale = ((scale * 256.0).round() / 256.0).clamp(1.0 / 256.0, 8.0);
            (
                Self {
                    view: Transform::from_scale(scale, scale),
                    scale,
                    ..*self
                },
                LayerFrame {
                    pose: crate::model::Pose {
                        x: 0.0,
                        y: 0.0,
                        r: 0.0,
                        ..frame.pose
                    },
                    transform: Transform::identity(),
                    space: Transform::identity(),
                },
            )
        } else {
            (*self, frame)
        };
        let key = if local {
            LayerSample::of(&flat, shape, flat_frame)
        } else {
            key
        };
        let source = if let Some(pixels) = self.cached_pixels(key.as_ref()) {
            pixels
        } else {
            let ink = if local {
                self.check_ink(shape)?;
                let Some(ink) = flat.ink_at(shape, flat_frame) else {
                    return Ok(None);
                };
                ink
            } else {
                match existing {
                    Some((ink, _)) => ink,
                    None => {
                        let Some((ink, _)) = self.visible_ink(shape)? else {
                            return Ok(None);
                        };
                        ink
                    }
                }
            };
            let Some(source) = flat.flat_pixels(shape, flat_frame, &ink) else {
                return Ok(None);
            };
            let source = Arc::new(source);
            self.cache_pixels(key, &source);
            source
        };
        let Some(plane) = plane else {
            return Ok(Some(source));
        };
        let dof = camera.blur(camera.depth(plane.center())) * self.scale;
        let Some(inverse) = flat.view.invert() else {
            return Ok(None);
        };
        let to_scene = if local {
            inverse.post_concat(frame.transform)
        } else {
            inverse
        };
        let result =
            crate::projection::warp(&source, plane, camera, to_scene, self.view, self.size, dof)
                .map(Arc::new);
        if let Some(pixels) = &result {
            self.cache_pixels(projection, pixels);
        }
        Ok(result)
    }

    fn cached_pixels(&self, key: Option<&LayerSample>) -> Option<Arc<LayerPixels>> {
        self.renderer
            .layer_pixels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key?)
    }

    fn cache_pixels(&self, key: Option<LayerSample>, pixels: &Arc<LayerPixels>) {
        if let Some(key) = key {
            self.renderer
                .layer_pixels
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key, Arc::clone(pixels), pixels.pixmap.data().len());
        }
    }

    fn flat_pixels(&self, shape: &Shape, frame: LayerFrame, ink: &Ink<'_>) -> Option<LayerPixels> {
        let blur = self.layer_blur(shape, frame);
        let effects = shape.sampled_effects(self.time);
        if !shape.is_3d() {
            return self.raster_effects(shape, frame, ink, blur, effects);
        }
        let bounds = ink.bounds()?;
        let margin = (crate::effect_render::margin(&effects, self.scale, bounds) + blur * 3.0)
            .clamp(0.0, 1536.0)
            .ceil()
            + 3.0;
        let area = IntRect::from_ltrb(
            (bounds.left() - margin).floor() as i32,
            (bounds.top() - margin).floor() as i32,
            (bounds.right() + margin).ceil() as i32,
            (bounds.bottom() + margin).ceil() as i32,
        )
        .filter(|area| fits_filter(*area))?;
        if effects.is_empty() {
            return self.raster_area(shape, frame, ink, blur, area);
        }
        let view = self.view.post_translate(-area.x() as f32, -area.y() as f32);
        let flat = Self {
            view,
            size: (area.width(), area.height()),
            ..*self
        };
        let (ink, frame) = flat.visible_ink(shape).ok().flatten()?;
        let filtered = flat.raster_effects(shape, frame, &ink, blur, effects)?;
        let area = IntRect::from_xywh(
            filtered.area.x().checked_add(area.x())?,
            filtered.area.y().checked_add(area.y())?,
            filtered.area.width(),
            filtered.area.height(),
        )?;
        Some(LayerPixels {
            area,
            pixmap: filtered.pixmap,
        })
    }

    fn raster_effects(
        &self,
        shape: &Shape,
        frame: LayerFrame,
        ink: &Ink<'_>,
        blur: f32,
        effects: Vec<Sample>,
    ) -> Option<LayerPixels> {
        if effects.is_empty() {
            return self.raster(shape, frame, ink, blur);
        }
        let local_frame = if shape
            .text_layout
            .as_ref()
            .is_some_and(|text| text.path.is_some())
        {
            // Text on an external path resolves that path in scene coordinates.
            frame
        } else {
            LayerFrame {
                transform: Transform::identity(),
                ..frame
            }
        };
        for quality in FILTER_QUALITY {
            let local = LocalPixels::new(frame, self.view, self.size, quality)?;
            let local_view = if local_frame.transform.is_identity() {
                Transform::from_scale(local.scale, local.scale)
            } else {
                local_frame
                    .transform
                    .invert()?
                    .post_scale(local.scale, local.scale)
            };
            let local_renderer = Self {
                view: local_view,
                ..*self
            };
            let ink = match ink {
                Ink::Picture(PicturePixels::Composition(pixels), _, _) => picture_ink(
                    PicturePixels::Composition(Arc::clone(pixels)), local_frame, local_view,
                ),
                _ => local_renderer.ink_at(shape, local_frame)?,
            };
            let bounds = ink.bounds()?;
            let local_blur = crate::render::blur(shape) * local.scale;
            let region = local.region(bounds, &effects, local_blur * 3.0)?;
            if !fits_filter(region.area) {
                continue;
            }
            let input =
                local_renderer.raster_area(shape, local_frame, &ink, local_blur, region.area)?;
            let output = self.apply_stack(shape, effects, region, input.pixmap);
            return local.to_view(output, region.area, self.size);
        }
        self.raster(shape, frame, ink, blur)
    }

    fn apply_stack(
        &self,
        shape: &Shape,
        effects: Vec<Sample>,
        region: Region,
        input: Pixmap,
    ) -> Pixmap {
        self.renderer
            .effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .apply(&shape.id, effects, region, input)
    }

    fn adjust(&self, canvas: &mut Pixmap, shape: &Shape, matte: Option<&LayerPixels>) {
        if !shape.visible(self.time, self.scene.duration) {
            return;
        }
        let frame = self.scene.layer_frame(shape, self.time);
        let opacity = (frame.pose.o / 100.0).clamp(0.0, 1.0);
        if opacity <= 0.0 {
            return;
        }
        let effects = shape.sampled_effects(self.time);
        if effects.is_empty() {
            return;
        }
        let Some((local, region)) = FILTER_QUALITY.into_iter().find_map(|quality| {
            let local = LocalPixels::new(frame, self.view, self.size, quality)?;
            let bounds = Rect::from_xywh(
                -frame.pose.w * local.scale / 2.0,
                -frame.pose.h * local.scale / 2.0,
                frame.pose.w * local.scale,
                frame.pose.h * local.scale,
            )?;
            let region = local.region(bounds, &effects, 0.0)?;
            fits_filter(region.area).then_some((local, region))
        }) else {
            return;
        };
        let bounds = region.bounds;
        let Some(input) = local.capture(canvas, region.area) else {
            return;
        };
        let output = self.apply_stack(shape, effects, region, input);
        let Some(mut coverage) = Pixmap::new(region.area.width(), region.area.height()) else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(
            255,
            255,
            255,
            (opacity * 255.0).round() as u8,
        ));
        let offset = Transform::from_translate(-region.area.x() as f32, -region.area.y() as f32);
        coverage.fill_rect(bounds, &paint, offset, None);
        if !shape.masks.is_empty() {
            let mask_view = frame
                .transform
                .invert()
                .map(|inverse| inverse.post_scale(local.scale, local.scale));
            if let Some(mask_view) = mask_view {
                self.renderer
                    .masks
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .apply(
                        &mut coverage,
                        region.area,
                        shape,
                        frame,
                        mask_view,
                        self.time,
                    );
            }
        }
        let Some(mut filtered) = local.to_view(output, region.area, self.size) else {
            return;
        };
        let Some(mut coverage) = local.to_view(coverage, region.area, self.size) else {
            return;
        };
        if let Some(track) = &shape.track_matte {
            coverage.apply_matte(matte, track.mode);
        }
        if shape.blend() != Blend::Normal {
            blend_adjustment(canvas, &mut filtered, shape.blend());
        }
        replace_region(canvas, &filtered, &coverage);
    }

    fn layer_blur(&self, shape: &Shape, frame: LayerFrame) -> f32 {
        blur(shape) * crate::transform::max_scale(frame.space) * self.scale
    }

    fn raster(
        &self,
        shape: &Shape,
        frame: LayerFrame,
        ink: &Ink<'_>,
        blur: f32,
    ) -> Option<LayerPixels> {
        let bounds = ink.bounds()?;
        let (width, height) = (self.size.0 as f32, self.size.1 as f32);
        let area = if blur > 0.0 {
            // Preserve the blur's support outside the canvas to avoid clipping its halo.
            let margin = (blur * 3.0).ceil() + 2.0;
            IntRect::from_ltrb(
                (bounds.left().max(-margin) - margin).floor() as i32,
                (bounds.top().max(-margin) - margin).floor() as i32,
                (bounds.right().min(width + margin) + margin).ceil() as i32,
                (bounds.bottom().min(height + margin) + margin).ceil() as i32,
            )?
        } else {
            IntRect::from_ltrb(
                (bounds.left().floor() - 1.0).max(0.0) as i32,
                (bounds.top().floor() - 1.0).max(0.0) as i32,
                (bounds.right().ceil() + 1.0).min(width) as i32,
                (bounds.bottom().ceil() + 1.0).min(height) as i32,
            )?
        };
        self.raster_area(shape, frame, ink, blur, area)
    }

    fn raster_area(
        &self,
        shape: &Shape,
        frame: LayerFrame,
        ink: &Ink<'_>,
        blur: f32,
        area: IntRect,
    ) -> Option<LayerPixels> {
        let mut pixmap = Pixmap::new(area.width(), area.height())?;
        ink.paint(
            &mut pixmap,
            Transform::from_translate(-area.x() as f32, -area.y() as f32),
        );
        let opacity = ink.group_opacity();
        if opacity < 1.0 {
            for pixel in pixmap.pixels_mut() {
                let alpha = |channel: u8| (f32::from(channel) * opacity).round() as u8;
                if let Some(scaled) = tiny_skia::PremultipliedColorU8::from_rgba(
                    alpha(pixel.red()), alpha(pixel.green()), alpha(pixel.blue()), alpha(pixel.alpha()),
                ) {
                    *pixel = scaled;
                }
            }
        }
        if blur > 0.0 {
            gaussian_blur(&mut pixmap, blur);
        }
        if !shape.masks.is_empty() {
            self.renderer
                .masks
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .apply(&mut pixmap, area, shape, frame, self.view, self.time);
        }
        Some(LayerPixels { area, pixmap })
    }
}

// Extreme zoom or shear can make a layer-space temporary much larger than the viewport.
const FILTER_QUALITY: [f32; 5] = [1.0, 0.5, 0.25, 0.125, 0.0625];

fn fits_filter(area: IntRect) -> bool {
    u64::from(area.width()) * u64::from(area.height()) <= 24 * 1024 * 1024
}

struct LocalPixels {
    scale: f32,
    to_view: Transform,
    from_view: Transform,
    visible: Rect,
}

impl LocalPixels {
    fn new(frame: LayerFrame, view: Transform, (w, h): (u32, u32), quality: f32) -> Option<Self> {
        let place = frame.transform.post_concat(view);
        let scale = (crate::transform::max_scale(place).min(8.0) * quality).max(0.0001);
        let to_view = Transform::from_scale(1.0 / scale, 1.0 / scale).post_concat(place);
        let from_view = to_view.invert()?;
        let visible = transformed(Rect::from_xywh(0.0, 0.0, w as f32, h as f32)?, from_view)?;
        Some(Self {
            scale,
            to_view,
            from_view,
            visible,
        })
    }

    fn region(&self, bounds: Rect, effects: &[Sample], extra: f32) -> Option<Region> {
        let margin = (effect_render::margin(effects, self.scale, bounds) + extra)
            .ceil()
            .clamp(0.0, 1536.0)
            + 2.0;
        // Input beyond the visible viewport can still contribute to its filter halo.
        let area = IntRect::from_ltrb(
            ((bounds.left() - margin).max(self.visible.left() - margin * 2.0)).floor() as i32,
            ((bounds.top() - margin).max(self.visible.top() - margin * 2.0)).floor() as i32,
            ((bounds.right() + margin).min(self.visible.right() + margin * 2.0)).ceil() as i32,
            ((bounds.bottom() + margin).min(self.visible.bottom() + margin * 2.0)).ceil() as i32,
        )?;
        Some(Region {
            area,
            bounds,
            scale: self.scale,
        })
    }

    fn capture(&self, canvas: &Pixmap, area: IntRect) -> Option<Pixmap> {
        let mut pixels = Pixmap::new(area.width(), area.height())?;
        let transform = self
            .from_view
            .post_translate(-area.x() as f32, -area.y() as f32);
        pixels.draw_pixmap(0, 0, canvas.as_ref(), &sampling(), transform, None);
        Some(pixels)
    }

    fn to_view(&self, pixels: Pixmap, area: IntRect, (w, h): (u32, u32)) -> Option<LayerPixels> {
        let placed =
            Transform::from_translate(area.x() as f32, area.y() as f32).post_concat(self.to_view);
        if placed.is_translate() || placed.is_identity() {
            let (x, y) = (placed.tx, placed.ty);
            if x.fract() == 0.0
                && y.fract() == 0.0
                && x >= 0.0
                && y >= 0.0
                && x + pixels.width() as f32 <= w as f32
                && y + pixels.height() as f32 <= h as f32
            {
                let area = IntRect::from_xywh(x as i32, y as i32, pixels.width(), pixels.height())?;
                return Some(LayerPixels {
                    area,
                    pixmap: pixels,
                });
            }
        }
        let bounds = transformed(
            Rect::from_xywh(0.0, 0.0, pixels.width() as f32, pixels.height() as f32)?,
            placed,
        )?;
        let area = IntRect::from_ltrb(
            (bounds.left().floor() - 1.0).max(0.0) as i32,
            (bounds.top().floor() - 1.0).max(0.0) as i32,
            (bounds.right().ceil() + 1.0).min(w as f32) as i32,
            (bounds.bottom().ceil() + 1.0).min(h as f32) as i32,
        )?;
        let mut pixmap = Pixmap::new(area.width(), area.height())?;
        let placed = placed.post_translate(-area.x() as f32, -area.y() as f32);
        pixmap.draw_pixmap(0, 0, pixels.as_ref(), &sampling(), placed, None);
        Some(LayerPixels { area, pixmap })
    }
}

fn sampling() -> PixmapPaint {
    PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..PixmapPaint::default()
    }
}

fn transformed(rect: Rect, transform: Transform) -> Option<Rect> {
    let mut points = [
        Point::from_xy(rect.left(), rect.top()),
        Point::from_xy(rect.right(), rect.top()),
        Point::from_xy(rect.left(), rect.bottom()),
        Point::from_xy(rect.right(), rect.bottom()),
    ];
    transform.map_points(&mut points);
    Rect::from_points(&points)
}

fn blend_adjustment(canvas: &Pixmap, filtered: &mut LayerPixels, blend: Blend) {
    let Some(mut backdrop) = canvas.clone_rect(filtered.area) else {
        return;
    };
    backdrop.draw_pixmap(
        0,
        0,
        filtered.pixmap.as_ref(),
        &PixmapPaint {
            blend_mode: blend.skia(),
            ..PixmapPaint::default()
        },
        Transform::identity(),
        None,
    );
    filtered.pixmap = backdrop;
}

fn replace_region(canvas: &mut Pixmap, filtered: &LayerPixels, coverage: &LayerPixels) {
    use rayon::prelude::*;
    let width = canvas.width() as usize * 4;
    let start = filtered.area.x() as usize * 4;
    let top = filtered.area.y() as usize;
    let stride = filtered.area.width() as usize * 4;
    let blend = |((row, changed), mask): ((&mut [u8], &[u8]), &[u8])| {
        for ((dst, src), a) in row[start..start + stride]
            .chunks_exact_mut(4)
            .zip(changed.chunks_exact(4))
            .zip(mask.chunks_exact(4))
        {
            let a = u16::from(a[3]);
            for (dst, src) in dst.iter_mut().zip(src) {
                *dst = ((u16::from(*dst) * (255 - a) + u16::from(*src) * a + 127) / 255) as u8;
            }
        }
    };
    let rows = &mut canvas.data_mut()[top * width..(top + filtered.area.height() as usize) * width];
    if filtered.pixmap.data().len() < 98_304 {
        rows.chunks_exact_mut(width)
            .zip(filtered.pixmap.data().chunks_exact(stride))
            .zip(coverage.pixmap.data().chunks_exact(stride))
            .for_each(blend);
    } else {
        rows.par_chunks_mut(width)
            .zip(filtered.pixmap.data().par_chunks(stride))
            .zip(coverage.pixmap.data().par_chunks(stride))
            .for_each(blend);
    }
}

fn average(
    times: &[f64],
    mut draw: impl FnMut(f64) -> Result<Pixmap, String>,
) -> Result<Pixmap, String> {
    use rayon::prelude::*;
    let Some((&time, rest)) = times.split_first() else {
        return Err("No shutter samples".into());
    };
    let mut picture = draw(time)?;
    let mut sum: Vec<u16> = picture.data().iter().map(|&byte| u16::from(byte)).collect();
    for &time in rest {
        let sample = draw(time)?;
        sum.par_chunks_mut(32_768)
            .zip(sample.data().par_chunks(32_768))
            .for_each(|(sum, src)| {
                for (sum, &src) in sum.iter_mut().zip(src) {
                    *sum += u16::from(src);
                }
            });
    }
    let count = times.len() as u16;
    picture
        .data_mut()
        .par_chunks_mut(32_768)
        .zip(sum.par_chunks(32_768))
        .for_each(|(dst, sum)| {
            for (dst, &sum) in dst.iter_mut().zip(sum) {
                *dst = ((sum + count / 2) / count) as u8;
            }
        });
    Ok(picture)
}

impl MatteFrame {
    fn source(
        &mut self,
        frame: &RenderFrame<'_>,
        index: usize,
        sample: Option<ShutterSample<'_>>,
    ) -> Result<Option<Arc<LayerPixels>>, String> {
        let mut chain = Vec::new();
        let mut next = Some(index);
        let mut previous = None;
        while let Some(i) = next {
            if let Some(cached) = self.cache.get(&i) {
                previous = cached.clone();
                break;
            }
            chain.push(i);
            next = self.links.sources[i];
        }
        for i in chain.into_iter().rev() {
            let shape = &frame.scene.shapes[i];
            let frame = frame.sampled(shape, sample);
            let shape = &frame.scene.shapes[i];
            let mut layer = frame.layer_pixels(shape, Camera::at(frame.scene, frame.time), None)?;
            if let (Some(layer), Some(matte), Some(_)) =
                (&mut layer, &shape.track_matte, self.links.sources[i])
            {
                Arc::make_mut(layer).apply_matte(previous.as_deref(), matte.mode);
            }
            let bytes = layer.as_ref().map_or(0, |layer| layer.pixmap.data().len());
            previous = layer;
            if self.bytes + bytes <= 32 * 1024 * 1024 {
                self.bytes += bytes;
                self.cache.insert(i, previous.clone());
            }
        }
        Ok(previous)
    }
}

/// The outline of a shape `w` and `h` from its center to its sides, as `paint.js` draws it.
pub(crate) fn shape_outline(kind: &str, w: f32, h: f32) -> Option<Outline> {
    let mut path = PathBuilder::new();
    let mut polygon = |points: &[(f32, f32)]| {
        path.move_to(points[0].0, points[0].1);
        for &(x, y) in &points[1..] {
            path.line_to(x, y);
        }
        path.close();
    };
    match kind {
        "triangle" => polygon(&[(0.0, -h), (w, h), (-w, h)]),
        "diamond" => polygon(&[(0.0, -h), (w, 0.0), (0.0, h), (-w, 0.0)]),
        "arrow" => polygon(&[
            (-w, -h * 0.18),
            (w * 0.12, -h * 0.18),
            (w * 0.12, -h),
            (w, 0.0),
            (w * 0.12, h),
            (w * 0.12, h * 0.18),
            (-w, h * 0.18),
        ]),
        "circle" => path.push_oval(Rect::from_ltrb(-w, -h, w, h)?),
        "rect" => {
            // `roundRect(..., 3)`: quarter circles, as cubic curves, no bigger than the shape allows.
            let r = 3.0f32.min(w).min(h);
            let k = r * 0.552_285;
            path.move_to(-w + r, -h);
            path.line_to(w - r, -h);
            path.cubic_to(w - r + k, -h, w, -h + r - k, w, -h + r);
            path.line_to(w, h - r);
            path.cubic_to(w, h - r + k, w - r + k, h, w - r, h);
            path.line_to(-w + r, h);
            path.cubic_to(-w + r - k, h, -w, h - r + k, -w, h - r);
            path.line_to(-w, -h + r);
            path.cubic_to(-w, -h + r - k, -w + r - k, -h, -w + r, -h);
            path.close();
        }
        _ => return None,
    }
    path.finish()
}

/// A layer's blur in its own pixels.
pub(crate) fn blur(shape: &Shape) -> f32 {
    shape
        .extra
        .get("blur")
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite())
        .map_or(0.0, |v| v.clamp(0.0, MAX_BLUR) as f32)
}

/// Gaussian blur with standard deviation `sigma` (what CSS `blur()` means), as three box blurs each way, the
/// approximation SVG and CSS filters use. Pixels outside are transparent.
pub(crate) fn gaussian_blur(pixmap: &mut Pixmap, sigma: f32) {
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    let data = pixmap.data_mut();
    let mut scratch = vec![0; data.len()];
    for size in box_sizes(sigma) {
        let radius = (size - 1) / 2;
        box_blur::<4>(data, &mut scratch, height, width, 4, width * 4, radius);
        box_blur::<4>(&scratch, data, width, height, width * 4, 4, radius);
    }
}

/// The widths of three box blurs that together approximate a Gaussian.
pub(crate) fn box_sizes(sigma: f32) -> [usize; 3] {
    let variance = 12.0 * sigma * sigma;
    let ideal = (variance / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as i32;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let smaller = ((variance - 3.0 * (lower * lower) as f32 - 12.0 * lower as f32 - 9.0)
        / (-4.0 * lower as f32 - 4.0))
        .round() as i32;
    std::array::from_fn(|i| {
        if (i as i32) < smaller {
            lower as usize
        } else {
            lower as usize + 2
        }
    })
}

/// Averages each of `lines` lines of `length` pixels over `radius` pixels either side. Pixel `j` of line `i` starts
/// at byte `i * stride + j * step`.
pub(crate) fn box_blur<const CHANNELS: usize>(
    from: &[u8],
    to: &mut [u8],
    lines: usize,
    length: usize,
    step: usize,
    stride: usize,
    radius: usize,
) {
    let window = (2 * radius + 1) as u32;
    for line in 0..lines {
        for channel in 0..CHANNELS {
            let at = |j: usize| line * stride + j * step + channel;
            let mut sum: u32 = (0..=radius.min(length - 1))
                .map(|j| from[at(j)] as u32)
                .sum();
            for j in 0..length {
                to[at(j)] = ((sum + window / 2) / window) as u8;
                if j + radius + 1 < length {
                    sum += from[at(j + radius + 1)] as u32;
                }
                if j >= radius {
                    sum -= from[at(j - radius)] as u32;
                }
            }
        }
    }
}

fn picture(shape: &Shape, place: &Place) -> Result<Pixmap, String> {
    let image = assets::image(shape, place)?.ok_or("This layer has no picture")?;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    let size = IntSize::from_wh(image.size[0] as u32, image.size[1] as u32)
        .ok_or("Invalid picture size")?;
    Pixmap::from_vec(bytes, size).ok_or_else(|| "Invalid picture pixels".into())
}

/// An SVG layer drawn at twice its largest size, so it stays sharp shown larger than the scene (the editor on a
/// high-density screen); after that it's drawn like a picture.
fn svg_picture(shape: &Shape, place: &Place) -> Result<Pixmap, String> {
    let source = assets::svg_source(shape, place)?.ok_or("This layer has no SVG")?;
    let tree = svg::parse(&source)?;
    let (w, h) = shape
        .keys
        .iter()
        .fold((1.0f32, 1.0f32), |(w, h), key| (w.max(key.w), h.max(key.h)));
    let fit = (4096.0 / (2.0 * w.max(h))).min(1.0);
    svg::rasterize(
        &tree,
        (2.0 * w * fit).ceil() as u32,
        (2.0 * h * fit).ceil() as u32,
    )
    .ok_or_else(|| "This SVG is too large to draw".into())
}

fn sources(scene: &Scene) -> Vec<(String, Option<u64>)> {
    use std::hash::{Hash, Hasher};
    scene
        .shapes
        .iter()
        .filter(|s| matches!(s.kind.as_str(), "image" | "video" | "svg"))
        .map(|s| {
            let key = assets::image_key(s).map(|key| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                key.hash(&mut hasher);
                hasher.finish()
            });
            (s.id.clone(), key)
        })
        .collect()
}

/// One frame, video layers included (each read from its file).
pub fn frame(scene: &Scene, place: &Place, t: f64) -> Result<Pixmap, String> {
    frame_sized(scene, place, t, scene_size(scene))
}

/// One frame at `size` pixels (the scene scaled to fit), video layers included.
pub fn frame_sized(
    scene: &Scene,
    place: &Place,
    t: f64,
    size: (u32, u32),
) -> Result<Pixmap, String> {
    frame_with_media(scene, place, t, size, true)
}

pub(crate) fn preview_sized(
    scene: &Scene,
    place: &Place,
    t: f64,
    size: (u32, u32),
) -> Result<Pixmap, String> {
    frame_with_media(scene, place, t, size, scene.motion_blur_preview)
}

fn frame_with_media(
    scene: &Scene,
    place: &Place,
    t: f64,
    size: (u32, u32),
    with_motion: bool,
) -> Result<Pixmap, String> {
    let renderer = Renderer::new(scene, place);
    let mut frames = Frames::new();
    let fps = motion_blur::DEFAULT_FPS;
    let shutter = if with_motion {
        motion_blur::exposure(scene, t, fps)
    } else {
        Vec::new()
    };
    let mut movies = Vec::new();
    for shape in scene.shapes.iter().filter(|s| s.kind == "video") {
        let sampled = shape.motion_blur && !shutter.is_empty();
        let times = if sampled {
            shutter.as_slice()
        } else {
            std::slice::from_ref(&t)
        };
        if !times
            .iter()
            .any(|&time| shape.visible(time, scene.duration))
        {
            continue;
        }
        let Some(file) = renderer.video(&shape.id) else {
            continue;
        };
        let info = crate::media::probe(file)?;
        let decode = limit_decode_size(video_size(scene, shape, size, times.iter().copied()), info);
        if sampled {
            movies.push((shape, (file, info.duration, decode), None));
        } else {
            frames.insert(
                shape.id.clone(),
                crate::media::Frames::start(
                    file,
                    shape.media_time(t, info.duration),
                    fps,
                    decode.0,
                    decode.1,
                )?
                .next()?,
            );
        }
    }
    if shutter.is_empty() {
        return renderer.draw(scene, t, size, true, &frames, None);
    }
    renderer.exposed_frame(scene, t, size, fps, &frames, |time| {
        let mut pixels = SampleFrames::new();
        for (shape, source, reader) in &mut movies {
            if shape.visible(time, scene.duration) {
                let frame = crate::media::ShutterReader::at(reader, *source, shape, time, fps)?;
                pixels.insert(shape.id.clone(), frame);
            }
        }
        Ok(Some(pixels))
    })
}

/// The scene's size in whole pixels.
pub fn scene_size(scene: &Scene) -> (u32, u32) {
    (scene.width.round() as u32, scene.height.round() as u32)
}

/// Pick a fixed decode size large enough for every requested frame, including the entire Follows chain.
pub fn video_size(
    scene: &Scene,
    shape: &Shape,
    output_size: (u32, u32),
    times: impl Iterator<Item = f64>,
) -> (u32, u32) {
    let view = Transform::from_scale(
        output_size.0 as f32 / scene.width,
        output_size.1 as f32 / scene.height,
    );
    let (width, height) = times
        .filter(|&t| shape.visible(t, scene.duration))
        .map(|t| {
            let frame = scene.layer_frame(shape, t);
            crate::transform::axis_scales(frame.box_transform().post_concat(view))
        })
        .fold((2.0f32, 2.0f32), |(w, h), (x, y)| (w.max(x), h.max(y)));
    (width.round() as u32, height.round() as u32)
}

pub(crate) fn limit_decode_size(
    (width, height): (u32, u32),
    source: crate::media::Info,
) -> (u32, u32) {
    // FFmpeg can autorotate the video before scaling. Use the long edge for both axes.
    let max = source.width.max(source.height);
    if max == 0 {
        (width, height)
    } else {
        (width.min(max), height.min(max))
    }
}

pub fn save_png(scene: &Scene, place: &Place, t: f64, output: &Path) -> Result<(), String> {
    let pixmap = frame(scene, place, t)?;
    image::save_buffer(
        output,
        pixmap.data(),
        pixmap.width(),
        pixmap.height(),
        image::ColorType::Rgba8,
    )
    .map_err(|e| e.to_string())
}

/// A layer's color at opacity `o` (0-100).
pub(crate) fn fill(color: &str, o: f32) -> Paint<'static> {
    let [r, g, b] = rgb(color);
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, (o * 2.55).round() as u8);
    paint.anti_alias = true;
    paint
}

pub(crate) fn rgb(hex: &str) -> [u8; 3] {
    let channel = |i| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    [channel(1), channel(3), channel(5)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Pose;

    fn place(root: std::path::PathBuf) -> Place {
        Place {
            space: None,
            projects_dir: root.clone(),
            data_dir: root.clone(),
            exports_dir: root,
        }
    }

    #[test]
    #[ignore = "reports 1080p software preview timing on this machine"]
    fn thirty_spatial_layers_scrub_at_full_hd() {
        use crate::keyframes::{Lane, Property};
        use crate::spatial::{Property as SpatialProperty, Spatial};
        use std::time::Instant;

        let mut scene = Scene::empty("spatial-timing".into());
        scene.add_camera("camera".into()).unwrap();
        for i in 0..30 {
            scene.add_shape(format!("card-{i}"), "rect");
            let shape = scene.shapes.last_mut().unwrap();
            let x = 140.0 + 305.0 * (i % 6) as f32;
            let y = 105.0 + 210.0 * (i / 6) as f32;
            (shape.keys[0].x, shape.keys[0].y) = (x, y);
            (shape.keys[0].w, shape.keys[0].h) = (260.0, 170.0);
            shape.spatial = Some(Spatial::default());
            shape.set_spatial(SpatialProperty::RotationY, [20.0, 0.0, 0.0], 0.0);
            if i % 3 == 0 {
                shape.set_expression(
                    Lane::Pose(Property::Rotation),
                    Some("sin(time) * 14".into()),
                );
            }
        }
        let root = std::env::temp_dir();
        let place = Place {
            space: None,
            data_dir: root.clone(),
            projects_dir: root.clone(),
            exports_dir: root,
        };
        let renderer = Renderer::new(&scene, &place);
        let frames = Frames::new();
        for quality in crate::preview_cache::Quality::ALL {
            let view = quality.view(((0.0, 0.0), 1.0, (1920, 1080)));
            let mut durations = Vec::new();
            for frame in 0..=12 {
                let start = Instant::now();
                assert!(
                    renderer
                        .view_with_samples(
                            &scene,
                            f64::from(frame) / 30.0,
                            view,
                            true,
                            &frames,
                            |_| None
                        )
                        .is_some()
                );
                if frame != 0 {
                    durations.push(start.elapsed());
                }
            }
            durations.sort();
            eprintln!(
                "{} median {:.2} ms",
                quality.label(),
                durations[durations.len() / 2].as_secs_f64() * 1000.0
            );
        }
    }

    #[test]
    fn a_circle_covers_its_center_but_not_the_background_corner() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "circle");
        let output = frame(&scene, &place(std::env::temp_dir()), 0.0).unwrap();
        let center = output.pixel(480, 300).unwrap();
        let corner = output.pixel(0, 0).unwrap();
        assert!(
            center.green() < corner.green(),
            "circle should be coral on the pale background"
        );
        assert_eq!(corner.alpha(), 255);
    }

    #[test]
    fn a_parented_rotated_child_has_the_same_pixels_in_preview_and_export() {
        let mut scene = Scene::empty("parent-pixels".into());
        scene.add_shape("parent".into(), "rect");
        scene.add_shape("child".into(), "rect");
        let parent = &mut scene.shapes[0];
        (parent.keys[0].x, parent.keys[0].y, parent.keys[0].o) = (100.0, 100.0, 0.0);
        let child = &mut scene.shapes[1];
        child.color = "#ff0000".into();
        (
            child.keys[0].x,
            child.keys[0].y,
            child.keys[0].w,
            child.keys[0].h,
        ) = (170.0, 100.0, 65.0, 20.0);
        scene.set_parent(1, Some(0), 0.0).unwrap();
        scene.shapes[0].keys[0].r = 90.0;
        let renderer = Renderer::new(&scene, &place(std::env::temp_dir()));
        let frames = Frames::default();
        let export = renderer
            .frame(&scene, 0.0, scene_size(&scene), &frames)
            .unwrap();
        let preview = renderer.preview(&scene, 0.0, 1.0, &frames).unwrap();
        assert_eq!(export.data(), preview.data());
        let red = |x, y| {
            let pixel = export.pixel(x, y).unwrap();
            (pixel.red(), pixel.green(), pixel.blue()) == (255, 0, 0)
        };
        assert!(red(100, 192));
        assert!(!red(123, 170));
        assert!(!red(170, 100));
    }

    #[test]
    fn picture_layers_render_from_the_shared_media_library() {
        let root =
            std::env::temp_dir().join(format!("forma-render-picture-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("blue.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 255]))
            .save(&path)
            .unwrap();
        let place = place(root.clone());
        let (url, _, _) = assets::import_picture(&path, &place).unwrap();
        let mut scene = Scene::empty("test".into());
        scene.add_shape("image".into(), "image");
        scene.shapes[0].extra.insert("assetData".into(), url.into());
        let output = frame(&scene, &place, 0.0).unwrap();
        let center = output.pixel(480, 300).unwrap();
        assert!(center.blue() > 200 && center.red() < 30);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_picture_stops_an_export_but_not_the_preview() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("lost".into(), "image");
        scene.shapes[0].extra.insert(
            "assetData".into(),
            format!("forma-asset://media/{}.png", "0".repeat(64)).into(),
        );
        let renderer = Renderer::new(&scene, &place(std::env::temp_dir()));
        assert!(
            renderer
                .frame(&scene, 0.0, scene_size(&scene), &Frames::new())
                .is_err()
        );
        assert!(renderer.check(&scene).is_err());
        assert!(renderer.leaves_out(&scene.shapes[0]));
        assert_eq!(
            renderer
                .preview(&scene, 0.0, 0.5, &Frames::new())
                .unwrap()
                .width(),
            480
        );
    }

    #[test]
    fn blur_softens_edges_like_a_gaussian() {
        let mut scene = Scene::empty("test".into());
        scene.bg = "#ffffff".into();
        scene.add_shape("a".into(), "rect");
        scene.shapes[0].color = "#000000".into();
        scene.shapes[0].extra.insert("blur".into(), 8.into());
        let output = frame(&scene, &place(std::env::temp_dir()), 0.0).unwrap();
        // A 160 px square at the center, x 400..560. Electron Forma's canvas, measured on 2026-11-01, darkens these
        // pixels of row 300 this much.
        let darkness = |x: u32| 1.0 - output.pixel(x, 300).unwrap().red() as f32 / 255.0;
        for (x, electron) in [
            (370, 0.0),
            (380, 0.008),
            (392, 0.173),
            (396, 0.329),
            (400, 0.525),
            (404, 0.71),
            (410, 0.906),
            (420, 0.996),
            (480, 1.0),
        ] {
            assert!(
                (darkness(x) - electron).abs() < 0.03,
                "x {x}: {} where Electron has {electron}",
                darkness(x)
            );
        }
    }

    #[test]
    fn stored_blur_is_clamped_to_electrons_range() {
        let mut scene = Scene::empty("blur-limit".into());
        scene.add_shape("a".into(), "rect");
        let shape = &mut scene.shapes[0];
        assert_eq!(blur(shape), 0.0);
        for (stored, expected) in [(-1.0, 0.0), (60.0, 60.0), (1e100, 100.0)] {
            shape.extra.insert("blur".into(), stored.into());
            assert_eq!(blur(shape), expected);
        }
    }

    #[test]
    fn a_scaled_child_blurs_like_the_equivalent_canvas_size_and_radius() {
        let mut scene = Scene::empty("parent-blur".into());
        scene.add_shape("parent".into(), "rect");
        scene.add_shape("child".into(), "circle");
        scene.shapes[0].keys[0].o = 0.0;
        scene.shapes[1].extra.insert("blur".into(), 8.into());
        scene.set_parent(1, Some(0), 0.0).unwrap();
        let parent = &mut scene.shapes[0].keys[0];
        parent.w *= 2.0;
        parent.h *= 2.0;
        let place = place(std::env::temp_dir());
        let parented = frame(&scene, &place, 0.0).unwrap();
        scene.shapes[1].parent = None;
        scene.shapes[1].parent_space = None;
        let child = &mut scene.shapes[1];
        child.keys[0].w *= 2.0;
        child.keys[0].h *= 2.0;
        child.extra.insert("blur".into(), 16.into());
        let direct = frame(&scene, &place, 0.0).unwrap();
        assert_eq!(parented.data(), direct.data());
    }

    #[test]
    fn video_decode_uses_the_largest_visible_frame_across_animated_ancestors() {
        let mut scene = Scene::empty("video-decode".into());
        for (id, kind) in [("root", "rect"), ("parent", "rect"), ("child", "video")] {
            scene.add_shape(id.into(), kind);
        }
        let child = &mut scene.shapes[2];
        (child.keys[0].w, child.keys[0].h) = (80.0, 40.0);
        child.in_time = Some(0.5);
        scene.set_parent(1, Some(0), 0.0).unwrap();
        scene.set_parent(2, Some(1), 0.0).unwrap();
        let root = &mut scene.shapes[0];
        let rest = root.pose(0.0);
        root.edit_pose(
            1.0,
            Pose {
                w: rest.w * 2.0,
                ..rest
            },
        );
        root.edit_pose(2.0, rest);
        scene.shapes[1].keys[0].h *= 3.0;
        let size = (1920, 1200);
        let child = &scene.shapes[2];
        assert_eq!(
            video_size(&scene, child, size, [0.5, 1.0, 2.0].into_iter()),
            (320, 240)
        );
        assert_eq!(
            video_size(&scene, child, size, std::iter::once(2.0)),
            (160, 240)
        );
    }

    #[test]
    fn rectangles_have_canvas_round_corners() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "rect");
        let output = frame(&scene, &place(std::env::temp_dir()), 0.0).unwrap();
        let green = |x, y| output.pixel(x, y).unwrap().green() as i32;
        let (background, fill) = (green(0, 0), green(480, 300));
        assert!(
            (green(400, 220) - background).abs() < (background - fill).abs() / 4,
            "the very corner is cut"
        );
        assert_eq!(green(480, 220), fill, "the edge is not");
    }
}
