//! Frame-accurate software rendering of a scene, the way Electron Forma's canvas paints it (`src/render/paint.js`).
//! Exports and the editor's preview both draw through it, so what the editor shows is what gets exported.

use crate::model::{Pose, Scene, Shape};
use crate::{assets, text};
use canopy_ui::Place;
use std::collections::HashMap;
use std::path::Path;
use tiny_skia::{
    Color, FillRule, IntRect, IntSize, Paint, Path as Outline, PathBuilder, Pixmap, PixmapPaint, Point, Rect, Transform,
};

pub struct Renderer {
    images: HashMap<String, Pixmap>,
    /// Picture layers whose picture couldn't be read, and why. Exports stop on them; the editor leaves them out.
    missing: HashMap<String, String>,
    /// Which picture each picture layer was read from, so the editor knows when to read again.
    sources: Vec<(String, Option<String>)>,
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
        for shape in &scene.shapes {
            // Video layers show their poster until video frames are drawn natively.
            if !matches!(shape.kind.as_str(), "image" | "video") {
                continue;
            }
            match picture(shape, place) {
                Ok(pixmap) => {
                    images.insert(shape.id.clone(), pixmap);
                }
                Err(error) => {
                    missing.insert(shape.id.clone(), error);
                }
            }
        }
        Self { images, missing, sources: sources(scene) }
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
        if let Some(error) = self.missing.get(&shape.id) {
            return Err(format!("{}: {error}", shape.name));
        }
        if shape.kind == "video" {
            return Err("Export of video layers is coming later.".into());
        }
        if !self.draws(shape) {
            return Err(format!("Export of {} layers is coming later.", shape.kind));
        }
        Ok(())
    }

    /// Whether the editor's preview leaves this layer out (the editor marks where it is).
    pub fn leaves_out(&self, shape: &Shape) -> bool {
        !self.draws(shape)
    }

    fn draws(&self, shape: &Shape) -> bool {
        matches!(shape.kind.as_str(), "circle" | "rect" | "triangle" | "diamond" | "arrow" | "text")
            || (matches!(shape.kind.as_str(), "image" | "video") && self.images.contains_key(&shape.id))
    }

    /// The frame at `t`, for export: a visible layer it can't draw is an error, never left out.
    pub fn frame(&self, scene: &Scene, t: f64) -> Result<Pixmap, String> {
        self.draw(scene, t, 1.0, true)
    }

    /// The frame at `t` at `scale` times the scene's size, for the editor: layers it can't draw are left out.
    pub fn preview(&self, scene: &Scene, t: f64, scale: f32) -> Option<Pixmap> {
        self.draw(scene, t, scale, false).ok()
    }

    fn draw(&self, scene: &Scene, t: f64, scale: f32, strict: bool) -> Result<Pixmap, String> {
        let (width, height) = ((scene.width * scale).round().max(1.0) as u32, (scene.height * scale).round().max(1.0) as u32);
        let mut canvas = Pixmap::new(width, height).ok_or("Scene is too large to render")?;
        let [r, g, b] = rgb(&scene.bg);
        canvas.fill(Color::from_rgba8(r, g, b, 255));
        let view = Transform::from_scale(scale, scale);
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
            }
            let Some(ink) = self.ink(shape, pose, &shape.color_at(t), view) else { continue };
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
    fn ink(&self, shape: &Shape, pose: Pose, color: &str, view: Transform) -> Option<Ink<'_>> {
        let place = Transform::from_rotate(pose.r).post_translate(pose.x, pose.y).post_concat(view);
        if let Some(pixmap) = self.images.get(&shape.id) {
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

fn sources(scene: &Scene) -> Vec<(String, Option<String>)> {
    scene.shapes.iter().filter(|s| matches!(s.kind.as_str(), "image" | "video")).map(|s| (s.id.clone(), assets::image_key(s))).collect()
}

pub fn frame(scene: &Scene, place: &Place, t: f64) -> Result<Pixmap, String> {
    Renderer::new(scene, place).frame(scene, t)
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
        assert!(renderer.frame(&scene, 0.0).is_err());
        assert!(renderer.check(&scene).is_err());
        assert!(renderer.leaves_out(&scene.shapes[0]));
        assert_eq!(renderer.preview(&scene, 0.0, 0.5).unwrap().width(), 480);
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
