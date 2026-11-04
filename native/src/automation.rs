//! Forma MCP's thirteen editing tools for the native editor, with the arguments, results and error codes of Electron
//! Forma's `src/automation.js`, run against the open scene on the UI thread. `bridge.rs` brings the calls in.

use crate::contract::{ToolError, fail};
use crate::model::{ColorKey, Key, Pose, Scene, Shape};
use crate::{animated_svg, assets, render, video};
use base64::Engine;
use canopy_ui::Place;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The editor state a tool works on.
pub struct Editor<'a> {
    pub scene: &'a mut Scene,
    pub selected: &'a mut Option<String>,
    pub time: &'a mut f64,
    pub playing: &'a mut bool,
    pub place: &'a Place,
    /// Where exports go: the app's export folder, or `FORMA_EXPORT_DIR` as in Electron Forma.
    pub exports: &'a Path,
}

/// What a tool call comes to: an answer, a scene to open as a project of its own (the app keeps the current one, then
/// answers with the new scene), or a video rendering (answered when it's done).
pub enum Outcome {
    Done(Value),
    Open(Scene),
    Video(PendingVideo),
}

pub struct PendingVideo {
    pub export: video::Export,
    /// Rendered here first, then moved to `destination`, so a failed render leaves no half file behind.
    pub temporary: PathBuf,
    pub destination: PathBuf,
    pub mime: &'static str,
}

impl PendingVideo {
    /// The answer once the render is done.
    pub fn finish(&self, rendered: Result<PathBuf, String>) -> Result<Value, ToolError> {
        let rendered = rendered.map_err(|e| fail("EXPORT_FAILED", e))?;
        std::fs::rename(&rendered, &self.destination)
            .map_err(|e| fail("EXPORT_FAILED", format!("Couldn't keep the video ({e}).")))?;
        exported(&self.destination, self.mime)
    }
}

pub fn dispatch(e: &mut Editor, name: &str, args: &Value) -> Result<Outcome, ToolError> {
    let done = |value: Value| Ok(Outcome::Done(value));
    match name {
        "forma_get_scene" => done(snapshot(e, args["includeAssets"] == true)),
        "forma_create_scene" => {
            confirmed(args, "replace")?;
            let mut scene = Scene::empty(crate::app::new_id());
            scene.width = args["width"].as_f64().unwrap_or(960.0) as f32;
            scene.height = args["height"].as_f64().unwrap_or(600.0) as f32;
            scene.duration = args["duration"].as_f64().unwrap_or(4.0);
            scene.bg = args["bg"].as_str().unwrap_or("#f4f0e8").into();
            usable(&scene)?;
            Ok(Outcome::Open(scene))
        }
        "forma_update_scene" => {
            let mut next = e.scene.clone();
            if let Some(width) = args["width"].as_f64() {
                next.width = width as f32;
            }
            if let Some(height) = args["height"].as_f64() {
                next.height = height as f32;
            }
            if let Some(duration) = args["duration"].as_f64() {
                next.duration = duration;
            }
            if let Some(bg) = args["bg"].as_str() {
                next.bg = bg.into();
            }
            let late = |t: f64| t > next.duration;
            if next.shapes.iter().any(|s| {
                s.keys.iter().any(|k| late(k.t))
                    || s.color_keys.iter().any(|k| late(k.t))
                    || late(s.in_time.unwrap_or(0.0))
                    || late(s.out_time.unwrap_or(0.0))
            }) {
                return Err(fail(
                    "KEYFRAMES_OUT_OF_RANGE",
                    "Remove or retime later keyframes and clips before shortening the scene.",
                ));
            }
            install(e, next, None)?;
            done(snapshot(e, false))
        }
        "forma_add_shape" => {
            let mut next = e.scene.clone();
            let mut layer = new_layer(&next, args, args["type"].as_str().unwrap_or("rect"));
            clip_timing(&mut next, &mut layer, args)?;
            let id = layer.id.clone();
            next.shapes.push(layer);
            install(e, next, Some(id.clone()))?;
            done(summarized(e.scene, &id))
        }
        "forma_update_shape" => {
            let id = args["id"].as_str().unwrap_or_default();
            let mut next = e.scene.clone();
            let index = find(&next, id)?;
            let is_text = next.shapes[index].kind == "text";
            if (args.get("text").is_some() || args.get("linkId").is_some()) && !is_text {
                return Err(fail(
                    "INVALID_LAYER_TYPE",
                    "Only text layers have text or linkId.",
                ));
            }
            if let Some(link) = args.get("linkId").and_then(Value::as_str) {
                let pose = e.scene.render_pose(&e.scene.shapes[index], *e.time);
                let layer = &mut next.shapes[index];
                if link.is_empty() {
                    layer.extra.insert("linkId".into(), "".into());
                    layer.keys = vec![key_at(0.0, pose)];
                } else {
                    let target = &e.scene.shapes[find(e.scene, link)?];
                    if target.kind == "text" {
                        return Err(fail("INVALID_LINK", "Text must follow a non-text layer."));
                    }
                    let to = e.scene.render_pose(target, *e.time);
                    let angle = (-to.r as f64).to_radians();
                    let (dx, dy) = ((pose.x - to.x) as f64, (pose.y - to.y) as f64);
                    for (field, value) in [
                        ("linkId", json!(target.id)),
                        ("linkBase", json!({"w": to.w, "h": to.h})),
                        ("offsetX", json!(dx * angle.cos() - dy * angle.sin())),
                        ("offsetY", json!(dx * angle.sin() + dy * angle.cos())),
                        ("textWidth", json!(pose.w)),
                        ("fontSize", json!(pose.h)),
                        ("rotationOffset", json!(pose.r - to.r)),
                        (
                            "opacityFactor",
                            json!(if to.o != 0.0 { pose.o / to.o } else { 1.0 }),
                        ),
                    ] {
                        layer.extra.insert(field.into(), value);
                    }
                }
            }
            let mut layer = next.shapes[index].clone();
            if let Some(name) = args["name"].as_str() {
                layer.name = name.into();
            }
            if let Some(color) = args["color"].as_str() {
                layer.color = color.into();
            }
            if let Some(ease) = args["ease"].as_str() {
                layer.ease = ease.into();
            }
            if let Some(text) = args["text"].as_str() {
                layer.extra.insert("text".into(), text.into());
            }
            clip_timing(&mut next, &mut layer, args)?;
            next.shapes[index] = layer;
            install(e, next, Some(id.into()))?;
            done(summarized(e.scene, id))
        }
        "forma_duplicate_shape" => {
            let mut next = e.scene.clone();
            let index = find(&next, args["id"].as_str().unwrap_or_default())?;
            let at = if args["placement"].as_str().unwrap_or("after") == "after" {
                next.continue_after(
                    index,
                    None,
                    *e.time,
                    crate::app::new_id(),
                    crate::app::new_id(),
                )
                .map_err(|e| fail("INVALID_CLIP", e))?
            } else {
                next.duplicate_layer(index, crate::app::new_id())
            };
            // x/y move the copy (keys and drawn moves alike), which makes the picture jump there: stop motion.
            let copy = &mut next.shapes[at];
            let first = (copy.keys[0].x, copy.keys[0].y);
            let (dx, dy) = (
                args["x"].as_f64().map_or(0.0, |x| x as f32 - first.0),
                args["y"].as_f64().map_or(0.0, |y| y as f32 - first.1),
            );
            for key in &mut copy.keys {
                (key.x, key.y) = (key.x + dx, key.y + dy);
            }
            if let Some(Value::Array(motions)) = copy.extra.get_mut("motions") {
                for motion in motions {
                    shift_point(&mut motion["startPose"], dx, dy);
                    for field in ["sourcePoints", "baseKeys"] {
                        if let Some(points) = motion.get_mut(field).and_then(Value::as_array_mut) {
                            points.iter_mut().for_each(|p| shift_point(p, dx, dy));
                        }
                    }
                }
            }
            let id = copy.id.clone();
            install(e, next, Some(id.clone()))?;
            done(summarized(e.scene, &id))
        }
        "forma_set_keyframes" => {
            let id = args["id"].as_str().unwrap_or_default();
            let mut next = e.scene.clone();
            let index = find(&next, id)?;
            let given: Vec<Key> = serde_json::from_value(args["keyframes"].clone())
                .map_err(|e| fail("INVALID_ARGUMENTS", e.to_string()))?;
            let mut times: Vec<f64> = given.iter().map(|k| k.t).collect();
            times.sort_by(f64::total_cmp);
            if times.windows(2).any(|w| w[0] == w[1]) {
                return Err(fail("DUPLICATE_TIME", "Each keyframe needs a unique time."));
            }
            let layer = &mut next.shapes[index];
            let mut keys = if args["replace"] == true {
                Vec::new()
            } else {
                layer.keys.clone()
            };
            for key in given {
                keys.retain(|k| k.t != key.t);
                keys.push(key);
            }
            keys.sort_by(|a, b| a.t.total_cmp(&b.t));
            layer.keys = keys;
            // A drawn move would write its keyframes back over these.
            layer.extra.remove("motions");
            install(e, next, Some(id.into()))?;
            done(summarized(e.scene, id))
        }
        "forma_delete_shape" => {
            confirmed(args, "confirm")?;
            let id = args["id"].as_str().unwrap_or_default();
            find(e.scene, id)?;
            let mut next = e.scene.clone();
            // Text that followed it stays where it is now.
            for (i, layer) in e.scene.shapes.iter().enumerate() {
                if layer.kind == "text"
                    && layer.extra.get("linkId").and_then(Value::as_str) == Some(id)
                {
                    next.shapes[i].keys = vec![key_at(0.0, e.scene.render_pose(layer, *e.time))];
                    next.shapes[i].extra.insert("linkId".into(), "".into());
                }
            }
            next.shapes.retain(|layer| layer.id != id);
            install(e, next, None)?;
            done(json!({"deleted": id}))
        }
        "forma_set_playback" => {
            match args["action"].as_str() {
                Some("seek") => {
                    let at = args["time"]
                        .as_f64()
                        .ok_or_else(|| fail("INVALID_TIME", "Seek requires time."))?;
                    *e.time = checked_time(e, Some(at))?;
                    *e.playing = false;
                }
                Some("pause") => *e.playing = false,
                Some("play") => *e.playing = true,
                _ => return Err(fail("INVALID_ACTION", "Unknown playback action.")),
            }
            done(json!({"time": *e.time, "playing": *e.playing}))
        }
        "forma_preview" => {
            let at = checked_time(e, args["time"].as_f64())?;
            let most = args["maxWidth"].as_f64().unwrap_or(960.0);
            let ratio = (most / e.scene.width as f64)
                .min(1600.0 / e.scene.height as f64)
                .min(1.0);
            let size = (
                (e.scene.width as f64 * ratio).round().max(1.0) as u32,
                (e.scene.height as f64 * ratio).round().max(1.0) as u32,
            );
            let png = png_bytes(
                &render::frame_sized(e.scene, e.place, at, size)
                    .map_err(|e| fail("EXPORT_FAILED", e))?,
            )?;
            done(
                json!({"data": base64::engine::general_purpose::STANDARD.encode(png), "mimeType": "image/png", "width": size.0, "height": size.1, "time": at}),
            )
        }
        "forma_export" => export(e, args),
        "forma_open_project" => {
            confirmed(args, "replace")?;
            let bytes = local_file(args["file"].as_str().unwrap_or_default(), &["json"])?;
            let mut scene = Scene::read(&bytes)
                .map_err(|_| fail("INVALID_PROJECT", "This file is not a valid Forma project."))?;
            usable(&scene)
                .map_err(|_| fail("INVALID_PROJECT", "This file is not a valid Forma project."))?;
            // Like opening a file in the editor, it becomes a project of its own: the open one is kept as it is.
            scene.project_id = crate::app::new_id();
            Ok(Outcome::Open(scene))
        }
        "forma_import_asset" => {
            let file = args["file"].as_str().unwrap_or_default();
            let extension = extension_of(file);
            let bytes = local_file(file, &["png", "jpg", "jpeg", "webp", "svg"])?;
            let path = Path::new(file);
            let base = path
                .file_name()
                .map(|n| n.to_string_lossy().chars().take(99).collect::<String>())
                .unwrap_or_default();
            let mut with_name = args.clone();
            if with_name.get("name").is_none() {
                with_name["name"] = json!(base);
            }
            let (kind, field, value, natural) = if extension == "svg" {
                let (source, w, h) =
                    assets::import_svg(path).map_err(|e| fail("INVALID_ASSET", e))?;
                ("svg", "svgSource", source, (w, h))
            } else {
                let looks_right = match extension.as_str() {
                    "png" => bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]),
                    "webp" => {
                        bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
                    }
                    _ => bytes.starts_with(&[255, 216, 255]),
                };
                if !looks_right {
                    return Err(fail(
                        "INVALID_ASSET",
                        "File content does not match its image extension.",
                    ));
                }
                let (url, w, h) =
                    assets::import_picture(path, e.place).map_err(|e| fail("INVALID_ASSET", e))?;
                ("image", "assetData", url, (w as f32, h as f32))
            };
            if natural.0 <= 0.0 || natural.1 <= 0.0 {
                return Err(fail("INVALID_ASSET", "Image has no usable dimensions."));
            }
            let mut layer = new_layer(e.scene, &with_name, kind);
            layer.extra.insert(field.into(), value.into());
            let w = args["w"]
                .as_f64()
                .map_or(natural.0.min(e.scene.width * 0.6), |w| w as f32);
            let h = args["h"]
                .as_f64()
                .map_or((w * natural.1 / natural.0).min(10000.0), |h| h as f32);
            (layer.keys[0].w, layer.keys[0].h) = (w, h);
            let id = layer.id.clone();
            let mut next = e.scene.clone();
            next.shapes.push(layer);
            install(e, next, Some(id.clone()))?;
            done(summarized(e.scene, &id))
        }
        _ => Err(fail("UNKNOWN_TOOL", "Unknown Forma tool.")),
    }
}

/// What `forma_get_scene` reports: the project (media as byte counts unless asked for), selection, playback, and
/// every layer's transform now.
pub fn snapshot(e: &Editor, include_assets: bool) -> Value {
    let mut project = serde_json::to_value(&*e.scene).unwrap_or_default();
    if !include_assets {
        for layer in project["shapes"]
            .as_array_mut()
            .into_iter()
            .flatten()
            .filter_map(Value::as_object_mut)
        {
            omit_media(layer, true);
        }
    }
    let transforms: Vec<Value> = e.scene.shapes.iter().map(|layer| {
        let p = e.scene.render_pose(layer, *e.time);
        json!({"id": layer.id, "x": p.x, "y": p.y, "w": p.w, "h": p.h, "r": p.r, "o": p.o, "color": layer.color_at(*e.time)})
    }).collect();
    json!({
        "project": project, "selected": e.selected.clone(), "time": *e.time, "playing": *e.playing, "transforms": transforms,
        "connection": {"enabled": true, "exportDirectory": e.exports.display().to_string()},
    })
}

fn omit_media(layer: &mut Map<String, Value>, count: bool) {
    for field in ["assetData", "posterData", "svgSource"] {
        if let Some(Value::String(value)) = layer.remove(field) {
            if count {
                layer.insert(format!("{field}Bytes"), value.len().into());
            }
        }
    }
}

fn summarized(scene: &Scene, id: &str) -> Value {
    let mut layer = scene
        .shapes
        .iter()
        .find(|s| s.id == id)
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .unwrap_or_default();
    if let Some(map) = layer.as_object_mut() {
        omit_media(map, false);
    }
    json!({"layer": layer})
}

fn confirmed(args: &Value, field: &str) -> Result<(), ToolError> {
    if args[field] == true {
        Ok(())
    } else {
        Err(fail(
            "CONFIRMATION_REQUIRED",
            format!("{field}=true is required."),
        ))
    }
}

fn find(scene: &Scene, id: &str) -> Result<usize, ToolError> {
    scene
        .shapes
        .iter()
        .position(|layer| layer.id == id)
        .ok_or_else(|| {
            fail(
                "LAYER_NOT_FOUND",
                format!("No layer has ID {id}. Inspect forma_get_scene for current IDs."),
            )
        })
}

fn checked_time(e: &Editor, value: Option<f64>) -> Result<f64, ToolError> {
    let at = value.unwrap_or(*e.time);
    if at.is_finite() && (0.0..=e.scene.duration).contains(&at) {
        Ok(at)
    } else {
        Err(fail(
            "INVALID_TIME",
            format!("Time must be inside 0–{} seconds.", e.scene.duration),
        ))
    }
}

fn key_at(t: f64, p: Pose) -> Key {
    Key {
        t,
        x: p.x,
        y: p.y,
        w: p.w,
        h: p.h,
        r: p.r,
        o: p.o,
        ease: None,
        curve: None,
        extra: Map::new(),
    }
}

fn shift_point(point: &mut Value, dx: f32, dy: f32) {
    for (axis, by) in [("x", dx), ("y", dy)] {
        if let Some(v) = point.get(axis).and_then(Value::as_f64) {
            point[axis] = json!(v + by as f64);
        }
    }
}

/// Whether the edited scene is one Forma can open: what `Scene::read` accepts, keyframes within the scene and at most
/// 1000 a layer (Electron's `usableProject`).
fn usable(scene: &Scene) -> Result<(), ToolError> {
    let bytes = serde_json::to_vec(scene).map_err(|e| fail("INVALID_PROJECT", e.to_string()))?;
    let readable = Scene::read(&bytes).is_ok();
    let timed = scene.shapes.iter().all(|s| {
        s.keys.len() <= 1000
            && s.keys
                .iter()
                .all(|k| k.t >= 0.0 && k.t <= scene.duration + 1e-9)
    });
    if readable && timed {
        Ok(())
    } else {
        Err(fail(
            "INVALID_PROJECT",
            "The edit would create an invalid Forma scene. Check sizes, timestamps, colors, motion metadata, and layer limits.",
        ))
    }
}

/// Puts an edited scene in place: playback stops, the selection stays if it can (else the first layer), and the
/// playhead stays inside the scene. The app's history and autosave pick the change up.
fn install(e: &mut Editor, next: Scene, select: Option<String>) -> Result<(), ToolError> {
    usable(&next)?;
    *e.playing = false;
    let wanted = select.or_else(|| e.selected.clone());
    *e.selected = wanted
        .filter(|id| next.shapes.iter().any(|s| &s.id == id))
        .or_else(|| next.shapes.first().map(|s| s.id.clone()));
    *e.time = e.time.min(next.duration);
    *e.scene = next;
    Ok(())
}

/// A new layer as Electron's automation makes one (`newLayer`): 160 px across (text 240 by 36), coral, smooth, in the
/// middle of the scene unless placed.
fn new_layer(scene: &Scene, args: &Value, kind: &str) -> Shape {
    let text = kind == "text";
    let number = |field: &str, default: f64| args[field].as_f64().unwrap_or(default);
    let (w, h) = (
        number("w", if text { 240.0 } else { 160.0 }),
        number("h", if text { 36.0 } else { 160.0 }),
    );
    let mut title = kind.to_owned();
    title[..1].make_ascii_uppercase();
    let mut layer = json!({
        "id": crate::app::new_id(),
        "name": args["name"].as_str().unwrap_or(if text { "Text" } else { &title }),
        "type": kind,
        "color": args["color"].as_str().unwrap_or("#ed795e"),
        "ease": args["ease"].as_str().unwrap_or("smooth"),
        "keys": [{"t": 0, "x": number("x", scene.width as f64 / 2.0), "y": number("y", scene.height as f64 / 2.0), "w": w, "h": h, "r": number("r", 0.0), "o": number("o", 100.0)}],
    });
    if text {
        for (field, value) in [
            ("text", json!(args["text"].as_str().unwrap_or("Your text"))),
            ("linkId", json!("")),
            ("offsetX", json!(0)),
            ("offsetY", json!(0)),
            ("textWidth", json!(w)),
            ("fontSize", json!(h)),
        ] {
            layer[field] = value;
        }
    }
    serde_json::from_value(layer).expect("a new layer is a valid shape")
}

/// `inTime`/`outTime` trim a clip without moving its keyframes (a null `outTime` lasts to the end), `trackId` puts it on
/// a row ("" takes it off), `colorKeys` replaces its color keyframes; the scene grows to fit.
fn clip_timing(scene: &mut Scene, layer: &mut Shape, args: &Value) -> Result<(), ToolError> {
    if let Some(start) = args["inTime"].as_f64() {
        layer.in_time = Some(start);
    }
    match args.get("outTime") {
        Some(Value::Null) => layer.out_time = None,
        Some(end) => layer.out_time = end.as_f64(),
        None => {}
    }
    match args["trackId"].as_str() {
        Some("") => {
            layer.extra.remove("trackId");
        }
        Some(track) => {
            layer.extra.insert("trackId".into(), track.into());
        }
        None => {}
    }
    if layer
        .out_time
        .is_some_and(|end| end <= layer.in_time.unwrap_or(0.0))
    {
        return Err(fail("INVALID_TIMING", "outTime must be later than inTime."));
    }
    if let Some(keys) = args["colorKeys"].as_array() {
        layer.color_keys = keys
            .iter()
            .map(|k| ColorKey {
                t: k["t"].as_f64().unwrap_or(0.0),
                color: k["color"].as_str().unwrap_or("#000000").into(),
                extra: Map::new(),
            })
            .collect();
        layer.color_keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    }
    scene.duration = scene
        .duration
        .max(layer.in_time.unwrap_or(0.0))
        .max(layer.out_time.unwrap_or(0.0));
    Ok(())
}

fn extension_of(file: &str) -> String {
    Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// A file to read for a tool: an absolute path to a regular file with one of `extensions`.
fn local_file(file: &str, extensions: &[&str]) -> Result<Vec<u8>, ToolError> {
    let path = Path::new(file);
    if !path.is_absolute() {
        return Err(fail("INVALID_PATH", "Use an absolute local path."));
    }
    if !extensions.contains(&extension_of(file).as_str()) {
        let listed: Vec<String> = extensions.iter().map(|e| format!(".{e}")).collect();
        return Err(fail(
            "UNSUPPORTED_FILE",
            format!("Supported file extensions: {}", listed.join(", ")),
        ));
    }
    if !path.is_file() {
        return Err(fail("INVALID_FILE", "Path must point to a regular file."));
    }
    std::fs::read(path).map_err(|e| fail("INVALID_FILE", e.to_string()))
}

fn png_bytes(pixmap: &tiny_skia::Pixmap) -> Result<Vec<u8>, ToolError> {
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        pixmap.data(),
        pixmap.width(),
        pixmap.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| fail("EXPORT_FAILED", format!("PNG rendering failed: {e}")))?;
    Ok(png)
}

fn exported(path: &Path, mime: &str) -> Result<Value, ToolError> {
    let bytes = std::fs::metadata(path)
        .map_err(|e| fail("EXPORT_FAILED", e.to_string()))?
        .len();
    Ok(json!({"path": path.display().to_string(), "bytes": bytes, "mimeType": mime}))
}

/// `forma_export`: a simple file name with the right extension, in the export folder, never over an existing file
/// unless `overwrite` (written whole, then moved into place).
fn export(e: &mut Editor, args: &Value) -> Result<Outcome, ToolError> {
    let format = args["format"].as_str().unwrap_or_default();
    let filename = args["filename"].as_str().unwrap_or_default();
    let extension = match format {
        "png" => "png",
        "svg" | "animated_svg" => "svg",
        "mp4" => "mp4",
        "webm" => "webm",
        _ => "json",
    };
    let stem = filename
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let reserved = ["con", "prn", "aux", "nul"].contains(&stem.as_str())
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if Path::new(filename).file_name().and_then(|n| n.to_str()) != Some(filename)
        || filename.ends_with([' ', '.'])
        || reserved
        || extension_of(filename) != extension
    {
        return Err(fail(
            "INVALID_FILENAME",
            format!("Use a simple filename ending in .{extension}."),
        ));
    }
    std::fs::create_dir_all(e.exports).map_err(|e| fail("EXPORT_FAILED", e.to_string()))?;
    let destination = e.exports.join(filename);
    if args["overwrite"] != true && std::fs::symlink_metadata(&destination).is_ok() {
        return Err(fail(
            "FILE_EXISTS",
            "Export already exists; choose a new name or set overwrite=true.",
        ));
    }
    let at = checked_time(e, args["time"].as_f64())?;
    let temporary = e
        .exports
        .join(format!(".forma-{}.tmp.{extension}", crate::app::new_id()));
    if format == "mp4" || format == "webm" {
        let fps = args["fps"].as_u64().unwrap_or(30) as u32;
        let settings = video::Settings {
            format: if format == "mp4" {
                video::Format::Mp4
            } else {
                video::Format::WebM
            },
            fps,
            scale: 1,
        };
        *e.playing = false;
        let export = video::start(
            e.scene,
            e.place,
            temporary.clone(),
            settings,
            egui::Context::default(),
        )
        .map_err(|e| fail("EXPORT_FAILED", e))?;
        let mime = if format == "mp4" {
            "video/mp4"
        } else {
            "video/webm"
        };
        return Ok(Outcome::Video(PendingVideo {
            export,
            temporary,
            destination,
            mime,
        }));
    }
    let (bytes, mime) = match format {
        "png" => (
            png_bytes(&render::frame(e.scene, e.place, at).map_err(|e| fail("EXPORT_FAILED", e))?)?,
            "image/png",
        ),
        "svg" => (
            animated_svg::still(e.scene, e.place, at)
                .map_err(|e| fail("EXPORT_FAILED", e))?
                .into_bytes(),
            "image/svg+xml",
        ),
        "animated_svg" => (
            animated_svg::animated(e.scene, e.place)
                .map_err(|e| fail("EXPORT_FAILED", e))?
                .into_bytes(),
            "image/svg+xml",
        ),
        _ => (
            serde_json::to_vec_pretty(&inline_media(e.scene, e.place)?)
                .map_err(|e| fail("EXPORT_FAILED", e.to_string()))?,
            "application/json",
        ),
    };
    let written =
        std::fs::write(&temporary, &bytes).and_then(|()| std::fs::rename(&temporary, &destination));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(fail("EXPORT_FAILED", error.to_string()));
    }
    Ok(Outcome::Done(exported(&destination, mime)?))
}

/// The scene with its media as data URLs, so the project opens anywhere (Electron's `inlineMedia`).
fn inline_media(scene: &Scene, place: &Place) -> Result<Value, ToolError> {
    let mut project =
        serde_json::to_value(scene).map_err(|e| fail("EXPORT_FAILED", e.to_string()))?;
    let error = |e: String| fail("EXPORT_FAILED", e);
    for (shape, copy) in scene
        .shapes
        .iter()
        .zip(project["shapes"].as_array_mut().into_iter().flatten())
    {
        let Some(copy) = copy.as_object_mut() else {
            continue;
        };
        for field in ["assetData", "posterData"] {
            let Some(value) = assets::stored(shape, field, place).map_err(error)? else {
                continue;
            };
            if !value.starts_with("data:") {
                let (bytes, extension) = assets::media_bytes(&value, place).map_err(error)?;
                let mime = match extension.as_str() {
                    "png" => "image/png",
                    "jpg" => "image/jpeg",
                    "webp" => "image/webp",
                    "mp4" => "video/mp4",
                    "webm" => "video/webm",
                    "ogv" => "video/ogg",
                    _ => "application/octet-stream",
                };
                copy.insert(
                    field.into(),
                    format!(
                        "data:{mime};base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(bytes)
                    )
                    .into(),
                );
            } else {
                copy.insert(field.into(), value.into());
            }
            copy.remove(&format!("{field}Ref"));
        }
        if let Some(source) = assets::svg_source(shape, place).map_err(error)? {
            copy.insert("svgSource".into(), source.into());
            copy.remove("svgSourceRef");
        }
    }
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        scene: Scene,
        selected: Option<String>,
        time: f64,
        playing: bool,
        place: Place,
        exports: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("forma-automation-{}", crate::app::new_id()));
            let place = Place {
                space: None,
                projects_dir: root.join("projects"),
                data_dir: root.join("data"),
                exports_dir: root.join("exports"),
            };
            Fixture {
                scene: Scene::empty("test".into()),
                selected: None,
                time: 0.0,
                playing: false,
                exports: root.join("exports"),
                place,
            }
        }

        fn call(&mut self, name: &str, args: Value) -> Result<Value, ToolError> {
            crate::contract::validate(name, &args)?;
            let mut editor = Editor {
                scene: &mut self.scene,
                selected: &mut self.selected,
                time: &mut self.time,
                playing: &mut self.playing,
                place: &self.place,
                exports: &self.exports,
            };
            match dispatch(&mut editor, name, &args)? {
                Outcome::Done(value) => Ok(value),
                Outcome::Open(scene) => {
                    self.scene = scene;
                    let editor = Editor {
                        scene: &mut self.scene,
                        selected: &mut self.selected,
                        time: &mut self.time,
                        playing: &mut self.playing,
                        place: &self.place,
                        exports: &self.exports,
                    };
                    Ok(snapshot(&editor, false))
                }
                Outcome::Video(pending) => {
                    let rendered = pending
                        .export
                        .result
                        .recv()
                        .map_err(|e| fail("EXPORT_FAILED", e.to_string()))?;
                    pending.finish(rendered)
                }
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.place.data_dir.parent().unwrap());
        }
    }

    #[test]
    fn tools_build_and_inspect_a_scene() {
        let mut f = Fixture::new();
        let scene = f
            .call(
                "forma_create_scene",
                json!({"replace": true, "width": 800, "height": 400, "bg": "#101820"}),
            )
            .unwrap();
        assert_eq!(
            (
                scene["project"]["width"].as_f64(),
                scene["project"]["bg"].as_str()
            ),
            (Some(800.0), Some("#101820"))
        );
        let circle = f
            .call(
                "forma_add_shape",
                json!({"type": "circle", "color": "#ff7f50", "x": 100, "y": 200}),
            )
            .unwrap();
        let id = circle["layer"]["id"].as_str().unwrap().to_owned();
        assert_eq!(
            (
                circle["layer"]["name"].as_str(),
                circle["layer"]["keys"][0]["w"].as_f64()
            ),
            (Some("Circle"), Some(160.0))
        );
        f.call("forma_set_keyframes", json!({"id": id, "keyframes": [{"t": 4, "x": 700, "y": 200, "w": 160, "h": 160, "r": 360, "o": 100}]})).unwrap();
        // Text that follows the circle keeps its place beside it as it moves.
        let caption = f
            .call(
                "forma_add_shape",
                json!({"type": "text", "text": "Hello", "x": 100, "y": 100}),
            )
            .unwrap();
        let caption_id = caption["layer"]["id"].as_str().unwrap().to_owned();
        f.call(
            "forma_update_shape",
            json!({"id": caption_id, "linkId": id}),
        )
        .unwrap();
        f.call("forma_set_playback", json!({"action": "seek", "time": 4}))
            .unwrap();
        let seen = f.call("forma_get_scene", json!({})).unwrap();
        let at = |which: &str| {
            seen["transforms"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == which)
                .unwrap()
                .clone()
        };
        assert_eq!(
            (at(&id)["x"].as_f64(), at(&id)["r"].as_f64()),
            (Some(700.0), Some(360.0))
        );
        assert!(
            (at(&caption_id)["y"].as_f64().unwrap() - 100.0).abs() < 1.0,
            "the caption stays 100 px above: {}",
            at(&caption_id)
        );
        assert_eq!(seen["selected"].as_str(), Some(caption_id.as_str()));
        // Stop motion: copies after the clip, each jumping somewhere else.
        f.call("forma_update_shape", json!({"id": id, "outTime": 1}))
            .unwrap();
        let copy = f
            .call("forma_duplicate_shape", json!({"id": id, "x": 300}))
            .unwrap();
        assert_eq!(
            (
                copy["layer"]["inTime"].as_f64(),
                copy["layer"]["keys"][0]["x"].as_f64()
            ),
            (Some(1.0), Some(300.0))
        );
        // Mistakes come back as the codes Electron Forma uses.
        let code = |r: Result<Value, ToolError>| r.unwrap_err().code;
        assert_eq!(
            code(f.call("forma_update_shape", json!({"id": "nope", "name": "x"}))),
            "LAYER_NOT_FOUND"
        );
        assert_eq!(
            code(f.call("forma_update_shape", json!({"id": id, "text": "x"}))),
            "INVALID_LAYER_TYPE"
        );
        assert_eq!(
            code(f.call("forma_update_scene", json!({"duration": 2}))),
            "KEYFRAMES_OUT_OF_RANGE"
        );
        assert_eq!(
            code(f.call("forma_set_playback", json!({"action": "seek", "time": 99}))),
            "INVALID_TIME"
        );
        assert_eq!(
            code(f.call(
                "forma_update_shape",
                json!({"id": id, "inTime": 2, "outTime": 1})
            )),
            "INVALID_TIMING"
        );
        assert_eq!(
            code(f.call("forma_import_asset", json!({"file": "relative.png"}))),
            "INVALID_PATH"
        );
        // Deleting the circle leaves its caption where it was.
        f.call("forma_delete_shape", json!({"id": id, "confirm": true}))
            .unwrap();
        assert_eq!(
            f.scene
                .shapes
                .iter()
                .find(|s| s.id == caption_id)
                .unwrap()
                .extra["linkId"],
            ""
        );
    }

    #[test]
    fn previews_and_exports_are_real_files() {
        let mut f = Fixture::new();
        f.call(
            "forma_add_shape",
            json!({"type": "rect", "color": "#204080", "w": 400, "h": 300}),
        )
        .unwrap();
        let preview = f.call("forma_preview", json!({"maxWidth": 480})).unwrap();
        let png = base64::engine::general_purpose::STANDARD
            .decode(preview["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            (preview["width"].as_u64(), preview["height"].as_u64()),
            (Some(480), Some(300))
        );
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 480);
        for (format, name) in [
            ("png", "frame.png"),
            ("svg", "still.svg"),
            ("animated_svg", "moving.svg"),
            ("project", "backup.json"),
        ] {
            let out = f
                .call("forma_export", json!({"format": format, "filename": name}))
                .unwrap();
            assert!(out["bytes"].as_u64().unwrap() > 100, "{format}: {out}");
        }
        assert!(
            std::fs::read_to_string(f.exports.join("still.svg"))
                .unwrap()
                .contains("#204080")
        );
        assert_eq!(
            f.call(
                "forma_export",
                json!({"format": "png", "filename": "frame.png"})
            )
            .unwrap_err()
            .code,
            "FILE_EXISTS"
        );
        assert_eq!(
            f.call(
                "forma_export",
                json!({"format": "png", "filename": "frame.svg"})
            )
            .unwrap_err()
            .code,
            "INVALID_FILENAME"
        );
        // The backup opens again, as a project of its own.
        let reopened = f.call("forma_open_project", json!({"file": f.exports.join("backup.json").display().to_string(), "replace": true})).unwrap();
        assert_eq!(reopened["project"]["shapes"].as_array().unwrap().len(), 1);
        assert_ne!(reopened["project"]["projectId"], "test");
    }
}
