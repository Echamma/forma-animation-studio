//! `.forma` project files, as Electron Forma packs them (`src/export/project-file.js`): a zip of `project.json` and
//! every picture and video it uses as `media/<sha-256>.<extension>`, stored uncompressed (they're compressed already).
//! SVG markup travels inside `project.json`. Files packed here open in Electron Forma, and the other way round.

use crate::{assets, model::Scene};
use canopy_ui::Place;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MEDIA_FIELDS: [&str; 2] = ["assetData", "posterData"];

pub fn pack(scene: &Scene, place: &Place, output: &Path) -> Result<(), String> {
    let result = write(scene, place, output);
    if result.is_err() {
        let _ = std::fs::remove_file(output);
    }
    result
}

fn write(scene: &Scene, place: &Place, output: &Path) -> Result<(), String> {
    let mut project = serde_json::to_value(scene).map_err(|e| e.to_string())?;
    let mut media = Vec::new();
    let mut names: HashMap<String, String> = HashMap::new();
    let mut packed: HashSet<String> = HashSet::new();
    let shapes = project["shapes"].as_array_mut().ok_or("Invalid project")?;
    for (shape, copy) in scene.shapes.iter().zip(shapes) {
        let copy = copy.as_object_mut().ok_or("Invalid project")?;
        for field in MEDIA_FIELDS {
            let Some(value) = assets::stored(shape, field, place)? else { continue };
            let name = match names.get(&value) {
                Some(name) => name.clone(),
                None => {
                    let (bytes, extension) = assets::media_bytes(&value, place).map_err(|e| format!("{}: {e}", shape.name))?;
                    let name = format!("media/{:x}.{extension}", Sha256::digest(&bytes));
                    if packed.insert(name.clone()) {
                        media.push((name.clone(), bytes));
                    }
                    names.insert(value, name.clone());
                    name
                }
            };
            copy.insert(field.into(), name.into());
            copy.remove(&format!("{field}Ref"));
        }
        if let Some(source) = assets::svg_source(shape, place)? {
            copy.insert("svgSource".into(), source.into());
            copy.remove("svgSourceRef");
        }
    }
    let error = |e: zip::result::ZipError| e.to_string();
    let mut zip = ZipWriter::new(File::create(output).map_err(|e| e.to_string())?);
    for (name, bytes) in &media {
        zip.start_file(name.as_str(), SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).map_err(error)?;
        zip.write_all(bytes).map_err(|e| e.to_string())?;
    }
    zip.start_file("project.json", SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)).map_err(error)?;
    let json = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
    zip.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    zip.finish().map_err(error)?;
    Ok(())
}

/// Reads a `.forma` file, putting its media in this place's library.
pub fn unpack(path: &Path, place: &Place) -> Result<Scene, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|_| "This isn't a .forma file".to_owned())?;
    let json = entry(&mut zip, "project.json").map_err(|_| "This .forma file has no project.json".to_owned())?;
    let mut project: Value = serde_json::from_slice(&json).map_err(|e| format!("Invalid Forma project: {e}"))?;
    let mut stored: HashMap<String, String> = HashMap::new();
    for shape in project["shapes"].as_array_mut().into_iter().flatten() {
        for field in MEDIA_FIELDS {
            let Some(name) = shape.get(field).and_then(Value::as_str).filter(|n| n.starts_with("media/")).map(str::to_owned) else { continue };
            let url = match stored.get(&name) {
                Some(url) => url.clone(),
                None => {
                    let bytes = entry(&mut zip, &name).map_err(|_| format!("This .forma file is missing {name}"))?;
                    let url = assets::store(&bytes, name.rsplit_once('.').map_or("", |(_, ext)| ext), place)?;
                    stored.insert(name, url.clone());
                    url
                }
            };
            shape[field] = url.into();
        }
    }
    Scene::read(&serde_json::to_vec(&project).map_err(|e| e.to_string())?)
}

fn entry(zip: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>, String> {
    let mut file = zip.by_name(name).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(root: &Path) -> Place {
        Place { space: None, projects_dir: root.into(), data_dir: root.into(), exports_dir: root.into() }
    }

    #[test]
    fn a_project_travels_with_its_media() {
        let root = std::env::temp_dir().join(format!("forma-archive-{}", std::process::id()));
        let (here, there) = (root.join("here"), root.join("there"));
        std::fs::create_dir_all(&here).unwrap();
        let picture = here.join("red.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255])).save(&picture).unwrap();
        let (url, _, _) = assets::import_picture(&picture, &place(&here)).unwrap();
        let mut scene = Scene::empty("0a1b2c3d-0000-4000-8000-000000000002".into());
        for id in ["a", "b"] {
            scene.add_shape(id.into(), "image");
            scene.shapes.last_mut().unwrap().extra.insert("assetData".into(), url.clone().into());
        }
        scene.add_shape("c".into(), "svg");
        scene.shapes[2].extra.insert("svgSource".into(), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2\" height=\"2\"/>".into());
        let file = root.join("trip.forma");
        pack(&scene, &place(&here), &file).unwrap();

        let mut zip = ZipArchive::new(File::open(&file).unwrap()).unwrap();
        let names: Vec<String> = zip.file_names().map(|name| name.unwrap().into_owned()).collect();
        assert_eq!(names.len(), 2, "one picture for both layers, and project.json: {names:?}");
        let media = names.iter().find(|n| n.starts_with("media/")).unwrap().clone();
        assert!(media.ends_with(".png"));
        assert_eq!(zip.by_name(&media).unwrap().compression(), CompressionMethod::Stored);

        let opened = unpack(&file, &place(&there)).unwrap();
        assert_eq!(opened.shapes[0].extra["assetData"], url.as_str(), "same bytes, same library name");
        assert!(there.join("media").join(url.trim_start_matches("forma-asset://media/")).is_file());
        assert_eq!(opened.shapes[2].extra["svgSource"], scene.shapes[2].extra["svgSource"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opens_files_packed_by_electron_forma() {
        // Made by Electron Forma's packProjectFile code path (fflate zipSync, media stored, project.json deflated).
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/electron.forma");
        let root = std::env::temp_dir().join(format!("forma-archive-electron-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let scene = unpack(&file, &place(&root)).unwrap();
        assert_eq!(scene.project_name, "From Electron");
        let url = scene.shapes[0].extra["assetData"].as_str().unwrap();
        assert!(url.starts_with("forma-asset://media/") && url.ends_with(".png"));
        assert_eq!(crate::render::frame(&scene, &place(&root), 0.0).unwrap().pixel(480, 300).unwrap().blue(), 255);
        std::fs::remove_dir_all(root).unwrap();
    }
}
