use super::*;
use crate::{
    compositing::{Blend, MaskMode},
    keyframes::Property,
    model::{self, Scene, Shape},
    paths::PathData,
    shape_style::{
        Cap, Gradient, Join, PaintMode, PaintTarget, VectorLayer, VectorProperty, VectorValue,
    },
};
use std::collections::{HashMap, HashSet};
use tiny_skia::Transform;

pub fn export(scene: &Scene) -> Result<Vec<u8>, String> {
    let errors = unsupported(scene);
    if !errors.is_empty() {
        return Err(format!(
            "Lottie export is unavailable for these layers:\n{}",
            errors.join("\n")
        ));
    }
    let all = reachable(scene);
    let layers = layers(scene, scene)?;
    let assets = all
        .into_iter()
        .skip(1)
        .map(|source| {
            Ok(json!({"id": source.project_id, "nm": source.project_name,
            "w": source.width.round(), "h": source.height.round(), "op": source.duration * FPS,
            "layers": self::layers(source, scene)?}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    serde_json::to_vec_pretty(&json!({
        "v": "5.12.2", "fr": FPS, "ip": 0, "op": scene.duration * FPS,
        "w": scene.width.round(), "h": scene.height.round(), "nm": scene.project_name,
        "ddd": 0, "assets": assets, "layers": layers,
    }))
    .map_err(|e| e.to_string())
}

fn reachable(scene: &Scene) -> Vec<&Scene> {
    let mut result = vec![scene];
    let mut seen = HashSet::from([scene.project_id.as_str()]);
    let mut index = 0;
    while let Some(item) = result.get(index) {
        for layer in &item.shapes {
            if let Some(source) = layer
                .composition
                .as_ref()
                .and_then(|playback| scene.composition(&playback.source))
                && seen.insert(&source.project_id)
            {
                result.push(source);
            }
        }
        index += 1;
    }
    result
}

pub fn unsupported(scene: &Scene) -> Vec<String> {
    if let Err(error) = scene.validate_compositions() {
        return vec![error];
    }
    let mut errors = Vec::new();
    for source in reachable(scene) {
        for shape in &source.shapes {
            let mut features = Vec::new();
            if !matches!(
                shape.kind.as_str(),
                "composition"
                    | "text"
                    | "circle"
                    | "rect"
                    | "triangle"
                    | "diamond"
                    | "arrow"
                    | "path"
            ) {
                features.push(shape.kind.as_str());
            }
            if shape.effects.iter().any(|effect| effect.enabled) {
                features.push("effects");
            }
            if shape.motion_blur && source.motion_blur.is_some() {
                features.push("motion blur");
            }
            if crate::render::blur(shape) > 0.0 {
                features.push("layer blur");
            }
            if shape.blend() != Blend::Normal {
                features.push("blend mode");
            }
            if shape.track_matte.is_some() {
                features.push("track matte");
            }
            if model::motions(shape).next().is_some() {
                features.push("motion clips");
            }
            if !shape.rotation_clips.is_empty() {
                features.push("rotation clips");
            }
            if shape.disappear.is_some() {
                features.push("disappear animation");
            }
            if model::is_linked(shape) {
                features.push("linked text");
            }
            if shape.text_layout.as_ref().is_some_and(|layout| {
                !layout.animators.is_empty()
                    || layout.path.is_some()
                    || layout.style.values().any(|track| !track.keys.is_empty())
            }) {
                features.push("text animators, animated typesetting or text on a path");
            }
            if shape.kind == "text" && shape.vector.is_some() {
                features.push("custom text fill or stroke");
            }
            if shape
                .composition
                .as_ref()
                .is_some_and(|playback| playback.looping)
            {
                features.push("looping source time");
            }
            if shape.vector.as_ref().is_some_and(|vector| {
                vector
                    .keys
                    .iter()
                    .any(|key| matches!(key.property, VectorProperty::GradientStop(..)))
                    || vector.trim.is_some_and(|trim| {
                        vector.stroke.is_some()
                            && vector
                                .fill
                                .as_ref()
                                .is_none_or(|fill| fill.mode != PaintMode::None)
                            && (!trim.full()
                                || vector.has_keys(VectorProperty::TrimStart)
                                || vector.has_keys(VectorProperty::TrimEnd))
                    })
            }) {
                features.push("animated gradient stops or differently trimmed fill and stroke");
            }
            if shape.vector.as_ref().is_some_and(|vector| {
                vector.has_keys(VectorProperty::Path) && !matching_contours(vector)
            }) {
                features.push("path keys with differing contours");
            }
            if shape.vector.as_ref().is_some_and(|vector| {
                let reference = shape.pose(0.0);
                shape
                    .keys
                    .iter()
                    .filter(|key| key.sets(Property::Size))
                    .any(|key| {
                        let (x, y) = (key.w / reference.w, key.h / reference.h);
                        let stroke = vector
                            .stroke
                            .as_ref()
                            .is_some_and(|stroke| stroke.paint.mode != PaintMode::None);
                        let gradient = vector
                            .fill
                            .as_ref()
                            .is_some_and(|paint| paint.mode.gradient());
                        stroke && ((x - 1.0).abs() > 0.0001 || (y - 1.0).abs() > 0.0001)
                            || gradient && (x - y).abs() > 0.0001
                    })
            }) {
                features.push("animated size with a fixed-width stroke or a stretching gradient");
            }
            if shape.masks.iter().any(|mask| {
                mask.rest.feather != 0.0
                    || mask
                        .keys
                        .iter()
                        .any(|key| key.property != crate::compositing::MaskProperty::Opacity)
            }) {
                features.push("feathered or animated mask geometry");
            }
            if shape.parent.is_some() || shape.parent_space.is_some() {
                match parent_basis(source, scene, shape) {
                    Some((_, matrix)) if split_basis(matrix).is_some() => {}
                    _ => features.push("missing parent or sheared parent space"),
                }
            }
            if !features.is_empty() {
                errors.push(format!(
                    "{} / {} — {}",
                    source.project_name,
                    shape.name,
                    features.join(", ")
                ));
            }
        }
    }
    errors
}

fn matching_contours(vector: &VectorLayer) -> bool {
    let mut paths = vector.keys.iter().filter_map(|key| match &key.value {
        VectorValue::Path(path) => Some(path),
        _ => None,
    });
    let topology = |path: &PathData| {
        path.contours
            .iter()
            .map(|c| (c.closed, c.points.len()))
            .collect::<Vec<_>>()
    };
    let Some(first) = paths.clone().next().map(topology) else {
        return true;
    };
    paths.all(|path| topology(path) == first)
}

fn size(scene: &Scene, shape: &Shape) -> [f32; 2] {
    if let Some(source) = shape
        .composition
        .as_ref()
        .and_then(|playback| scene.composition(&playback.source))
    {
        return [source.width, source.height];
    }
    let pose = shape.pose(0.0);
    [pose.w.max(0.001), pose.h.max(0.001)]
}

fn center(project: &Scene, shape: &Shape) -> [f32; 2] {
    if shape.composition.is_some() {
        size(project, shape).map(|side| side / 2.0)
    } else {
        [0.0; 2]
    }
}

fn parent_basis<'a>(
    scene: &'a Scene,
    project: &Scene,
    shape: &Shape,
) -> Option<(Option<&'a Shape>, Transform)> {
    let mut basis = shape
        .parent_space
        .map_or_else(Transform::identity, |[a, b, c, d, e, f]| {
            Transform::from_row(a, b, c, d, e, f)
        });
    let parent = match shape.parent.as_deref() {
        Some(id) => {
            let p = scene.shapes.iter().find(|layer| layer.id == id)?;
            let [w, h] = size(project, p);
            let [x, y] = center(project, p);
            basis = basis.post_scale(w, h).post_translate(x, y);
            Some(p)
        }
        None => None,
    };
    Some((parent, basis))
}

fn split_basis(matrix: Transform) -> Option<([f32; 2], f32, [f32; 2])> {
    let sx = matrix.sx.hypot(matrix.ky);
    if !sx.is_finite() || sx <= 0.000001 {
        return None;
    }
    let sy = (matrix.sx * matrix.sy - matrix.ky * matrix.kx) / sx;
    let skew = (matrix.sx * matrix.kx + matrix.ky * matrix.sy) / sx;
    if !sy.is_finite() || sy <= 0.000001 || skew.abs() > sy * 0.0001 {
        return None;
    }
    Some((
        [sx * 100.0, sy * 100.0],
        matrix.ky.atan2(matrix.sx).to_degrees(),
        [matrix.tx, matrix.ty],
    ))
}

fn layers(scene: &Scene, project: &Scene) -> Result<Vec<Value>, String> {
    let mut layers = Vec::new();
    let indices: HashMap<_, _> = scene
        .shapes
        .iter()
        .enumerate()
        .map(|(i, shape)| (shape.id.as_str(), i + 1))
        .collect();
    let mut helper = scene.shapes.len() + 1;
    for shape in scene.shapes.iter().rev() {
        let index = indices
            .get(shape.id.as_str())
            .copied()
            .ok_or("Missing layer index")?;
        let mut layer = layer(scene, project, shape, index)?;
        if let Some((parent, basis)) = parent_basis(scene, project, shape) {
            let parent = parent.and_then(|parent| indices.get(parent.id.as_str()));
            if basis.is_identity() {
                if let Some(parent) = parent {
                    layer["parent"] = (*parent).into();
                }
            } else {
                let (scale, angle, position) =
                    split_basis(basis).ok_or("Unsupported parent transform")?;
                let mut basis = json!({"ty": 3, "ind": helper, "nm": format!("{} attachment", shape.name),
                    "ip": 0, "op": scene.duration * FPS, "st": 0, "sr": 1,
                    "ks": {"a": constant(json!([0, 0])), "p": constant(json!(position)),
                        "s": constant(json!(scale)), "r": constant(angle), "o": constant(100)}});
                if let Some(parent) = parent {
                    basis["parent"] = (*parent).into();
                }
                layer["parent"] = helper.into();
                helper += 1;
                layers.push(basis);
            }
        }
        layers.push(layer);
    }
    if !scene.transparent_background {
        layers.push(json!({"ty": 1, "ind": helper, "nm": "Canvas background",
            "sw": scene.width.round(), "sh": scene.height.round(), "sc": scene.bg,
            "ip": 0, "op": scene.duration * FPS, "st": 0, "sr": 1, "ks": identity()}));
    }
    Ok(layers)
}

fn identity() -> Value {
    json!({"a": constant(json!([0, 0])), "p": constant(json!([0, 0])),
        "s": constant(json!([100, 100])), "r": constant(0), "o": constant(100)})
}

fn layer(scene: &Scene, project: &Scene, shape: &Shape, index: usize) -> Result<Value, String> {
    let bounds = size(project, shape);
    let origin = center(project, shape);
    let anchor = shape.anchor.unwrap_or_default();
    let parameter = |property, count: usize, ratio: [f32; 2]| {
        let values = |time| {
            let values = property_values(property, scene, shape, time, ratio);
            if count == 1 {
                json!(values[0])
            } else {
                json!(values)
            }
        };
        let keys: Vec<_> = shape
            .keys
            .iter()
            .filter(|k| k.sets(property) && !k.linked_space)
            .map(|key| {
                let value = values(key.t);
                animated_key(
                    key.t,
                    if count == 1 { json!([value]) } else { value },
                    key.ease.as_deref().unwrap_or(&shape.ease),
                    key.curve,
                )
            })
            .collect();
        if keys.len() <= 1 {
            constant(values(0.0))
        } else {
            json!({"a": 1, "k": keys})
        }
    };
    let start = shape.clip_start();
    let end = shape.out_time.unwrap_or(scene.duration);
    let mut layer = json!({
        "ty": 4, "ind": index, "nm": shape.name, "ip": start * FPS, "op": end * FPS,
        "st": 0, "sr": 1, "ddd": 0,
        "ks": {
            "a": constant(json!([(anchor.x - 0.5) * bounds[0] + origin[0], (anchor.y - 0.5) * bounds[1] + origin[1]])),
            "p": parameter(Property::Position, 2, [1.0; 2]),
            "s": parameter(Property::Size, 2, [100.0 / bounds[0], 100.0 / bounds[1]]),
            "r": parameter(Property::Rotation, 1, [1.0; 2]),
            "o": parameter(Property::Opacity, 1, [1.0; 2]),
        }
    });
    if let Some(playback) = &shape.composition {
        let source = project
            .composition(&playback.source)
            .ok_or("Missing composition")?;
        layer["ty"] = 0.into();
        layer["refId"] = playback.source.clone().into();
        layer["w"] = bounds[0].round().into();
        layer["h"] = bounds[1].round().into();
        let cap = (source.duration - 0.000001).max(0.0);
        let until = if playback.speed == 0.0 {
            start
        } else {
            (start + (cap - playback.offset).max(0.0) / playback.speed).clamp(start, end)
        };
        layer["tm"] = if until <= start {
            constant(playback.offset.min(cap))
        } else {
            json!({"a": 1, "k": [
                animated_key(start, json!([playback.offset.min(cap)]), "linear", None),
                animated_key(until, json!([(playback.offset + (until - start) * playback.speed).min(cap)]), "hold", None)
            ]})
        };
    } else if shape.extra.get("lottieNull").and_then(Value::as_bool) == Some(true) {
        layer["ty"] = 3.into();
    } else {
        layer["shapes"] = json!(geometry(scene, shape, bounds)?);
    }
    if !shape.masks.is_empty() {
        let masks = shape.masks.iter().map(|mask| {
            let rest = mask.rest;
            let mode = match mask.mode { MaskMode::Add => "a", MaskMode::Subtract => "s", MaskMode::Intersect => "i", MaskMode::Difference => "f" };
            let [x, y] = origin;
            let [sx, sy] = [bounds[0] / mask.reference_size[0], bounds[1] / mask.reference_size[1]];
            if rest.expansion != 0.0 && (sx - sy).abs() > 0.0001 {
                return Err(format!("{}: Lottie cannot export a mask expansion under uneven scaling.", shape.name));
            }
            let outline = mask.shape.outline(rest.size).and_then(|path| path.transform(rest.placement().post_scale(sx, sy).post_translate(x, y)))
                .ok_or_else(|| format!("{}: invalid mask outline.", shape.name))?;
            let path = PathData::from_outline(&outline, [1.0; 2]);
            let contour = path.contours.first().ok_or("Empty mask outline")?;
            let opacity = mask.keys.iter().filter(|key| key.property == crate::compositing::MaskProperty::Opacity)
                .map(|key| animated_key(key.t, json!([key.values[0]]), key.ease.as_deref().unwrap_or(&shape.ease), key.curve))
                .collect::<Vec<_>>();
            Ok(json!({ "mode": mode, "inv": mask.inverted, "pt": constant(curve(contour, [1.0; 2], [0.0; 2])),
                "o": if opacity.is_empty() { constant(rest.opacity) } else { json!({"a":1,"k":opacity}) },
                "x": constant(rest.expansion * sx) }))
        }).collect::<Result<Vec<_>, String>>()?;
        layer["hasMask"] = true.into();
        layer["masksProperties"] = masks.into();
    }
    Ok(layer)
}

fn property_values(
    property: Property,
    scene: &Scene,
    shape: &Shape,
    time: f64,
    ratio: [f32; 2],
) -> [f32; 2] {
    let [a, b] = property.values(scene.local_pose(shape, time));
    [a * ratio[0], b * ratio[1]]
}

fn geometry(scene: &Scene, shape: &Shape, size: [f32; 2]) -> Result<Vec<Value>, String> {
    let [w, h] = size;
    let vector = shape.vector.as_ref();
    let native_paths =
        if shape.kind == "path" && vector.is_some_and(|v| v.source_shape.is_none()) {
            vector.and_then(|vector| vector.path.clone())
        } else if shape.kind == "text" {
            Some(outlined_text(scene, shape, size))
        } else {
            let kind = vector
                .and_then(|v| v.source_shape.as_deref())
                .unwrap_or(&shape.kind);
            crate::render::shape_outline(kind, w / 2.0, h / 2.0)
                .map(|path| PathData::from_outline(&path, size))
        }
        .ok_or_else(|| format!("{}: Lottie cannot export this outline.", shape.name))?;
    let path_keys: Vec<_> = vector
        .into_iter()
        .flat_map(|v| &v.keys)
        .filter_map(|key| match &key.value {
            VectorValue::Path(path) => Some((key, path)),
            _ => None,
        })
        .collect();
    let contours = path_keys
        .first()
        .map_or(&native_paths.contours, |(_, path)| &path.contours);
    let mut shapes = Vec::new();
    for (index, contour) in contours.iter().enumerate() {
        let ks = if path_keys.is_empty() {
            constant(curve(contour, size, [0.0; 2]))
        } else {
            json!({"a": 1, "k": path_keys.iter().filter_map(|(key, path)| {
                Some(animated_key(key.t, json!([curve(path.contours.get(index)?, size, [0.0; 2])]),
                    key.ease.as_deref().unwrap_or(&shape.ease), key.curve))
            }).collect::<Vec<_>>()})
        };
        shapes.push(json!({"ty": "sh", "ks": ks, "nm": shape.name}));
    }
    let fill = vector.and_then(|v| v.fill.as_ref());
    if fill.is_none_or(|style| style.mode == PaintMode::Solid) {
        let to_color = |color: &str| crate::render::rgb(color).map(|value| value as f32 / 255.0);
        let keys: Vec<_> = shape
            .color_keys
            .iter()
            .map(|key| {
                animated_key(
                    key.t,
                    json!(to_color(&key.color)),
                    key.ease.as_deref().unwrap_or(&shape.ease),
                    key.curve,
                )
            })
            .collect();
        shapes.push(json!({"ty": "fl", "r": 1, "o": constant(100),
            "c": if keys.is_empty() { constant(json!(to_color(&shape.color))) } else { json!({"a":1,"k":keys}) }}));
    } else if let Some(gradient) = fill.and_then(|fill| fill.gradient.as_ref())
        && let Some(vector) = vector
    {
        shapes.push(gradient_shape(
            vector,
            gradient,
            fill.map_or(PaintMode::Linear, |fill| fill.mode),
            PaintTarget::Fill,
            size,
            &shape.ease,
        ));
    }
    if let Some(vector) = vector {
        if let Some(stroke) = vector
            .stroke
            .as_ref()
            .filter(|stroke| stroke.paint.mode != PaintMode::None)
        {
            let mut item = if let Some(gradient) = &stroke.paint.gradient {
                gradient_shape(
                    vector,
                    gradient,
                    stroke.paint.mode,
                    PaintTarget::Stroke,
                    size,
                    &shape.ease,
                )
            } else {
                json!({"ty": "st", "c": visual(vector, VectorProperty::StrokeColor, &shape.ease, json!(stroke.rgba[..3].iter().map(|v| *v / 255.0).collect::<Vec<_>>()), |v| json!([v[0]/255.0,v[1]/255.0,v[2]/255.0])),
                    "o": scalar(vector, VectorProperty::StrokeColor, &shape.ease, stroke.rgba[3], 3, 1.0)})
            };
            item["w"] = scalar(
                vector,
                VectorProperty::StrokeWidth,
                &shape.ease,
                stroke.width,
                0,
                1.0,
            );
            item["lc"] = match stroke.cap {
                Cap::Butt => 1,
                Cap::Round => 2,
                Cap::Square => 3,
            }
            .into();
            item["lj"] = match stroke.join {
                Join::Miter => 1,
                Join::Round => 2,
                Join::Bevel => 3,
            }
            .into();
            item["ml"] = 4.into();
            if let Some(dash) = stroke.dash {
                item["d"] = json!([{"n":"d","v":constant(dash.length)}, {"n":"g","v":constant(dash.gap)},
                    {"n":"o","v":scalar(vector, VectorProperty::DashOffset, &shape.ease, dash.offset, 0, 1.0)}]);
            }
            shapes.push(item);
        }
        if let Some(trim) = vector.trim
            && (trim.fill
                || vector
                    .stroke
                    .as_ref()
                    .is_some_and(|stroke| stroke.paint.mode != PaintMode::None))
        {
            shapes.push(json!({"ty":"tm","m":1,
                "s":scalar(vector,VectorProperty::TrimStart,&shape.ease,trim.start,0,1.0),
                "e":scalar(vector,VectorProperty::TrimEnd,&shape.ease,trim.end,0,1.0),
                "o":scalar(vector,VectorProperty::TrimOffset,&shape.ease,trim.offset,0,1.0)}));
        }
    }
    Ok(shapes)
}

fn outlined_text(scene: &Scene, shape: &Shape, size: [f32; 2]) -> PathData {
    let [w, h] = size;
    if !crate::text_render::TextInk::used_by(shape) {
        return crate::text::outline(crate::text::content(shape), w, h)
            .map_or_else(PathData::default, |path| {
                PathData::from_outline(&path, size)
            });
    }
    let mut frame = scene.layer_frame(shape, 0.0);
    frame.pose.w = w;
    frame.pose.h = h;
    frame.transform = Transform::identity();
    crate::text_render::TextInk::new(
        scene,
        shape,
        frame,
        Transform::identity(),
        &shape.color,
        0.0,
        &mut Default::default(),
        &mut Default::default(),
        &mut Default::default(),
    )
    .map_or_else(PathData::default, |ink| ink.outlined(size))
}

fn visual(
    vector: &VectorLayer,
    property: VectorProperty,
    ease: &str,
    rest: Value,
    convert: impl Fn([f32; 5]) -> Value,
) -> Value {
    let keys: Vec<_> = vector
        .keys
        .iter()
        .filter(|key| key.property == property)
        .filter_map(|key| match key.value {
            VectorValue::Numbers(values) => Some(animated_key(
                key.t,
                convert(values),
                key.ease.as_deref().unwrap_or(ease),
                key.curve,
            )),
            _ => None,
        })
        .collect();
    if keys.is_empty() {
        constant(rest)
    } else {
        json!({"a":1,"k":keys})
    }
}

fn scalar(
    vector: &VectorLayer,
    property: VectorProperty,
    ease: &str,
    rest: f32,
    at: usize,
    ratio: f32,
) -> Value {
    visual(vector, property, ease, json!(rest * ratio), |values| {
        json!([values[at] * ratio])
    })
}

fn gradient_shape(
    vector: &VectorLayer,
    gradient: &Gradient,
    mode: PaintMode,
    target: PaintTarget,
    [w, h]: [f32; 2],
    ease: &str,
) -> Value {
    let [x1, y1, x2, y2] = gradient.ends;
    let property = VectorProperty::GradientEnds(target);
    let mut stops: Vec<f32> = gradient
        .stops
        .iter()
        .flat_map(|stop| {
            [
                stop.offset / 100.0,
                stop.rgba[0] / 255.0,
                stop.rgba[1] / 255.0,
                stop.rgba[2] / 255.0,
            ]
        })
        .collect();
    stops.extend(
        gradient
            .stops
            .iter()
            .flat_map(|stop| [stop.offset / 100.0, stop.rgba[3] / 100.0]),
    );
    json!({"ty":if target == PaintTarget::Fill {"gf"} else {"gs"},
        "r":1, "t":if mode == PaintMode::Radial {2} else {1}, "h":constant(0), "a":constant(0),
        "s":visual(vector,property,ease,json!([x1*w,y1*h]), |v| json!([v[0]*w,v[1]*h])),
        "e":visual(vector,property,ease,json!([x2*w,y2*h]), |v| json!([v[2]*w,v[3]*h])),
        "g":{"p":gradient.stops.len(),"k":constant(json!(stops))},
        "o":constant(100)})
}
