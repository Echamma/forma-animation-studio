//! Frame-accurate software rendering of a scene, the way Electron Forma's canvas paints it (`src/render/paint.js`).
//! Exports and the editor's preview both draw through it, so what the editor shows is what gets exported.

use crate::model::{Pose, Scene, Shape};
use crate::{assets, svg, text};
use canopy_ui::Place;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_skia::{
    Color, FillRule, IntRect, IntSize, Paint, Path as Outline, PathBuilder, Pixmap, PixmapPaint, Point, Rect, Transform,
};

/// Video layers' frames at one moment, by layer id.
pub type Frames = HashMap<String, Pixmap>;

pub struct Renderer {
    /// Pictures, SVGs (drawn once), and video posters, by layer id.
    images: HashMap<String, Pixmap>,
    /// Video layers' files.
    videos: HashMap<String, PathBuf>,
    /// Picture layers whose picture couldn't be read, and why. Exports stop on them; the editor leaves them out.
    missing: HashMap<String, String>,
    /// What each media layer was read from (hashed), so the editor knows when to read again.
    sources: Vec<(String, Option<u64>)>,
}

/// What a layer puts on the canvas, in canvas pixels.
enum Ink<'a> {
    Fill(Outline, Paint<'static>),
    Picture(&'a Pixmap, PixmapPaint, Transform),
}

impl Ink<'_> {
    fn bounds(&self) -> Option<Rect> {
        match self {
            Ink::Fill(outline, _) => Some(outline.bounds()),
            Ink::Picture(pixmap, _, transform) => {
                let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
                let mut corners = [Point::from_xy(0.0, 0.0), Point::from_xy(w, 0.0), Point::from_xy(w, h), Point::from_xy(0.0, h)];
                transform.map_points(&mut corners);
                Rect::from_points(&corners)
            }
        }
    }

    fn paint(&self, target: &mut Pixmap, offset: Transform) {
        match self {
            Ink::Fill(outline, paint) => target.fill_path(outline, paint, FillRule::Winding, offset, None),
            Ink::Picture(pixmap, paint, transform) => {
                target.draw_pixmap(0, 0, pixmap.as_ref(), paint, transform.post_concat(offset), None)
            }
        }
    }
}

impl Renderer {
    pub fn new(scene: &Scene, place: &Place) -> Self {
        let mut images = HashMap::new();
        let mut missing = HashMap::new();
        let mut videos = HashMap::new();
        for shape in scene.shapes.iter().filter(|s| s.kind == "video") {
            if let Some(file) = assets::stored(shape, "assetData", place).ok().flatten().and_then(|url| assets::library_file(&url, place)) {
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
        Self { images, videos, missing, sources: sources(scene) }
    }

    /// Whether this renderer has read the pictures `scene` shows.
    pub fn matches(&self, scene: &Scene) -> bool {
        self.sources == sources(scene)
    }

    /// Whether every layer can be exported: none is left out of an export.
    pub fn check(&self, scene: &Scene) -> Result<(), String> {
        scene.shapes.iter().try_for_each(|shape| self.exportable(shape))
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

    /// Whether the editor's preview leaves this layer out (the editor marks where it is).
    pub fn leaves_out(&self, shape: &Shape) -> bool {
        !self.draws(shape)
    }

    fn draws(&self, shape: &Shape) -> bool {
        matches!(shape.kind.as_str(), "circle" | "rect" | "triangle" | "diamond" | "arrow" | "text")
            || (matches!(shape.kind.as_str(), "image" | "video" | "svg") && self.images.contains_key(&shape.id))
    }

    /// The frame at `t`, `width` × `height` pixels, for export, with video layers showing `frames`: a visible layer it
    /// can't draw is an error, never left out.
    pub fn frame(&self, scene: &Scene, t: f64, (width, height): (u32, u32), frames: &Frames) -> Result<Pixmap, String> {
        self.draw(scene, t, (width, height), true, frames)
    }

    /// The frame at `t` at `scale` times the scene's size, for the editor: layers it can't draw are left out, and
    /// video layers without one of `frames` show their poster.
    pub fn preview(&self, scene: &Scene, t: f64, scale: f32, frames: &Frames) -> Option<Pixmap> {
        let size = ((scene.width * scale).round().max(1.0) as u32, (scene.height * scale).round().max(1.0) as u32);
        self.draw(scene, t, size, false, frames).ok()
    }

    fn draw(&self, scene: &Scene, t: f64, (width, height): (u32, u32), strict: bool, frames: &Frames) -> Result<Pixmap, String> {
        let mut canvas = Pixmap::new(width.max(1), height.max(1)).ok_or("Scene is too large to render")?;
        let [r, g, b] = rgb(&scene.bg);
        canvas.fill(Color::from_rgba8(r, g, b, 255));
        // Like Electron's export, which scales the canvas by width and height separately.
        let (scale_x, scale_y) = (width as f32 / scene.width, height as f32 / scene.height);
        let (scale, view) = (scale_x, Transform::from_scale(scale_x, scale_y));
        for shape in &scene.shapes {
            if !shape.visible(t, scene.duration) {
                continue;
            }
            let pose = scene.render_pose(shape, t);
            if pose.o <= 0.0 {
                continue;
            }
            if strict {
                self.exportable(shape)?;
                if shape.kind == "video" && !frames.contains_key(&shape.id) {
                    return Err(format!("{}: no video frame", shape.name));
                }
            }
            let Some(ink) = self.ink(shape, pose, &shape.color_at(t), view, frames) else { continue };
            let blur = blur(shape) * scale;
            if blur <= 0.0 {
                ink.paint(&mut canvas, Transform::identity());
                continue;
            }
            // A blurred layer is drawn on its own, blurred, then laid on the canvas, like canvas's `filter: blur()`.
            let margin = (blur * 3.0).ceil() + 2.0;
            let area = ink.bounds().and_then(|b| {
                let (left, top) = (b.left().max(-margin) - margin, b.top().max(-margin) - margin);
                let (right, bottom) = (b.right().min(width as f32 + margin) + margin, b.bottom().min(height as f32 + margin) + margin);
                IntRect::from_ltrb(left.floor() as i32, top.floor() as i32, right.ceil() as i32, bottom.ceil() as i32)
            });
            let Some(area) = area else { continue };
            let Some(mut layer) = Pixmap::new(area.width(), area.height()) else { continue };
            ink.paint(&mut layer, Transform::from_translate(-area.x() as f32, -area.y() as f32));
            gaussian_blur(&mut layer, blur);
            canvas.draw_pixmap(area.x(), area.y(), layer.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
        Ok(canvas)
    }

    /// A layer at `pose` in `color`: shapes and text as outlines, pictures as pixmaps. Each is drawn around its center,
    /// turned by its rotation, then placed (Electron translates, rotates, then draws at the origin).
    fn ink<'a>(&'a self, shape: &Shape, pose: Pose, color: &str, view: Transform, frames: &'a Frames) -> Option<Ink<'a>> {
        let place = Transform::from_rotate(pose.r).post_translate(pose.x, pose.y).post_concat(view);
        if let Some(pixmap) = frames.get(&shape.id).or_else(|| self.images.get(&shape.id)) {
            let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
            let mut paint = PixmapPaint::default();
            paint.opacity = (pose.o / 100.0).clamp(0.0, 1.0);
            paint.quality = tiny_skia::FilterQuality::Bilinear;
            let transform = Transform::from_scale(pose.w / w, pose.h / h).post_translate(-pose.w / 2.0, -pose.h / 2.0).post_concat(place);
            return Some(Ink::Picture(pixmap, paint, transform));
        }
        let outline = match shape.kind.as_str() {
            "text" => text::outline(text::content(shape), pose.w, pose.h)?,
            kind => shape_outline(kind, pose.w / 2.0, pose.h / 2.0)?,
        };
        Some(Ink::Fill(outline.transform(place)?, fill(color, pose.o)))
    }
}

/// The outline of a shape `w` and `h` from its center to its sides, as `paint.js` draws it.
fn shape_outline(kind: &str, w: f32, h: f32) -> Option<Outline> {
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
        "arrow" => polygon(&[(-w, -h * 0.18), (w * 0.12, -h * 0.18), (w * 0.12, -h), (w, 0.0), (w * 0.12, h), (w * 0.12, h * 0.18), (-w, h * 0.18)]),
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

/// A layer's blur in pixels, as Electron Forma reads it.
fn blur(shape: &Shape) -> f32 {
    shape.extra.get("blur").and_then(|v| v.as_f64()).filter(|v| v.is_finite()).map_or(0.0, |v| v.clamp(0.0, 100.0) as f32)
}

/// Gaussian blur with standard deviation `sigma` (what CSS `blur()` means), as three box blurs each way, the
/// approximation SVG and CSS filters use. Pixels outside are transparent.
fn gaussian_blur(pixmap: &mut Pixmap, sigma: f32) {
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    let data = pixmap.data_mut();
    let mut scratch = vec![0; data.len()];
    for size in box_sizes(sigma) {
        let radius = (size - 1) / 2;
        box_blur(data, &mut scratch, height, width, 4, width * 4, radius);
        box_blur(&scratch, data, width, height, width * 4, 4, radius);
    }
}

/// The widths of three box blurs that together approximate a Gaussian.
fn box_sizes(sigma: f32) -> [usize; 3] {
    let variance = 12.0 * sigma * sigma;
    let ideal = (variance / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as i32;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let smaller = ((variance - 3.0 * (lower * lower) as f32 - 12.0 * lower as f32 - 9.0) / (-4.0 * lower as f32 - 4.0)).round() as i32;
    std::array::from_fn(|i| if (i as i32) < smaller { lower as usize } else { lower as usize + 2 })
}

/// Averages each of `lines` lines of `length` pixels over `radius` pixels either side. Pixel `j` of line `i` starts
/// at byte `i * stride + j * step`.
fn box_blur(from: &[u8], to: &mut [u8], lines: usize, length: usize, step: usize, stride: usize, radius: usize) {
    let window = (2 * radius + 1) as u32;
    for line in 0..lines {
        for channel in 0..4 {
            let at = |j: usize| line * stride + j * step + channel;
            let mut sum: u32 = (0..=radius.min(length - 1)).map(|j| from[at(j)] as u32).sum();
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
    let size = IntSize::from_wh(image.size[0] as u32, image.size[1] as u32).ok_or("Invalid picture size")?;
    Pixmap::from_vec(bytes, size).ok_or_else(|| "Invalid picture pixels".into())
}

/// An SVG layer drawn at twice its largest size, so it stays sharp shown larger than the scene (the editor on a
/// high-density screen); after that it's drawn like a picture.
fn svg_picture(shape: &Shape, place: &Place) -> Result<Pixmap, String> {
    let source = assets::svg_source(shape, place)?.ok_or("This layer has no SVG")?;
    let tree = svg::parse(&source)?;
    let (w, h) = shape.keys.iter().fold((1.0f32, 1.0f32), |(w, h), key| (w.max(key.w), h.max(key.h)));
    let fit = (4096.0 / (2.0 * w.max(h))).min(1.0);
    svg::rasterize(&tree, (2.0 * w * fit).ceil() as u32, (2.0 * h * fit).ceil() as u32).ok_or_else(|| "This SVG is too large to draw".into())
}

fn sources(scene: &Scene) -> Vec<(String, Option<u64>)> {
    use std::hash::{Hash, Hasher};
    scene.shapes.iter().filter(|s| matches!(s.kind.as_str(), "image" | "video" | "svg")).map(|s| {
        let key = assets::image_key(s).map(|key| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
            hasher.finish()
        });
        (s.id.clone(), key)
    }).collect()
}

/// One frame, video layers included (each read from its file).
pub fn frame(scene: &Scene, place: &Place, t: f64) -> Result<Pixmap, String> {
    frame_sized(scene, place, t, scene_size(scene))
}

/// One frame at `size` pixels (the scene scaled to fit), video layers included.
pub fn frame_sized(scene: &Scene, place: &Place, t: f64, size: (u32, u32)) -> Result<Pixmap, String> {
    let renderer = Renderer::new(scene, place);
    let mut frames = Frames::new();
    for shape in scene.shapes.iter().filter(|s| s.kind == "video" && s.visible(t, scene.duration)) {
        let Some(file) = renderer.video(&shape.id) else { continue };
        let duration = crate::media::probe(file)?.duration;
        let (width, height) = video_size(shape, 1.0);
        frames.insert(shape.id.clone(), crate::media::Frames::start(file, shape.media_time(t, duration), 30, width, height)?.next()?);
    }
    renderer.frame(scene, t, size, &frames)
}

/// The scene's size in whole pixels.
pub fn scene_size(scene: &Scene) -> (u32, u32) {
    (scene.width.round() as u32, scene.height.round() as u32)
}

/// The size to decode a video layer's frames at for an export `scale` times the scene's size: its largest size.
pub fn video_size(shape: &Shape, scale: f32) -> (u32, u32) {
    let (w, h) = shape.keys.iter().fold((2.0f32, 2.0f32), |(w, h), key| (w.max(key.w), h.max(key.h)));
    ((w * scale).round() as u32, (h * scale).round() as u32)
}

pub fn save_png(scene: &Scene, place: &Place, t: f64, output: &Path) -> Result<(), String> {
    let pixmap = frame(scene, place, t)?;
    image::save_buffer(output, pixmap.data(), pixmap.width(), pixmap.height(), image::ColorType::Rgba8).map_err(|e| e.to_string())
}

/// A layer's color at opacity `o` (0-100).
fn fill(color: &str, o: f32) -> Paint<'static> {
    let [r, g, b] = rgb(color);
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, (o * 2.55).round() as u8);
    paint.anti_alias = true;
    paint
}

fn rgb(hex: &str) -> [u8; 3] {
    let channel = |i| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    [channel(1), channel(3), channel(5)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(root: std::path::PathBuf) -> Place {
        Place { space: None, projects_dir: root.clone(), data_dir: root.clone(), exports_dir: root }
    }

    #[test]
    fn a_circle_covers_its_center_but_not_the_background_corner() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "circle");
        let output = frame(&scene, &place(std::env::temp_dir()), 0.0).unwrap();
        let center = output.pixel(480, 300).unwrap();
        let corner = output.pixel(0, 0).unwrap();
        assert!(center.green() < corner.green(), "circle should be coral on the pale background");
        assert_eq!(corner.alpha(), 255);
    }

    #[test]
    fn picture_layers_render_from_the_shared_media_library() {
        let root = std::env::temp_dir().join(format!("forma-render-picture-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("blue.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 255])).save(&path).unwrap();
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
        scene.shapes[0].extra.insert("assetData".into(), format!("forma-asset://media/{}.png", "0".repeat(64)).into());
        let renderer = Renderer::new(&scene, &place(std::env::temp_dir()));
        assert!(renderer.frame(&scene, 0.0, scene_size(&scene), &Frames::new()).is_err());
        assert!(renderer.check(&scene).is_err());
        assert!(renderer.leaves_out(&scene.shapes[0]));
        assert_eq!(renderer.preview(&scene, 0.0, 0.5, &Frames::new()).unwrap().width(), 480);
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
        for (x, electron) in [(370, 0.0), (380, 0.008), (392, 0.173), (396, 0.329), (400, 0.525), (404, 0.71), (410, 0.906), (420, 0.996), (480, 1.0)] {
            assert!((darkness(x) - electron).abs() < 0.03, "x {x}: {} where Electron has {electron}", darkness(x));
        }
    }

    #[test]
    fn rectangles_have_canvas_round_corners() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "rect");
        let output = frame(&scene, &place(std::env::temp_dir()), 0.0).unwrap();
        let green = |x, y| output.pixel(x, y).unwrap().green() as i32;
        let (background, fill) = (green(0, 0), green(480, 300));
        assert!((green(400, 220) - background).abs() < (background - fill).abs() / 4, "the very corner is cut");
        assert_eq!(green(480, 220), fill, "the edge is not");
    }
}
