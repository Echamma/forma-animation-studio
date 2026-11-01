//! Frame-accurate software rendering for Forma's native picture export.

use crate::{assets, model::Scene};
use canopy_ui::Place;
use std::path::Path;
use tiny_skia::{Color, FillRule, IntSize, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};

pub fn frame(scene: &Scene, place: &Place, t: f64) -> Result<Pixmap, String> {
    let (width, height) = (scene.width.round() as u32, scene.height.round() as u32);
    let mut canvas = Pixmap::new(width, height).ok_or("Scene is too large to render")?;
    let [r, g, b] = rgb(&scene.bg);
    canvas.fill(Color::from_rgba8(r, g, b, 255));
    for shape in &scene.shapes {
        if !shape.visible(t, scene.duration) { continue; }
        if shape.extra.get("blur").and_then(|v| v.as_f64()).unwrap_or(0.0) > 0.0 {
            return Err("Picture export of blurred layers is coming later.".into());
        }
        let pose = scene.render_pose(shape, t);
        if pose.o <= 0.0 { continue; }
        if shape.kind == "image" {
            let image = assets::image(shape, place)?.ok_or("This image layer has no picture to export")?;
            let source_bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            let source = Pixmap::from_vec(source_bytes, IntSize::from_wh(image.size[0] as u32, image.size[1] as u32).ok_or("Invalid picture size")?)
                .ok_or("Invalid picture pixels")?;
            let (sx, sy) = (pose.w / source.width() as f32, pose.h / source.height() as f32);
            let angle = pose.r.to_radians();
            let (cos, sin) = (angle.cos(), angle.sin());
            let (a, b, c, d) = (sx * cos, sx * sin, -sy * sin, sy * cos);
            let (hw, hh) = (source.width() as f32 / 2.0, source.height() as f32 / 2.0);
            let transform = Transform::from_row(a, b, c, d, pose.x - a * hw - c * hh, pose.y - b * hw - d * hh);
            let mut paint = PixmapPaint::default();
            paint.opacity = (pose.o / 100.0).clamp(0.0, 1.0);
            paint.quality = tiny_skia::FilterQuality::Bilinear;
            canvas.draw_pixmap(0, 0, source.as_ref(), &paint, transform, None);
            continue;
        }
        let mut path = PathBuilder::new();
        let (w, h) = (pose.w / 2.0, pose.h / 2.0);
        match shape.kind.as_str() {
            "circle" => path.push_oval(Rect::from_xywh(pose.x - w, pose.y - h, pose.w, pose.h).ok_or("Invalid circle")?),
            "rect" => path.push_rect(Rect::from_xywh(pose.x - w, pose.y - h, pose.w, pose.h).ok_or("Invalid rectangle")?),
            "triangle" => { path.move_to(pose.x, pose.y - h); path.line_to(pose.x + w, pose.y + h); path.line_to(pose.x - w, pose.y + h); path.close(); }
            "diamond" => { path.move_to(pose.x, pose.y - h); path.line_to(pose.x + w, pose.y); path.line_to(pose.x, pose.y + h); path.line_to(pose.x - w, pose.y); path.close(); }
            "arrow" => {
                for (i, (x, y)) in [(-w, -h * 0.18), (w * 0.12, -h * 0.18), (w * 0.12, -h), (w, 0.0), (w * 0.12, h), (w * 0.12, h * 0.18), (-w, h * 0.18)].into_iter().enumerate() {
                    if i == 0 { path.move_to(pose.x + x, pose.y + y); } else { path.line_to(pose.x + x, pose.y + y); }
                }
                path.close();
            }
            _ => return Err(format!("Picture export of {} layers is coming later.", shape.kind)),
        }
        let [r, g, b] = rgb(&shape.color_at(t));
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, (pose.o * 2.55).round() as u8);
        paint.anti_alias = true;
        canvas.fill_path(&path.finish().ok_or("Invalid shape path")?, &paint, FillRule::Winding,
            Transform::from_rotate_at(pose.r, pose.x, pose.y), None);
    }
    Ok(canvas)
}

pub fn save_png(scene: &Scene, place: &Place, t: f64, output: &Path) -> Result<(), String> {
    let pixmap = frame(scene, place, t)?;
    image::save_buffer(output, pixmap.data(), pixmap.width(), pixmap.height(), image::ColorType::Rgba8).map_err(|e| e.to_string())
}

fn rgb(hex: &str) -> [u8; 3] {
    let channel = |i| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    [channel(1), channel(3), channel(5)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circle_covers_its_center_but_not_the_background_corner() {
        let root = std::env::temp_dir();
        let place = Place { space: None, projects_dir: root.clone(), data_dir: root.clone(), exports_dir: root };
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "circle");
        let output = frame(&scene, &place, 0.0).unwrap();
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
        let place = Place { space: None, projects_dir: root.clone(), data_dir: root.clone(), exports_dir: root.clone() };
        let (url, _, _) = assets::import_picture(&path, &place).unwrap();
        let mut scene = Scene::empty("test".into());
        scene.add_shape("image".into(), "image");
        scene.shapes[0].extra.insert("assetData".into(), url.into());
        let output = frame(&scene, &place, 0.0).unwrap();
        let center = output.pixel(480, 300).unwrap();
        assert!(center.blue() > 200 && center.red() < 30);
        std::fs::remove_dir_all(root).unwrap();
    }
}
