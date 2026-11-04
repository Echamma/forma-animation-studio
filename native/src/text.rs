//! Text layers, drawn the way Electron Forma's canvas draws them (`src/render/paint.js`): DM Sans SemiBold at the
//! layer's height in pixels, centered on the middle of the em box, and stretched so its advance width is the layer's
//! width. Exports fill the outline with tiny-skia; the editor rasterizes the same outline into a texture.

use std::sync::OnceLock;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

/// The two halves of DM Sans that Forma bundles (split like its web fonts), then Segoe UI for anything else.
fn faces() -> &'static [Face<'static>] {
    static FACES: OnceLock<Vec<Face<'static>>> = OnceLock::new();
    FACES.get_or_init(|| {
        let mut faces: Vec<Face<'static>> = [
            include_bytes!("../fonts/DMSans-SemiBold.ttf").as_slice(),
            include_bytes!("../fonts/DMSans-SemiBold-LatinExt.ttf").as_slice(),
        ]
        .into_iter()
        .filter_map(|data| Face::parse(data, 0).ok())
        .collect();
        let fallback = ["seguisb.ttf", "segoeui.ttf"].iter().find_map(|name| {
            std::fs::read(std::path::Path::new(r"C:\Windows\Fonts").join(name)).ok()
        });
        if let Some(data) = fallback {
            if let Ok(face) = Face::parse(Box::leak(data.into_boxed_slice()), 0) {
                faces.push(face);
            }
        }
        faces
    })
}

/// What a text layer says: Forma shows "Text" for an empty one.
pub fn content(shape: &crate::model::Shape) -> &str {
    match shape.extra.get("text").and_then(|v| v.as_str()) {
        Some(text) if !text.is_empty() => text,
        _ => "Text",
    }
}

/// The outline of `text` `size` pixels high, stretched to `width`, centered on (0, 0) before rotation.
pub fn outline(text: &str, width: f32, size: f32) -> Option<Path> {
    let faces = faces();
    let primary = faces.first()?;
    // Lay out on one line, each character from the first face that has it.
    let mut glyphs = Vec::new();
    let mut pen = 0.0;
    let mut previous: Option<(usize, GlyphId)> = None;
    for c in text.chars().filter(|c| !c.is_control()) {
        let (index, id) = faces
            .iter()
            .enumerate()
            .find_map(|(i, face)| face.glyph_index(c).map(|id| (i, id)))
            .unwrap_or((0, GlyphId(0)));
        let face = &faces[index];
        let scale = size / face.units_per_em() as f32;
        if let Some((before, left)) = previous.filter(|(before, _)| *before == index) {
            pen += kerning(&faces[before], left, id) * scale;
        }
        glyphs.push((index, id, pen, scale));
        pen += face.glyph_hor_advance(id).unwrap_or(0) as f32 * scale;
        previous = Some((index, id));
    }
    let measured = pen.max(1.0);
    let stretch = width / measured;
    // Canvas's "middle" baseline is the middle of the em square, with the ascent and descent scaled to fill one em.
    let (ascent, descent) = (primary.ascender() as f32, -(primary.descender() as f32));
    let baseline = size * (ascent - descent) / (2.0 * (ascent + descent));
    let mut builder = Pen {
        path: PathBuilder::new(),
        origin: 0.0,
        scale: 0.0,
        stretch,
        left: measured / 2.0,
        baseline,
    };
    for (index, id, origin, scale) in glyphs {
        (builder.origin, builder.scale) = (origin, scale);
        faces[index].outline_glyph(id, &mut builder);
    }
    builder.path.finish()
}

fn kerning(face: &Face, left: GlyphId, right: GlyphId) -> f32 {
    face.tables().kern.map_or(0.0, |kern| {
        kern.subtables
            .into_iter()
            .filter(|table| table.horizontal && !table.variable)
            .find_map(|table| table.glyphs_kerning(left, right))
            .unwrap_or(0) as f32
    })
}

/// Turns a glyph's font units (y up, from its pen position) into layer pixels (y down, centered).
struct Pen {
    path: PathBuilder,
    origin: f32,
    scale: f32,
    stretch: f32,
    left: f32,
    baseline: f32,
}

impl Pen {
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        (
            (self.origin + x * self.scale - self.left) * self.stretch,
            self.baseline - y * self.scale,
        )
    }
}

impl OutlineBuilder for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.path.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.path.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let ((x1, y1), (x, y)) = (self.at(x1, y1), self.at(x, y));
        self.path.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let ((x1, y1), (x2, y2), (x, y)) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.path.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.path.close();
    }
}

/// The outline filled white on a transparent pixmap, and where its top-left corner sits from the layer's center.
/// The editor tints it with the layer's color.
pub fn rasterize(text: &str, width: f32, size: f32) -> Option<(Pixmap, [f32; 2])> {
    let path = outline(text, width, size)?;
    let bounds = path.bounds();
    let (left, top) = (bounds.left().floor() - 1.0, bounds.top().floor() - 1.0);
    let mut pixmap = Pixmap::new(
        (bounds.right().ceil() - left + 1.0) as u32,
        (bounds.bottom().ceil() - top + 1.0) as u32,
    )?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::from_translate(-left, -top),
        None,
    );
    Some((pixmap, [left, top]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a layer's ink lands on a 600x300 canvas with the layer at its center, from its center: left, right, top,
    /// bottom, and the ink's area.
    fn ink(text: &str, width: f32, size: f32, rotation: f32) -> [i32; 5] {
        let mut canvas = Pixmap::new(600, 300).unwrap();
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 255);
        canvas.fill_path(
            &outline(text, width, size).unwrap(),
            &paint,
            FillRule::Winding,
            Transform::from_rotate(rotation).post_translate(300.0, 150.0),
            None,
        );
        let [mut left, mut top, mut right, mut bottom] = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let mut area = 0.0;
        for (i, pixel) in canvas.pixels().iter().enumerate() {
            let (x, y) = ((i % 600) as i32, (i / 600) as i32);
            if pixel.alpha() > 127 {
                (left, right, top, bottom) =
                    (left.min(x), right.max(x + 1), top.min(y), bottom.max(y + 1));
            }
            area += pixel.alpha() as f32 / 255.0;
        }
        [
            left - 300,
            right - 300,
            top - 150,
            bottom - 150,
            area.round() as i32,
        ]
    }

    #[test]
    fn text_lands_where_electron_forma_draws_it() {
        // Measured from Electron Forma's canvas drawing (paint.js) with its bundled DM Sans, on 2026-11-01.
        for (text, width, size, rotation, electron) in [
            ("Hello", 400.0, 100.0, 0.0, [-188, 192, -46, 27, 11178]),
            ("Forma ÅÉ gy", 500.0, 80.0, 0.0, [-244, 248, -57, 40, 10080]),
            ("Tilt", 200.0, 90.0, 20.0, [-80, 95, -70, 54, 4638]),
        ] {
            let native = ink(text, width, size, rotation);
            assert_eq!(native[..4], electron[..4], "{text}");
            assert!(
                (native[4] - electron[4]).abs() * 100 < electron[4],
                "{text}: ink {} vs {}",
                native[4],
                electron[4]
            );
        }
    }

    #[test]
    fn accented_and_unbundled_characters_still_draw() {
        assert!(outline("Łódź", 200.0, 60.0).is_some());
        assert!(
            outline("→", 60.0, 60.0).is_some(),
            "falls back past DM Sans"
        );
    }
}
