//! Forma's project folder is the same `<dataDir>/projects/*.json` used by Electron Forma.

use crate::model::Scene;
use canopy_ui::{Document, Place};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

fn projects(place: &Place) -> PathBuf {
    place.data_dir.join("projects")
}

pub fn list(place: &Place) -> Vec<Document> {
    let Ok(entries) = std::fs::read_dir(projects(place)) else {
        return Vec::new();
    };
    let mut documents: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let id = path.file_stem()?.to_str()?.to_owned();
            if path.extension()?.to_str()? != "json" || !valid_id(&id) {
                return None;
            }
            let scene = load(&path).ok()?;
            let saved_at = entry
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_millis() as u64;
            Some(Document {
                id,
                name: scene.project_name,
                saved_at,
                detail: format!("{} layers", scene.shapes.len()),
            })
        })
        .collect();
    documents.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    documents
}

fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

pub fn load(path: &Path) -> Result<Scene, String> {
    Scene::read(&std::fs::read(path).map_err(|e| e.to_string())?)
}

pub fn load_document(place: &Place, id: &str) -> Result<Scene, String> {
    if !valid_id(id) {
        return Err("Invalid project ID".into());
    }
    load(&projects(place).join(format!("{id}.json")))
}

/// Load Electron Forma's default project as a fresh project, when one is present.
pub fn load_default(place: &Place, id: String) -> Result<Option<Scene>, String> {
    let path = place.data_dir.join("default-project.json");
    if !path.exists() {
        return Ok(None);
    }
    let mut scene = load(&path)?;
    for shape in &mut scene.shapes {
        for field in ["assetData", "posterData", "svgSource"] {
            let ref_field = format!("{field}Ref");
            let Some(token) = shape.extra.get(&ref_field).and_then(|v| v.as_str()) else {
                continue;
            };
            if !valid_id(token) {
                return Err("Invalid media reference".into());
            }
            let data = std::fs::read_to_string(place.data_dir.join("assets").join(token))
                .map_err(|e| e.to_string())?;
            shape.extra.insert(field.into(), data.into());
            shape.extra.remove(&ref_field);
        }
    }
    scene.project_id = id;
    Ok(Some(scene))
}

pub fn save(place: &Place, scene: &Scene) -> Result<(), String> {
    if !valid_id(&scene.project_id) {
        return Err("Invalid project ID".into());
    }
    let data = serde_json::to_vec(scene).map_err(|e| e.to_string())?;
    let folder = projects(place);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    atomic(&folder.join(format!("{}.json", scene.project_id)), &data)?;
    atomic(&place.data_dir.join("last-session.json"), &data)
}

fn atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, data).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_electron_project_layout() {
        let root =
            std::env::temp_dir().join(format!("forma-native-storage-{}", std::process::id()));
        let place = Place {
            space: None,
            projects_dir: root.join("unused"),
            data_dir: root.clone(),
            exports_dir: root.join("exports"),
        };
        let scene = Scene::empty("00000000-0000-0000-0000-000000000001".into());
        save(&place, &scene).unwrap();
        save(&place, &scene).unwrap();
        assert!(
            root.join("projects")
                .join(format!("{}.json", scene.project_id))
                .is_file()
        );
        assert!(root.join("last-session.json").is_file());
        assert_eq!(list(&place)[0].name, "Untitled exploration");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn default_project_is_a_new_project_and_missing_default_is_none() {
        let root = std::env::temp_dir().join(format!(
            "forma-default-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let place = Place {
            space: None,
            projects_dir: root.join("unused"),
            data_dir: root.clone(),
            exports_dir: root.join("exports"),
        };
        assert!(load_default(&place, "fresh".into()).unwrap().is_none());
        let mut scene = Scene::example("old".into());
        scene.project_name = "My default".into();
        let token = "00000000-0000-0000-0000-000000000002";
        scene.shapes[0]
            .extra
            .insert("assetDataRef".into(), token.into());
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(
            root.join("assets").join(token),
            "data:image/png;base64,AA==",
        )
        .unwrap();
        std::fs::write(
            root.join("default-project.json"),
            serde_json::to_vec(&scene).unwrap(),
        )
        .unwrap();
        let loaded = load_default(&place, "fresh".into()).unwrap().unwrap();
        assert_eq!(loaded.project_id, "fresh");
        assert_eq!(loaded.project_name, "My default");
        assert_eq!(loaded.shapes.len(), scene.shapes.len());
        assert_eq!(
            loaded.shapes[0].extra["assetData"],
            "data:image/png;base64,AA=="
        );
        assert!(!loaded.shapes[0].extra.contains_key("assetDataRef"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
