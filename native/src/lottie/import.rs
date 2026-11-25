use super::*;
use crate::{
    app::new_id,
    compositing::{LayerMask, MaskKey, MaskMode, MaskPoint, MaskPose, MaskProperty, MaskShape},
    composition::Playback,
    keyframes::Property,
    model::{ColorKey, Key, Pose, Scene, Shape},
    paths::PathData,
    shape_style::{
        Cap, Dash, Gradient, Join, PaintMode, PaintStyle, PaintTarget, Stop, StrokeStyle, Trim,
        VectorKey, VectorLayer, VectorProperty, VectorValue,
    },
    transform::Anchor,
};
use std::collections::{HashMap, HashSet};
use tiny_skia::{PathBuilder, Rect, Transform};

const BOX: [f32; 2] = [256.0; 2];

#[derive(Clone, Copy)]
struct Clock {
    fps: f64,
    first: f64,
}

impl Clock {
    fn seconds(self, frames: f64) -> f64 {
        (frames - self.first) / self.fps
    }
}

struct Sample {
    time: f64,
    value: Value,
    ease: String,
    curve: Option<[f32; 4]>,
}

pub fn import(bytes: &[u8]) -> Result<Scene, String> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let doc: Value =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid Lottie JSON: {e}"))?;
    let fps = doc["fr"]
        .as_f64()
        .filter(|fps| (1.0..=240.0).contains(fps))
        .ok_or("Invalid Lottie frame rate.")?;
    let clock = Clock {
        fps,
        first: doc["ip"].as_f64().unwrap_or(0.0),
    };
    let end = doc["op"].as_f64().ok_or("Missing Lottie duration.")?;
    let size = [number(&doc["w"])?, number(&doc["h"])?];
    let mut assets = HashMap::new();
    for asset in doc["assets"].as_array().into_iter().flatten() {
        if let Some(id) = asset["id"].as_str()
            && assets.insert(id, asset).is_some()
        {
            return Err("Duplicate Lottie asset ID.".into());
        }
    }
    let mut catalog = Catalog {
        assets,
        done: HashMap::new(),
        children: Vec::new(),
        visiting: HashSet::new(),
    };
    let mut scene = catalog.scene(&doc, clock, size, end)?;
    scene.compositions = catalog.children;
    scene.validate_compositions()?;
    Scene::read(&serde_json::to_vec(&scene).map_err(|error| error.to_string())?)
}

struct Catalog<'a> {
    assets: HashMap<&'a str, &'a Value>,
    done: HashMap<String, (String, [f32; 2])>,
    visiting: HashSet<String>,
    children: Vec<Scene>,
}

impl Catalog<'_> {
    fn source(&mut self, id: &str, layer: &Value, fps: f64) -> Result<(String, [f32; 2]), String> {
        let source = self
            .assets
            .get(id)
            .copied()
            .ok_or("Missing Lottie precomposition asset.")?;
        let layers = source["layers"]
            .as_array()
            .ok_or("A Lottie asset is not a composition.")?;
        let side = |key: &str| number(&layer[key]).or_else(|_| number(&source[key]));
        let size = [side("w")?, side("h")?];
        if let Some((native, previous)) = self.done.get(id) {
            if *previous != size {
                return Err("One Lottie asset uses different clipping sizes.".into());
            }
            return Ok((native.clone(), size));
        }
        if self.visiting.len() >= crate::composition::MAX_DEPTH
            || !self.visiting.insert(id.to_owned())
        {
            return Err("Lottie compositions form a loop or are nested too deeply.".into());
        }
        let end = source["op"].as_f64().unwrap_or_else(|| {
            layers
                .iter()
                .filter_map(|layer| layer["op"].as_f64())
                .fold(fps, f64::max)
        });
        let mut child = self.scene(source, Clock { fps, first: 0.0 }, size, end)?;
        if source["nm"].as_str().is_none() {
            child.project_name = name(layer, "Composition");
        }
        let native = child.project_id.clone();
        self.children.push(child);
        self.done.insert(id.to_owned(), (native.clone(), size));
        self.visiting.remove(id);
        Ok((native, size))
    }

    fn scene(
        &mut self,
        input: &Value,
        clock: Clock,
        size: [f32; 2],
        end: f64,
    ) -> Result<Scene, String> {
        let layers = input["layers"]
            .as_array()
            .ok_or("This file has no Lottie layers.")?;
        let length = clock.seconds(end);
        if !length.is_finite()
            || length <= 0.0
            || layers.len() > 500
            || size.iter().any(|side| !(100.0..=4000.0).contains(side))
        {
            return Err(
                "Lottie canvas, duration or layer count is outside Forma's supported range.".into(),
            );
        }
        let mut scene = Scene::empty(new_id());
        scene.project_name = name(input, "Lottie animation");
        [scene.width, scene.height] = size;
        scene.duration = length.max(1.0);
        scene.transparent_background = true;
        let mut indices = HashMap::new();
        let mut parents = Vec::new();
        for (ordinal, item) in layers.iter().enumerate().rev() {
            let label = name(item, "Layer");
            let ty = item["ty"].as_u64().ok_or("Missing Lottie layer type.")?;
            let index = item["ind"].as_i64().unwrap_or(ordinal as i64 + 1);
            if item["hd"].as_bool() == Some(true)
                || ["bm", "tt", "ddd", "ao"]
                    .iter()
                    .any(|key| item[*key].as_u64().unwrap_or(0) != 0)
                || item["ef"]
                    .as_array()
                    .is_some_and(|effects| !effects.is_empty())
            {
                return Err(format!(
                    "{label}: unsupported hidden layer, blending, matte, effect, 3D or auto-orient."
                ));
            }
            if ordinal == layers.len() - 1
                && ty == 1
                && label == "Canvas background"
                && background(item, size, clock, end)
            {
                scene.transparent_background = false;
                scene.bg = hex(&item["sc"])?;
                continue;
            }
            let composition = if ty == 0 {
                let source = item["refId"]
                    .as_str()
                    .ok_or("Missing Lottie composition ID.")?;
                Some(self.source(source, item, clock.fps)?)
            } else {
                None
            };
            let id = new_id();
            let size = composition.as_ref().map_or(BOX, |(_, size)| *size);
            let origin = if ty == 0 {
                size.map(|side| side / 2.0)
            } else {
                [0.0; 2]
            };
            if indices.insert(index, (id.clone(), size, origin)).is_some() {
                return Err("Lottie layer indices must be unique within a composition.".into());
            }
            let start = clock
                .seconds(item["ip"].as_f64().unwrap_or(clock.first))
                .max(0.0);
            let finish = clock
                .seconds(item["op"].as_f64().unwrap_or(end))
                .min(length);
            let duration = scene.duration;
            scene.add_shape(id.clone(), if ty == 0 { "composition" } else { "path" });
            let layer = scene
                .shapes
                .last_mut()
                .ok_or("Could not create a Lottie layer.")?;
            layer.name = label.clone();
            let result = (|| {
                if finish <= start {
                    return Err("Layer falls outside the composition.".into());
                }
                layer.in_time = (start > 0.0).then_some(start);
                layer.out_time = (finish < duration).then_some(finish);
                transforms(layer, &item["ks"], size, origin, clock, duration)?;
                match ty {
                    0 => {
                        let (source, _) = composition.ok_or("Missing composition source.")?;
                        layer.composition = Some(playback(source, item, clock, start)?);
                    }
                    1 => {
                        let dimensions = [number(&item["sw"])?, number(&item["sh"])?];
                        layer.color = hex(&item["sc"])?;
                        layer.vector = Some(VectorLayer {
                            path: Some(primitive(
                                "rc",
                                dimensions.map(|v| v / 2.0),
                                dimensions,
                                0.0,
                                size,
                            )?),
                            ..Default::default()
                        });
                    }
                    3 => null_layer(layer),
                    4 => vector(layer, &item["shapes"], size, clock, duration)?,
                    _ => return Err(format!("Unsupported Lottie layer type {ty}.")),
                }
                layer.masks = masks(&item["masksProperties"], size, origin, clock, duration)?;
                Ok::<_, String>(())
            })();
            result.map_err(|error| format!("{label}: {error}"))?;
            parents.push((id, item["parent"].as_i64()));
        }
        for (id, parent) in parents {
            let Some(parent) = parent else { continue };
            let (parent, [w, h], [x, y]) =
                indices.get(&parent).ok_or("Missing Lottie parent layer.")?;
            let from = scene
                .shapes
                .iter()
                .position(|layer| layer.id == id)
                .ok_or("Missing child layer.")?;
            let to = scene
                .shapes
                .iter()
                .position(|layer| layer.id == *parent)
                .ok_or("Missing parent layer.")?;
            if !scene.can_follow(from, to) {
                return Err(format!(
                    "{}: Lottie parenting forms a loop.",
                    scene.shapes[from].name
                ));
            }
            scene.shapes[from].parent = Some(parent.clone());
            scene.shapes[from].parent_space = Some([1.0 / w, 0.0, 0.0, 1.0 / h, -x / w, -y / h]);
        }
        Ok(scene)
    }
}

fn null_layer(layer: &mut Shape) {
    layer.kind = "rect".into();
    layer.extra.insert("lottieNull".into(), true.into());
    if let Some(pose) = &mut layer.rest_pose {
        pose.o = 0.0;
    }
    for key in &mut layer.keys {
        key.o = 0.0;
    }
}

fn background(item: &Value, [w, h]: [f32; 2], clock: Clock, end: f64) -> bool {
    item["sw"].as_f64() == Some(w as f64)
        && item["sh"].as_f64() == Some(h as f64)
        && item["ip"].as_f64() == Some(clock.first)
        && item["op"].as_f64() == Some(end)
        && item["parent"].is_null()
        && item["masksProperties"].is_null()
        && identity(&item["ks"])
}

fn identity(ks: &Value) -> bool {
    static_pair(&ks["a"], [0.0; 2]) == Ok([0.0; 2])
        && static_pair(&ks["p"], [0.0; 2]) == Ok([0.0; 2])
        && static_pair(&ks["s"], [100.0; 2]) == Ok([100.0; 2])
        && static_scalar(&ks["r"], 0.0) == Ok(0.0)
        && static_scalar(&ks["sk"], 0.0) == Ok(0.0)
        && static_scalar(&ks["o"], 100.0) == Ok(100.0)
}

fn name(item: &Value, fallback: &str) -> String {
    let mut length = 0;
    item["nm"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
        .chars()
        .take_while(|c| {
            length += c.len_utf16();
            length < 100
        })
        .collect()
}

fn transforms(
    layer: &mut Shape,
    ks: &Value,
    size: [f32; 2],
    origin: [f32; 2],
    clock: Clock,
    duration: f64,
) -> Result<(), String> {
    let anchor = static_pair(&ks["a"], [0.0; 2])?;
    if ks["p"]["s"].as_bool() == Some(true) || static_scalar(&ks["sk"], 0.0)? != 0.0 {
        return Err("Split position or skew cannot be represented as a Forma transform.".into());
    }
    layer.anchor = Some(Anchor {
        x: (anchor[0] - origin[0]) / size[0] + 0.5,
        y: (anchor[1] - origin[1]) / size[1] + 0.5,
    });
    let mut rest = Pose {
        x: 0.0,
        y: 0.0,
        w: size[0],
        h: size[1],
        r: 0.0,
        o: 100.0,
    };
    let mut keys = Vec::new();
    for (tag, property, default, count) in [
        ("p", Property::Position, [0.0; 2], 2),
        ("s", Property::Size, [100.0; 2], 2),
        ("r", Property::Rotation, [0.0; 2], 1),
        ("o", Property::Opacity, [100.0, 0.0], 1),
    ] {
        let item = if tag == "r" && ks[tag].is_null() {
            &ks["rz"]
        } else {
            &ks[tag]
        };
        let values = samples(item, clock)?;
        let convert = |value: &Value| -> Result<[f32; 2], String> {
            let mut v = if value.is_null() {
                default
            } else if count == 2 {
                pair(value)?
            } else {
                [scalar_value(value)?, 0.0]
            };
            if property == Property::Size {
                v = [v[0] * size[0] / 100.0, v[1] * size[1] / 100.0];
                if v.iter().any(|value| !(0.001..=10000.0).contains(value)) {
                    return Err("Negative, zero or very large scale is unsupported.".into());
                }
            }
            if v.iter().any(|v| v.abs() > 10000.0) {
                return Err("A Lottie transform exceeds Forma's property range.".into());
            }
            Ok(v)
        };
        let assign = |pose: &mut Pose, values: [f32; 2]| {
            if property == Property::Size {
                [pose.w, pose.h] = values;
            } else {
                property.set(pose, values);
            }
        };
        assign(
            &mut rest,
            convert(values.first().map_or(&item["k"], |sample| &sample.value))?,
        );
        for sample in values {
            let time = checked_time(sample.time, duration)?;
            let mut pose = rest;
            assign(&mut pose, convert(&sample.value)?);
            let mut key = Key::from_pose(time, pose);
            key.props = Some(vec![property]);
            key.ease = Some(sample.ease);
            key.curve = sample.curve;
            keys.push(key);
        }
    }
    layer.rest_pose = Some(rest);
    layer.keys = keys;
    Ok(())
}

fn playback(source: String, layer: &Value, clock: Clock, start: f64) -> Result<Playback, String> {
    let mut playback = Playback::new(source);
    let remap = &layer["tm"];
    if remap.is_null() {
        let stretch = layer["sr"].as_f64().unwrap_or(1.0);
        playback.speed = 1.0 / stretch;
        playback.offset = ((start - clock.seconds(layer["st"].as_f64().unwrap_or(clock.first)))
            * playback.speed)
            .max(0.0);
    } else {
        let values = samples(remap, clock)?;
        match values.as_slice() {
            [] => {
                playback.offset = f64::from(static_scalar(remap, 0.0)?);
                playback.speed = 0.0;
            }
            [first, last]
                if first.ease == "linear"
                    && (first.time - start).abs() < 0.001
                    && last.time > first.time =>
            {
                playback.offset = first.value[0].as_f64().ok_or("Missing remap start.")?;
                playback.speed = (last.value[0].as_f64().ok_or("Missing remap end.")?
                    - playback.offset)
                    / (last.time - first.time);
            }
            _ => return Err("Only linear or frozen composition remapping is supported.".into()),
        }
    }
    if !playback.offset.is_finite()
        || playback.offset < 0.0
        || !(0.0..=100.0).contains(&playback.speed)
    {
        return Err("Invalid Lottie composition playback speed or offset.".into());
    }
    Ok(playback)
}

fn vector(
    layer: &mut Shape,
    input: &Value,
    size: [f32; 2],
    clock: Clock,
    duration: f64,
) -> Result<(), String> {
    let mut items = input.as_array().ok_or("Missing Lottie vector shapes.")?;
    if items.len() == 1 && items[0]["ty"].as_str() == Some("gr") {
        items = items[0]["it"]
            .as_array()
            .ok_or("Invalid Lottie shape group.")?;
    }
    let mut path = PathData::default();
    let mut path_tracks = Vec::new();
    let mut vector = VectorLayer {
        fill: Some(PaintStyle::new(PaintMode::None)),
        ..Default::default()
    };
    let mut has_fill = false;
    for item in items {
        if item["hd"].as_bool() == Some(true) {
            continue;
        }
        match item["ty"].as_str() {
            Some("tr") if identity(item) => {}
            Some("sh") => {
                let keys = samples(&item["ks"], clock)?;
                let base = keys
                    .first()
                    .map_or(&item["ks"]["k"], |sample| &sample.value[0]);
                path.contours.push(read_curve(base, size, [0.0; 2])?);
                path_tracks.push(keys);
            }
            Some(kind @ ("rc" | "el")) => {
                let position = static_pair(&item["p"], [0.0; 2])?;
                let dimension = static_pair(&item["s"], [0.0; 2])?;
                let radius = static_scalar(&item["r"], 0.0)?;
                if item["d"].as_i64() == Some(3) {
                    return Err("Reversed primitive paths are unsupported.".into());
                }
                path.contours
                    .extend(primitive(kind, position, dimension, radius, size)?.contours);
                path_tracks.push(Vec::new());
            }
            Some("fl") if !has_fill => {
                if static_scalar(&item["o"], 100.0)? != 100.0 {
                    return Err("Independent fill opacity is unsupported.".into());
                }
                fill_rule(item)?;
                has_fill = true;
                vector.fill = Some(PaintStyle::new(PaintMode::Solid));
                let colors = samples(&item["c"], clock)?;
                layer.color = parse_color(
                    colors
                        .first()
                        .map_or(&item["c"]["k"], |sample| &sample.value),
                )?;
                for key in colors {
                    layer.color_keys.push(ColorKey {
                        t: checked_time(key.time, duration)?,
                        color: parse_color(&key.value)?,
                        ease: Some(key.ease),
                        curve: key.curve,
                        extra: Default::default(),
                    });
                }
            }
            Some("gf") if !has_fill => {
                fill_rule(item)?;
                has_fill = true;
                vector.fill = Some(gradient(
                    item,
                    size,
                    PaintTarget::Fill,
                    &mut vector.keys,
                    clock,
                    duration,
                )?);
            }
            Some(kind @ ("st" | "gs")) if vector.stroke.is_none() => {
                vector.stroke = Some(stroke(item, kind, size, &mut vector.keys, clock, duration)?);
            }
            Some("tm") if vector.trim.is_none() => {
                if item["m"].as_i64() == Some(2) {
                    return Err("Individually trimmed paths are unsupported.".into());
                }
                let mut scalar = |tag, default, scale, property| {
                    vector_scalar(
                        item,
                        tag,
                        default,
                        scale,
                        property,
                        &mut vector.keys,
                        clock,
                        duration,
                    )
                };
                vector.trim = Some(Trim {
                    start: scalar("s", 0.0, 1.0, VectorProperty::TrimStart)?,
                    end: scalar("e", 100.0, 1.0, VectorProperty::TrimEnd)?,
                    offset: scalar("o", 0.0, 1.0, VectorProperty::TrimOffset)?,
                    fill: false,
                });
            }
            other => {
                return Err(format!(
                    "Unsupported Lottie shape item {}.",
                    other.unwrap_or("without type")
                ));
            }
        }
    }
    if let Some(trim) = &mut vector.trim {
        trim.fill = has_fill && vector.stroke.is_none();
        if has_fill
            && vector.stroke.is_some()
            && (!trim.full()
                || vector.keys.iter().any(|key| {
                    matches!(
                        key.property,
                        VectorProperty::TrimStart | VectorProperty::TrimEnd
                    )
                }))
        {
            return Err("Combined trim on fill and stroke cannot be imported.".into());
        }
    }
    if vector.stroke.is_some()
        && layer.rest_pose.is_some_and(|rest| {
            (rest.w - size[0]).abs() > 0.0001
                || (rest.h - size[1]).abs() > 0.0001
                || layer
                    .keys
                    .iter()
                    .filter(|key| key.sets(Property::Size))
                    .any(|key| (key.w - size[0]).abs() > 0.0001 || (key.h - size[1]).abs() > 0.0001)
        })
    {
        return Err("Scaling a stroke on its own layer cannot be represented in Forma.".into());
    }
    if path.contours.is_empty() {
        null_layer(layer);
        return Ok(());
    }
    if let Some(reference) = path_tracks.iter().find(|track| !track.is_empty()) {
        if path_tracks
            .iter()
            .filter(|track| !track.is_empty())
            .any(|track| !same_track(reference, track))
        {
            return Err("Path contours use different animation times or easing.".into());
        }
        for (index, sample) in reference.iter().enumerate() {
            let mut keyed_path = path.clone();
            for (contour, track) in keyed_path.contours.iter_mut().zip(&path_tracks) {
                if let Some(sample) = track.get(index) {
                    *contour = read_curve(&sample.value[0], size, [0.0; 2])?;
                }
            }
            vector.keys.push(vector_key(
                sample,
                duration,
                VectorProperty::Path,
                VectorValue::Path(keyed_path),
            )?);
        }
    }
    if !path.valid() {
        return Err("Invalid or too many Lottie path points.".into());
    }
    vector.path = Some(path);
    layer.vector = Some(vector);
    Ok(())
}

fn fill_rule(item: &Value) -> Result<(), String> {
    if item["r"].as_u64().unwrap_or(1) != 1 {
        Err("Even-odd filling is unsupported.".into())
    } else {
        Ok(())
    }
}

fn primitive(
    kind: &str,
    [x, y]: [f32; 2],
    [w, h]: [f32; 2],
    radius: f32,
    size: [f32; 2],
) -> Result<PathData, String> {
    let mut builder = PathBuilder::new();
    let rect = Rect::from_xywh(-w / 2.0, -h / 2.0, w, h).ok_or("Invalid Lottie primitive size.")?;
    if kind == "el" {
        builder.push_oval(rect);
    } else if radius == 0.0 {
        builder.push_rect(rect);
    } else {
        let r = radius.clamp(0.0, w.min(h) / 2.0);
        let (l, t, b, rgt) = (rect.left(), rect.top(), rect.bottom(), rect.right());
        let c = r * (1.0 - 0.552_284_8);
        builder.move_to(l + r, t);
        builder.line_to(rgt - r, t);
        builder.cubic_to(rgt - c, t, rgt, t + c, rgt, t + r);
        builder.line_to(rgt, b - r);
        builder.cubic_to(rgt, b - c, rgt - c, b, rgt - r, b);
        builder.line_to(l + r, b);
        builder.cubic_to(l + c, b, l, b - c, l, b - r);
        builder.line_to(l, t + r);
        builder.cubic_to(l, t + c, l + c, t, l + r, t);
        builder.close();
    }
    builder
        .finish()
        .and_then(|path| path.transform(Transform::from_translate(x, y)))
        .map(|path| PathData::from_outline(&path, size))
        .ok_or_else(|| "Invalid Lottie primitive.".into())
}

fn gradient(
    item: &Value,
    [w, h]: [f32; 2],
    target: PaintTarget,
    keys: &mut Vec<VectorKey>,
    clock: Clock,
    duration: f64,
) -> Result<PaintStyle, String> {
    if static_scalar(&item["o"], 100.0)? != 100.0 || static_scalar(&item["h"], 0.0)? != 0.0 {
        return Err(
            "Independent gradient opacity or a displaced radial focus is unsupported.".into(),
        );
    }
    let mode = match item["t"].as_u64() {
        Some(1) => PaintMode::Linear,
        Some(2) => PaintMode::Radial,
        _ => return Err("Unknown Lottie gradient type.".into()),
    };
    let count = item["g"]["p"]
        .as_u64()
        .filter(|n| (2..=32).contains(n))
        .ok_or("Invalid Lottie gradient stop count.")? as usize;
    let data = static_value(&item["g"]["k"])?
        .as_array()
        .filter(|data| data.len() >= count * 4)
        .ok_or("Invalid Lottie gradient stops.")?;
    let data = data.iter().map(number).collect::<Result<Vec<_>, _>>()?;
    let (rgb, opacity) = data.split_at(count * 4);
    if opacity.len() % 2 != 0 || opacity.len() > 64 {
        return Err("Invalid gradient opacity stops.".into());
    }
    let mut positions: Vec<_> = rgb
        .chunks_exact(4)
        .map(|stop| stop[0])
        .chain(opacity.chunks_exact(2).map(|stop| stop[0]))
        .collect();
    positions.sort_by(f32::total_cmp);
    positions.dedup();
    let stops = positions
        .into_iter()
        .map(|at| {
            let color = |i| interpolate_stop(rgb, 4, i, at, 0.0) * 255.0;
            Stop {
                offset: at.clamp(0.0, 1.0) * 100.0,
                rgba: [
                    color(1),
                    color(2),
                    color(3),
                    interpolate_stop(opacity, 2, 1, at, 1.0) * 100.0,
                ],
            }
        })
        .collect();
    let start_keys = samples(&item["s"], clock)?;
    let end_keys = samples(&item["e"], clock)?;
    let start = pair(start_keys.first().map_or(&item["s"]["k"], |key| &key.value))?;
    let end = pair(end_keys.first().map_or(&item["e"]["k"], |key| &key.value))?;
    for (key, first, last) in paired(&start_keys, &end_keys, "gradient endpoints")? {
        let [x, y] = first.map_or(Ok(start), |key| pair(&key.value))?;
        let [u, v] = last.map_or(Ok(end), |key| pair(&key.value))?;
        keys.push(vector_key(
            key,
            duration,
            VectorProperty::GradientEnds(target),
            VectorValue::Numbers([x / w, y / h, u / w, v / h, 0.0]),
        )?);
    }
    Ok(PaintStyle {
        mode,
        gradient: Some(Gradient {
            ends: [start[0] / w, start[1] / h, end[0] / w, end[1] / h],
            stops,
        }),
    })
}

fn interpolate_stop(data: &[f32], width: usize, channel: usize, at: f32, fallback: f32) -> f32 {
    let stops = data.chunks_exact(width);
    let Some(first) = stops.clone().next() else {
        return fallback;
    };
    let before = stops
        .clone()
        .take_while(|stop| stop[0] <= at)
        .last()
        .unwrap_or(first);
    let after = stops.clone().find(|stop| stop[0] >= at).unwrap_or(before);
    let f = if after[0] > before[0] {
        (at - before[0]) / (after[0] - before[0])
    } else {
        0.0
    };
    (before[channel] + (after[channel] - before[channel]) * f).clamp(0.0, 1.0)
}

fn stroke(
    item: &Value,
    kind: &str,
    size: [f32; 2],
    keys: &mut Vec<VectorKey>,
    clock: Clock,
    duration: f64,
) -> Result<StrokeStyle, String> {
    let paint = if kind == "gs" {
        gradient(item, size, PaintTarget::Stroke, keys, clock, duration)?
    } else {
        PaintStyle::new(PaintMode::Solid)
    };
    let mut rgba = [255.0, 255.0, 255.0, 100.0];
    if kind == "st" {
        let color = samples(&item["c"], clock)?;
        let opacity = samples(&item["o"], clock)?;
        let rgb = |value: &Value| -> Result<[f32; 3], String> {
            Ok([number(&value[0])?, number(&value[1])?, number(&value[2])?]
                .map(|n| n.clamp(0.0, 1.0) * 255.0))
        };
        let base = rgb(color.first().map_or(&item["c"]["k"], |key| &key.value))?;
        let alpha = opacity.first().map_or_else(
            || static_scalar(&item["o"], 100.0),
            |key| scalar_value(&key.value),
        )?;
        rgba = [base[0], base[1], base[2], alpha];
        for (key, c, a) in paired(&color, &opacity, "stroke color and opacity")? {
            let [r, g, b] = c.map_or(Ok(base), |key| rgb(&key.value))?;
            let a = a.map_or(Ok(alpha), |key| scalar_value(&key.value))?;
            keys.push(vector_key(
                key,
                duration,
                VectorProperty::StrokeColor,
                VectorValue::Numbers([r, g, b, a, 0.0]),
            )?);
        }
    }
    let width = vector_scalar(
        item,
        "w",
        1.0,
        1.0,
        VectorProperty::StrokeWidth,
        keys,
        clock,
        duration,
    )?;
    let cap = match item["lc"].as_u64() {
        Some(2) => Cap::Round,
        Some(3) => Cap::Square,
        _ => Cap::Butt,
    };
    let join = match item["lj"].as_u64() {
        Some(2) => Join::Round,
        Some(3) => Join::Bevel,
        _ => Join::Miter,
    };
    let dash = if let Some(dashes) = item["d"].as_array().filter(|items| !items.is_empty()) {
        let mut dash = Dash::default();
        let mut seen = HashSet::new();
        for item in dashes {
            let name = item["n"].as_str().unwrap_or_default();
            if !seen.insert(name) {
                return Err("Only a single dash and gap are supported.".into());
            }
            match name {
                "d" => dash.length = static_scalar(&item["v"], dash.length)?,
                "g" => dash.gap = static_scalar(&item["v"], dash.gap)?,
                "o" => {
                    dash.offset = vector_scalar(
                        item,
                        "v",
                        0.0,
                        1.0,
                        VectorProperty::DashOffset,
                        keys,
                        clock,
                        duration,
                    )?
                }
                _ => return Err("Unsupported Lottie dash value.".into()),
            }
        }
        Some(dash)
    } else {
        None
    };
    Ok(StrokeStyle {
        paint,
        rgba,
        width,
        cap,
        join,
        dash,
    })
}

#[allow(clippy::too_many_arguments)]
fn vector_scalar(
    item: &Value,
    tag: &str,
    default: f32,
    scale: f32,
    property: VectorProperty,
    keys: &mut Vec<VectorKey>,
    clock: Clock,
    duration: f64,
) -> Result<f32, String> {
    let samples = samples(&item[tag], clock)?;
    let first = samples.first().map_or_else(
        || static_scalar(&item[tag], default),
        |sample| scalar_value(&sample.value),
    )?;
    for sample in samples {
        let value = scalar_value(&sample.value)? * scale;
        keys.push(vector_key(
            &sample,
            duration,
            property,
            VectorValue::Numbers([value, 0.0, 0.0, 0.0, 0.0]),
        )?);
    }
    Ok(first * scale)
}

fn vector_key(
    sample: &Sample,
    duration: f64,
    property: VectorProperty,
    value: VectorValue,
) -> Result<VectorKey, String> {
    Ok(VectorKey {
        t: checked_time(sample.time, duration)?,
        property,
        value,
        ease: Some(sample.ease.clone()),
        curve: sample.curve,
    })
}

fn masks(
    input: &Value,
    size: [f32; 2],
    origin: [f32; 2],
    clock: Clock,
    duration: f64,
) -> Result<Vec<LayerMask>, String> {
    input
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["mode"].as_str() != Some("n"))
        .enumerate()
        .map(|(index, item)| {
            let mode = match item["mode"].as_str() {
                Some("a") => MaskMode::Add,
                Some("s") => MaskMode::Subtract,
                Some("i") => MaskMode::Intersect,
                Some("f") => MaskMode::Difference,
                _ => return Err("Unsupported Lottie mask mode.".into()),
            };
            if static_pair(&item["f"], [0.0; 2])? != [0.0; 2] {
                return Err("Lottie mask feather is unsupported.".into());
            }
            let contour = read_curve(static_value(&item["pt"])?, size, origin)?;
            if !contour.closed || contour.points.len() < 3 {
                return Err("Lottie masks must be closed.".into());
            }
            let points = contour
                .points
                .into_iter()
                .map(|p| MaskPoint {
                    point: p.point,
                    incoming: p.incoming,
                    outgoing: p.outgoing,
                })
                .collect();
            let mut rest = MaskPose::new([0.0; 2], size);
            rest.expansion = static_scalar(&item["x"], 0.0)?;
            let opacity = samples(&item["o"], clock)?;
            rest.opacity = opacity.first().map_or_else(
                || static_scalar(&item["o"], 100.0),
                |sample| scalar_value(&sample.value),
            )?;
            let keys = opacity
                .into_iter()
                .map(|sample| {
                    Ok(MaskKey {
                        t: checked_time(sample.time, duration)?,
                        property: MaskProperty::Opacity,
                        values: [scalar_value(&sample.value)?, 0.0],
                        ease: Some(sample.ease),
                        curve: sample.curve,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(LayerMask {
                id: index as u64 + 1,
                shape: MaskShape::Path { points },
                mode,
                inverted: item["inv"].as_bool().unwrap_or(false),
                reference_size: size,
                rest,
                keys,
            })
        })
        .collect()
}

fn static_value(item: &Value) -> Result<&Value, String> {
    if item["a"].as_u64() == Some(1) || expressions(item) {
        return Err("Animation of this Lottie property is unsupported.".into());
    }
    Ok(&item["k"])
}

fn static_pair(item: &Value, fallback: [f32; 2]) -> Result<[f32; 2], String> {
    if item.is_null() {
        Ok(fallback)
    } else {
        pair(static_value(item)?)
    }
}

fn pair(value: &Value) -> Result<[f32; 2], String> {
    Ok([number(&value[0])?, number(&value[1])?])
}

fn static_scalar(item: &Value, fallback: f32) -> Result<f32, String> {
    if item.is_null() {
        Ok(fallback)
    } else {
        scalar_value(static_value(item)?)
    }
}

fn scalar_value(value: &Value) -> Result<f32, String> {
    if value.is_array() {
        number(&value[0])
    } else {
        number(value)
    }
}

fn expressions(item: &Value) -> bool {
    item["x"].as_str().is_some()
}

fn hex(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .filter(|color| {
            color.len() == 7
                && color.starts_with('#')
                && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        })
        .map(str::to_owned)
        .ok_or_else(|| "Invalid Lottie solid color.".into())
}

fn parse_color(value: &Value) -> Result<String, String> {
    let byte = |index| number(&value[index]).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let [r, g, b] = [byte(0usize)?, byte(1)?, byte(2)?];
    Ok(format!("#{r:02x}{g:02x}{b:02x}"))
}

fn checked_time(time: f64, duration: f64) -> Result<f64, String> {
    if !time.is_finite() || time < -0.00001 || time > duration + 0.00001 {
        return Err("A Lottie key is outside the composition.".into());
    }
    Ok(time.clamp(0.0, duration))
}

fn samples(item: &Value, clock: Clock) -> Result<Vec<Sample>, String> {
    if expressions(item) {
        return Err("Lottie expressions are unsupported.".into());
    }
    if item["a"].as_u64() != Some(1) {
        return Ok(Vec::new());
    }
    let keys = item["k"]
        .as_array()
        .filter(|keys| !keys.is_empty() && keys.len() <= 1000)
        .ok_or("Invalid Lottie keyframe list.")?;
    let mut result = Vec::new();
    let mut last_time = f64::NEG_INFINITY;
    for (index, key) in keys.iter().enumerate() {
        let time = clock.seconds(key["t"].as_f64().ok_or("Missing Lottie key time.")?);
        if !time.is_finite() || time <= last_time {
            return Err("Lottie key times must be increasing.".into());
        }
        last_time = time;
        if ["to", "ti"].into_iter().any(|tag| {
            key[tag]
                .as_array()
                .is_some_and(|v| v.iter().any(|n| n.as_f64() != Some(0.0)))
        }) {
            return Err("Spatial Bezier position keys are unsupported.".into());
        }
        let value = if !key["s"].is_null() {
            key["s"].clone()
        } else {
            index
                .checked_sub(1)
                .and_then(|i| keys.get(i))
                .map(|key| key["e"].clone())
                .filter(|value| !value.is_null())
                .ok_or("Missing Lottie key value.")?
        };
        let (ease, curve) = easing(key)?;
        result.push(Sample {
            time,
            value,
            ease: ease.into(),
            curve,
        });
    }
    Ok(result)
}

fn easing(key: &Value) -> Result<(&'static str, Option<[f32; 4]>), String> {
    if key["h"].as_u64() == Some(1) {
        return Ok(("hold", None));
    }
    if key["o"].is_null() || key["i"].is_null() {
        return Ok(("linear", None));
    }
    let value = |group: &str, axis: &str| {
        let entry = &key[group][axis];
        if entry
            .as_array()
            .is_some_and(|items| items.windows(2).any(|pair| pair[0] != pair[1]))
        {
            return Err("Different easing per axis is unsupported.".into());
        }
        scalar_value(entry)
    };
    let curve = [
        value("o", "x")?,
        value("o", "y")?,
        value("i", "x")?,
        value("i", "y")?,
    ];
    if (curve[0] - curve[1]).abs() < 0.00001 && (curve[2] - curve[3]).abs() < 0.00001 {
        return Ok(("linear", None));
    }
    if [curve[0], curve[2]]
        .iter()
        .any(|v| !(0.0..=1.0).contains(v))
        || [curve[1], curve[3]]
            .iter()
            .any(|v| !(-5.0..=5.0).contains(v))
    {
        return Err("A Lottie easing curve is outside Forma's supported range.".into());
    }
    Ok(("custom", Some(curve)))
}

fn same_track(a: &[Sample], b: &[Sample]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a.time == b.time && a.ease == b.ease && a.curve == b.curve)
}

type Paired<'a> = (&'a Sample, Option<&'a Sample>, Option<&'a Sample>);

fn paired<'a>(a: &'a [Sample], b: &'a [Sample], label: &str) -> Result<Vec<Paired<'a>>, String> {
    if a.is_empty() {
        return Ok(b.iter().map(|b| (b, None, Some(b))).collect());
    }
    if b.is_empty() {
        return Ok(a.iter().map(|a| (a, Some(a), None)).collect());
    }
    if !same_track(a, b) {
        return Err(format!(
            "Lottie {label} use different animation timing or easing."
        ));
    }
    Ok(a.iter()
        .zip(b)
        .map(|(a, b)| (a, Some(a), Some(b)))
        .collect())
}
