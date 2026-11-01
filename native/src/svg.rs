//! SVG layers, drawn with resvg. Like Electron Forma's sanitized SVGs (`src/svg/sanitize.js`), they can't reach
//! outside themselves: only pictures embedded as data URLs load, never files or the network, and scripts never run.

use std::sync::{Arc, OnceLock};
use tiny_skia::{Pixmap, Transform};
use resvg::usvg::{ImageHrefResolver, Options, Tree, fontdb, roxmltree};

pub fn parse(source: &str) -> Result<Tree, String> {
    let mut options = Options::default();
    options.image_href_resolver = ImageHrefResolver {
        resolve_data: ImageHrefResolver::default_data_resolver(),
        resolve_string: Box::new(|_, _| None),
    };
    // Text in an SVG is drawn with the computer's fonts, as the browser does. Reading them takes a moment, so only
    // SVGs with text pay for it.
    if source.contains("<text") {
        options.fontdb = system_fonts();
    }
    Tree::from_str(source, &options).map_err(|e| format!("Invalid SVG markup ({e})"))
}

fn system_fonts() -> Arc<fontdb::Database> {
    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        })
        .clone()
}

/// Paints an SVG's artwork one color, as Electron's `recolorSvg` does: the root's fill, and every fill and stroke that
/// isn't `none` or a gradient, in attributes and in `style`. Only those values change; the rest of the markup stays
/// as it was written.
pub fn recolor(source: &str, color: &str) -> Result<String, String> {
    let invalid = || "The SVG markup is not valid yet".to_owned();
    let doc = roxmltree::Document::parse(source).map_err(|_| invalid())?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(invalid());
    }
    let paintable = |value: &str| !value.trim().is_empty() && value.trim() != "none" && !value.trim().starts_with("url(");
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    match root.attributes().find(|a| a.name() == "fill" && a.namespace().is_none()) {
        Some(fill) => edits.push((fill.range_value(), color.to_owned())),
        None => {
            // Right after the tag's name: `<svg` becomes `<svg fill="…"`.
            let start = root.range().start + 1;
            let name = source[start..].find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(0);
            edits.push((start + name..start + name, format!(r#" fill="{color}""#)));
        }
    }
    for node in doc.descendants().filter(|n| n.is_element()) {
        for attribute in node.attributes().filter(|a| a.namespace().is_none()) {
            match attribute.name() {
                "fill" if node == root => {}
                "fill" | "stroke" if paintable(attribute.value()) => edits.push((attribute.range_value(), color.to_owned())),
                "style" => {
                    let style: Vec<String> = attribute.value().split(';').map(|declaration| match declaration.split_once(':') {
                        Some((name, value)) if ["fill", "stroke"].contains(&name.trim().to_ascii_lowercase().as_str()) && paintable(value) => format!("{}:{color}", name.trim()),
                        _ => declaration.to_owned(),
                    }).collect();
                    edits.push((attribute.range_value(), style.join(";")));
                }
                _ => {}
            }
        }
    }
    let mut out = source.to_owned();
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, text) in edits {
        out.replace_range(range, &text);
    }
    Ok(out)
}

/// The SVG's own size, from its width and height or its viewBox.
pub fn size(tree: &Tree) -> (f32, f32) {
    (tree.size().width(), tree.size().height())
}

/// The SVG drawn `width` × `height` pixels, stretched to fill it the way canvas `drawImage` stretches an SVG image.
pub fn rasterize(tree: &Tree, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    let (w, h) = size(tree);
    resvg::render(tree, Transform::from_scale(width as f32 / w, height as f32 / h), &mut pixmap.as_mut());
    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svgs_draw_stretched_to_the_layer() {
        let tree = parse(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect x="0" y="0" width="5" height="10" fill="#ff0000"/></svg>"##).unwrap();
        assert_eq!(size(&tree), (10.0, 10.0));
        let pixmap = rasterize(&tree, 40, 20).unwrap();
        assert_eq!(pixmap.pixel(10, 10).unwrap().red(), 255, "left half is red");
        assert_eq!(pixmap.pixel(30, 10).unwrap().alpha(), 0, "right half is empty");
    }

    #[test]
    fn recoloring_paints_fills_and_strokes_but_not_none_or_gradients() {
        let source = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4"><rect fill="#ff0000" stroke="none" width="2" height="2"/><circle style="fill: blue; stroke-width: 2" r="1"/><path fill="url(#g)" d="M0 0"/></svg>"##;
        let out = recolor(source, "#123456").unwrap();
        assert!(out.starts_with(r##"<svg fill="#123456" xmlns"##), "{out}");
        assert!(out.contains(r##"<rect fill="#123456" stroke="none""##), "{out}");
        assert!(out.contains(r##"style="fill:#123456; stroke-width: 2""##), "{out}");
        assert!(out.contains(r##"fill="url(#g)""##), "{out}");
        assert!(parse(&out).is_ok());
        assert!(recolor("<svg", "#000000").is_err());
    }

    #[test]
    fn svgs_never_load_files() {
        let picture = std::env::temp_dir().join(format!("forma-svg-outside-{}.png", std::process::id()));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 255, 0, 255])).save(&picture).unwrap();
        let source = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="4" height="4"><image width="4" height="4" xlink:href="{}"/></svg>"#, picture.display());
        let pixmap = rasterize(&parse(&source).unwrap(), 4, 4).unwrap();
        assert_eq!(pixmap.pixel(2, 2).unwrap().alpha(), 0);
        std::fs::remove_file(picture).unwrap();
        assert!(parse("<html>not an svg</html>").is_err());
    }
}
