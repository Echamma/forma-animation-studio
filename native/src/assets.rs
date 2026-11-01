//! Forma's content-addressed media library and embedded image data URLs.

use crate::model::Shape;
use base64::Engine;
use canopy_ui::Place;
use egui::ColorImage;
use sha2::{Digest, Sha256};
use std::path::Path;

const PREFIX: &str = "forma-asset://media/";

pub fn image(shape: &Shape, place: &Place) -> Result<Option<ColorImage>, String> {
    let field = if shape.kind == "video" { "posterData" } else if shape.kind == "image" { "assetData" } else { return Ok(None) };
    let value = shape.extra.get(field).and_then(|v| v.as_str());
    let owned = if let Some(value) = value { value.to_owned() } else {
        let Some(token) = shape.extra.get(&format!("{field}Ref")).and_then(|v| v.as_str()) else { return Ok(None) };
        if !valid_uuid(token) { return Err("Invalid Forma media reference".into()); }
        std::fs::read_to_string(place.data_dir.join("assets").join(token)).map_err(|e| e.to_string())?
    };
    let bytes = if let Some(name) = owned.strip_prefix(PREFIX) {
        if !valid_media_name(name) { return Err("Invalid Forma media name".into()); }
        std::fs::read(place.data_dir.join("media").join(name)).map_err(|e| e.to_string())?
    } else if owned.starts_with("data:image/") {
        let (_, encoded) = owned.split_once(',').ok_or("Invalid embedded image")?;
        base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| e.to_string())?
    } else { return Ok(None) };
    let decoded = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.to_rgba8();
    Ok(Some(ColorImage::from_rgba_unmultiplied([decoded.width() as usize, decoded.height() as usize], decoded.as_raw())))
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36 && value.bytes().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == b'-' } else { c.is_ascii_hexdigit() })
}

fn valid_media_name(name: &str) -> bool {
    let Some((hash, extension)) = name.rsplit_once('.') else { return false };
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && matches!(extension, "png" | "jpg" | "webp" | "mp4" | "webm" | "ogv")
}

/// Copies a picture into the Electron-compatible media library and returns its content URL and dimensions.
pub fn import_picture(path: &Path, place: &Place) -> Result<(String, u32, u32), String> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let extension = match extension.as_str() { "png" => "png", "jpg" | "jpeg" => "jpg", "webp" => "webp", _ => return Err("Use PNG, JPEG or WebP pictures".into()) };
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let (width, height) = image::image_dimensions(path).map_err(|e| e.to_string())?;
    let hash = Sha256::digest(&bytes);
    let name = format!("{hash:x}.{extension}");
    let directory = place.data_dir.join("media");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let destination = directory.join(&name);
    if !destination.exists() { std::fs::write(destination, bytes).map_err(|e| e.to_string())?; }
    Ok((format!("{PREFIX}{name}"), width, height))
}

pub fn image_key(shape: &Shape) -> Option<String> {
    let field = if shape.kind == "video" { "posterData" } else if shape.kind == "image" { "assetData" } else { return None };
    shape.extra.get(field).or_else(|| shape.extra.get(&format!("{field}Ref"))).and_then(|v| v.as_str()).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Scene;

    #[test]
    fn imported_picture_uses_electrons_media_url() {
        let root = std::env::temp_dir().join(format!("forma-native-picture-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let picture = root.join("picture.png");
        image::RgbaImage::from_pixel(2, 3, image::Rgba([255, 0, 0, 255])).save(&picture).unwrap();
        let place = Place { space: None, projects_dir: root.clone(), data_dir: root.clone(), exports_dir: root.clone() };
        let (url, width, height) = import_picture(&picture, &place).unwrap();
        assert_eq!((width, height), (2, 3));
        let mut scene = Scene::empty("id".into());
        scene.add_shape("layer".into(), "image");
        scene.shapes[0].extra.insert("assetData".into(), url.into());
        assert_eq!(image(&scene.shapes[0], &place).unwrap().unwrap().size, [2, 3]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
