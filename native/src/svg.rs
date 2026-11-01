//! SVG layers, drawn with resvg. Like Electron Forma's sanitized SVGs (`src/svg/sanitize.js`), they can't reach
//! outside themselves: only pictures embedded as data URLs load, never files or the network, and scripts never run.

use std::sync::{Arc, OnceLock};
use tiny_skia::{Pixmap, Transform};
use resvg::usvg::{ImageHrefResolver, Options, Tree, fontdb};

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
