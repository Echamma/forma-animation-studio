//! Version-one Forma scenes, preserving fields the native editor does not handle yet on every save.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub version: u32,
    pub project_id: String,
    pub project_name: String,
    #[serde(default = "landscape_width")]
    pub width: f32,
    #[serde(default = "landscape_height")]
    pub height: f32,
    pub duration: f64,
    pub bg: String,
    pub shapes: Vec<Shape>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub color: String,
    #[serde(default = "smooth")]
    pub ease: String,
    pub keys: Vec<Key>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_time: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_time: Option<f64>,
    #[serde(default)]
    pub base_rotation: f32,
    #[serde(default)]
    pub rotation_clips: Vec<RotationClip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disappear: Option<Disappear>,
    #[serde(default)]
    pub color_keys: Vec<ColorKey>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationClip {
    pub id: String,
    pub start_time: f64,
    pub duration: f64,
    pub degrees: f32,
    pub ease: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disappear {
    pub start_time: f64,
    pub duration: f64,
    pub reappear_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reappear_time: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_shape_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_clip_id: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorKey {
    pub t: f64,
    pub color: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn smooth() -> String { "smooth".into() }
fn landscape_width() -> f32 { 960.0 }
fn landscape_height() -> f32 { 600.0 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub t: f64,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub o: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<[f32; 4]>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f32, pub y: f32, pub w: f32, pub h: f32, pub r: f32, pub o: f32,
}

impl From<&Key> for Pose {
    fn from(k: &Key) -> Self { Self { x: k.x, y: k.y, w: k.w, h: k.h, r: k.r, o: k.o } }
}

impl Scene {
    pub fn empty(id: String) -> Self {
        Self { version: 1, project_id: id, project_name: "Untitled exploration".into(), width: 960.0, height: 600.0,
            duration: 4.0, bg: "#f4f0e8".into(), shapes: Vec::new(), extra: Map::new() }
    }

    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        let mut scene: Self = serde_json::from_slice(bytes).map_err(|e| format!("Invalid Forma project: {e}"))?;
        for shape in &mut scene.shapes {
            shape.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        }
        if scene.version != 1 || !(100.0..=4000.0).contains(&scene.width) || !(100.0..=4000.0).contains(&scene.height)
            || !scene.duration.is_finite() || scene.duration < 1.0 || scene.shapes.len() > 500
            || scene.shapes.iter().any(|shape| shape.keys.is_empty() || shape.keys.iter().any(|key| !key.t.is_finite() || key.w <= 0.0 || key.h <= 0.0)) {
            return Err("This is not a usable Forma version-one scene.".into());
        }
        Ok(scene)
    }

    pub fn add_shape(&mut self, id: String, kind: &str) -> String {
        let (name, color) = match kind {
            "circle" => ("Circle", "#ed795e"),
            "triangle" => ("Triangle", "#c3d77f"),
            "diamond" => ("Diamond", "#aaa0ce"),
            _ => ("Rectangle", "#aaa0ce"),
        };
        self.shapes.push(Shape { id: id.clone(), name: name.into(), kind: kind.into(), color: color.into(), ease: smooth(),
            keys: vec![Key { t: 0.0, x: self.width / 2.0, y: self.height / 2.0, w: 160.0, h: 160.0, r: 0.0, o: 100.0,
                ease: None, curve: None, extra: Map::new() }], in_time: None, out_time: None, base_rotation: 0.0,
            rotation_clips: Vec::new(), disappear: None, color_keys: Vec::new(), extra: Map::new() });
        id
    }

    pub fn add_text(&mut self, id: String) -> String {
        let font_size = (self.width.min(self.height) * 0.08).clamp(24.0, 320.0).round();
        let text_width = font_size * 4.2;
        self.add_shape(id.clone(), "text");
        let shape = self.shapes.last_mut().unwrap();
        shape.name = "Your text".into();
        shape.color = "#1F3D2B".into();
        shape.keys[0].w = text_width;
        shape.keys[0].h = font_size;
        shape.extra.insert("text".into(), "Your text".into());
        shape.extra.insert("linkId".into(), "".into());
        shape.extra.insert("offsetX".into(), 0.into());
        shape.extra.insert("offsetY".into(), 0.into());
        shape.extra.insert("textWidth".into(), serde_json::json!(text_width));
        shape.extra.insert("fontSize".into(), serde_json::json!(font_size));
        id
    }

    /// Electron Forma's size for imported media: its shape, the longer side 38 % of the scene's shorter side.
    pub fn fitted_size(&self, width: f32, height: f32) -> (f32, f32) {
        let longest = self.width.min(self.height) * 0.38;
        let ratio = (width / height.max(0.001)).clamp(0.05, 20.0);
        if ratio >= 1.0 { (longest, longest / ratio) } else { (longest * ratio, longest) }
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        let (sx, sy) = (width / self.width, height / self.height);
        for shape in &mut self.shapes {
            for key in &mut shape.keys {
                key.x *= sx; key.y *= sy; key.w *= sx; key.h *= sy;
            }
        }
        self.width = width;
        self.height = height;
    }

    pub fn minimum_duration(&self) -> f64 {
        self.shapes.iter().flat_map(|shape| shape.keys.iter().map(|key| key.t)
            .chain(shape.out_time)
            .chain(shape.rotation_clips.iter().map(|clip| clip.start_time + clip.duration)))
            .fold(1.0, f64::max)
    }

    /// The pose drawn on Forma's canvas: keyframes, rotation clips, disappearance, and linked text.
    pub fn render_pose(&self, shape: &Shape, t: f64) -> Pose {
        let mut own = shape.pose(t);
        own.r += shape.rotation_at(t) + shape.base_rotation;
        own.o *= self.visibility(shape, t);
        if shape.kind != "text" { return own; }
        let Some(target_id) = shape.extra.get("linkId").and_then(Value::as_str) else { return own };
        let Some(target) = self.shapes.iter().find(|s| s.id == target_id) else { return own };
        let mut p = target.pose(t);
        p.r += target.rotation_at(t) + target.base_rotation;
        let base_w = shape.extra.get("linkBase").and_then(|v| v.get("w")).and_then(Value::as_f64).unwrap_or(p.w as f64) as f32;
        let base_h = shape.extra.get("linkBase").and_then(|v| v.get("h")).and_then(Value::as_f64).unwrap_or(p.h as f64) as f32;
        let sx = p.w / base_w.max(0.001);
        let sy = p.h / base_h.max(0.001);
        let ox = value_f32(&shape.extra, "offsetX", 0.0) * sx;
        let oy = value_f32(&shape.extra, "offsetY", 0.0) * sy;
        let angle = p.r.to_radians();
        own.x = p.x + ox * angle.cos() - oy * angle.sin();
        own.y = p.y + ox * angle.sin() + oy * angle.cos();
        own.w = value_f32(&shape.extra, "textWidth", own.w) * sx.abs();
        own.h = value_f32(&shape.extra, "fontSize", own.h) * sy.abs();
        own.r = p.r + value_f32(&shape.extra, "rotationOffset", 0.0) + shape.rotation_at(t) + shape.base_rotation;
        own.o = (p.o * value_f32(&shape.extra, "opacityFactor", 1.0) * self.visibility(target, t) * self.visibility(shape, t)).clamp(0.0, 100.0);
        own
    }

    fn visibility(&self, shape: &Shape, t: f64) -> f32 {
        if !shape.visible(t, self.duration) { return 0.0; }
        let Some(d) = &shape.disappear else { return 1.0 };
        if t < d.start_time { return 1.0; }
        if t >= self.reappear_at(shape) { return 1.0; }
        if d.duration <= 0.0 || t >= d.start_time + d.duration { return 0.0; }
        (1.0 - (t - d.start_time) / d.duration) as f32
    }

    /// The pose the editor changes: what's drawn, except opacity from the layer's own keys, and for anything but
    /// linked text, rotation without rotation clips and base rotation (Electron's `editableState`).
    pub fn editable_pose(&self, shape: &Shape, t: f64) -> Pose {
        let mut pose = self.render_pose(shape, t);
        let own = shape.pose(t);
        pose.o = own.o;
        if !is_linked(shape) {
            pose.r = own.r;
        }
        pose
    }

    /// Sets the editable pose of the layer at `index`: linked text keeps it as offsets from what it's linked to
    /// (Electron's `setLinkedTextPose`), anything else as a keyframe at `t`.
    pub fn set_pose(&mut self, index: usize, t: f64, pose: Pose) {
        let shape = &self.shapes[index];
        let target = shape.extra.get("linkId").and_then(Value::as_str).filter(|_| shape.kind == "text")
            .and_then(|id| self.shapes.iter().find(|s| s.id == id));
        let Some(target) = target else { return self.shapes[index].edit_pose(t, pose) };
        let mut p = target.pose(t);
        p.r += target.rotation_at(t) + target.base_rotation;
        let base_w = shape.extra.get("linkBase").and_then(|v| v.get("w")).and_then(Value::as_f64).unwrap_or(p.w as f64) as f32;
        let base_h = shape.extra.get("linkBase").and_then(|v| v.get("h")).and_then(Value::as_f64).unwrap_or(p.h as f64) as f32;
        let (sx, sy) = (p.w / if base_w == 0.0 { p.w } else { base_w }, p.h / if base_h == 0.0 { p.h } else { base_h });
        let or_one = |v: f32| if v == 0.0 || !v.is_finite() { 1.0 } else { v };
        let angle = (-p.r).to_radians();
        let (dx, dy) = (pose.x - p.x, pose.y - p.y);
        let rotation = pose.r - p.r - shape.rotation_at(t) - shape.base_rotation;
        let opacity = if p.o != 0.0 { pose.o / p.o } else { 1.0 };
        let extra = &mut self.shapes[index].extra;
        extra.insert("offsetX".into(), serde_json::json!((dx * angle.cos() - dy * angle.sin()) / or_one(sx)));
        extra.insert("offsetY".into(), serde_json::json!((dx * angle.sin() + dy * angle.cos()) / or_one(sy)));
        extra.insert("textWidth".into(), serde_json::json!(pose.w / or_one(sx.abs())));
        extra.insert("fontSize".into(), serde_json::json!(pose.h / or_one(sy.abs())));
        extra.insert("rotationOffset".into(), serde_json::json!(rotation));
        extra.insert("opacityFactor".into(), serde_json::json!(opacity));
    }

    /// When a layer that disappears comes back (Electron's `reappearAt`); never is infinity.
    pub fn reappear_at(&self, shape: &Shape) -> f64 {
        let Some(d) = &shape.disappear else { return f64::INFINITY };
        match d.reappear_mode.as_str() {
            "time" => d.reappear_time.unwrap_or(f64::INFINITY),
            // With another layer's animation: when it ends, but never before this one has finished disappearing.
            "clip" => self.shapes.iter().find(|s| Some(&s.id) == d.trigger_shape_id.as_ref() && s.id != shape.id).and_then(|target| {
                let clip = d.trigger_clip_id.as_deref()?;
                if clip == "keys" && target.keys.len() > 1 { return target.keys.last().map(|k| k.t); }
                let motions = target.extra.get("motions").and_then(Value::as_array).into_iter().flatten().find_map(|m| {
                    (m.get("id")?.as_str()? == clip).then(|| m.get("startTime")?.as_f64().zip(m.get("duration")?.as_f64()))?
                }).map(|(start, duration)| start + duration);
                motions.or_else(|| target.rotation_clips.iter().find(|c| c.id == clip).map(|c| c.start_time + c.duration))
            }).map_or(f64::INFINITY, |end| end.max(d.start_time + d.duration)),
            _ => f64::INFINITY,
        }
    }
}

/// The shortest a clip can be (Electron's `MIN_CLIP`).
pub const MIN_CLIP: f64 = 0.05;

pub fn hundredths(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

impl Scene {
    /// Moves a layer's clip and everything timed inside it, never before the scene starts, and lengthens the scene
    /// to fit (Electron's `shiftLayer`). Returns how far it moved.
    pub fn shift_layer(&mut self, index: usize, delta: f64) -> f64 {
        let shape = &mut self.shapes[index];
        let delta = hundredths(delta).max(-shape.earliest_time());
        if delta == 0.0 {
            return 0.0;
        }
        shape.in_time = Some(hundredths(shape.clip_start() + delta));
        shape.out_time = shape.out_time.map(|end| hundredths(end + delta));
        for t in shape.keys.iter_mut().map(|k| &mut k.t).chain(shape.color_keys.iter_mut().map(|k| &mut k.t)) {
            *t = hundredths(*t + delta);
        }
        for motion in shape.extra.get_mut("motions").and_then(Value::as_array_mut).into_iter().flatten() {
            let shift = |value: &mut Value| if let Some(t) = value.as_f64() { *value = serde_json::json!(hundredths(t + delta)) };
            if let Some(start) = motion.get_mut("startTime") { shift(start) }
            for key in motion.get_mut("baseKeys").and_then(Value::as_array_mut).into_iter().flatten() {
                if let Some(t) = key.get_mut("t") { shift(t) }
            }
        }
        for clip in &mut shape.rotation_clips {
            clip.start_time = hundredths(clip.start_time + delta);
        }
        if let Some(d) = &mut shape.disappear {
            d.start_time = hundredths(d.start_time + delta);
            if d.reappear_mode == "time" {
                d.reappear_time = d.reappear_time.map(|t| hundredths(t + delta));
            }
        }
        self.duration = self.duration.max(shape.latest_time());
        delta
    }

    /// Changes where a layer's clip starts, and ends (`None`: with the scene), without moving its animation
    /// (Electron's `trimClip`).
    pub fn trim_clip(&mut self, index: usize, start: f64, end: Option<f64>) -> Result<(), String> {
        let (start, end) = (hundredths(start.max(0.0)), end.map(hundredths));
        if end.is_some_and(|end| end < start + MIN_CLIP) {
            return Err("A clip must last at least 0.05 seconds".into());
        }
        let shape = &mut self.shapes[index];
        if start > 0.0 || shape.in_time.is_some() {
            shape.in_time = Some(start);
        }
        shape.out_time = end;
        if let Some(end) = end {
            self.duration = self.duration.max(end);
        }
        Ok(())
    }
}

/// A drawn move's start and length (Electron keeps them in `motions`, which Forma native doesn't edit yet).
pub fn motions(shape: &Shape) -> impl Iterator<Item = (&Value, f64, f64)> {
    shape.extra.get("motions").and_then(Value::as_array).into_iter().flatten().filter_map(|m| {
        Some((m, m.get("startTime")?.as_f64()?, m.get("duration")?.as_f64()?))
    })
}

fn is_linked(shape: &Shape) -> bool {
    shape.kind == "text" && shape.extra.get("linkId").and_then(Value::as_str).is_some_and(|id| !id.is_empty())
}

/// Electron's resize handles, by direction from the center.
pub const HANDLES: [(i8, i8); 8] = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0)];

/// A pose dragged by its handle `(sx, sy)` by `delta` scene pixels, the layer turned `angle` degrees (Electron's
/// `resizedPose`): corners scale both ways together unless `freeform`, sides stretch one way, and the opposite side
/// stays put.
pub fn resized_pose(start: Pose, angle: f32, (sx, sy): (i8, i8), (dx, dy): (f32, f32), freeform: bool) -> Pose {
    let (sx, sy) = (sx as f32, sy as f32);
    let (cos, sin) = (angle.to_radians().cos(), angle.to_radians().sin());
    let (dx, dy) = (dx * cos + dy * sin, -dx * sin + dy * cos);
    let (max_w, max_h) = (start.w.max(4000.0), start.h.max(4000.0));
    let (mut w, mut h) = (start.w, start.h);
    if sx != 0.0 && sy != 0.0 && !freeform {
        let scale = (1.0 + (sx * dx * start.w + sy * dy * start.h) / (start.w * start.w + start.h * start.h))
            .clamp((5.0 / start.w).max(5.0 / start.h), (max_w / start.w).min(max_h / start.h));
        (w, h) = (start.w * scale, start.h * scale);
    } else {
        if sx != 0.0 { w = (start.w + sx * dx).clamp(5.0, max_w); }
        if sy != 0.0 { h = (start.h + sy * dy).clamp(5.0, max_h); }
    }
    if (w - start.w).abs() < 0.01 && (h - start.h).abs() < 0.01 { return start; }
    let (w, h) = ((w * 10.0).round() / 10.0, (h * 10.0).round() / 10.0);
    let (shift_x, shift_y) = (sx * (w - start.w) / 2.0, sy * (h - start.h) / 2.0);
    Pose { w, h, x: start.x + shift_x * cos - shift_y * sin, y: start.y + shift_x * sin + shift_y * cos, ..start }
}

fn value_f32(map: &Map<String, Value>, field: &str, fallback: f32) -> f32 {
    map.get(field).and_then(Value::as_f64).map(|v| v as f32).unwrap_or(fallback)
}

impl Shape {
    /// Whether the layer is on screen at `t`: from `in_time` up to, but not including, `out_time`. A layer without
    /// one lasts, and one that ends with the scene stays on its final frame (Electron's `inClip`).
    pub fn visible(&self, t: f64, scene_duration: f64) -> bool {
        const EPSILON: f64 = 1e-6;
        if self.in_time.is_some_and(|start| t < start - EPSILON) { return false; }
        self.out_time.is_none_or(|end| t < end - EPSILON || (end >= scene_duration - EPSILON && t >= scene_duration - EPSILON))
    }

    pub fn clip_start(&self) -> f64 {
        self.in_time.unwrap_or(0.0)
    }

    /// The earliest moment the layer's timing touches, so moves stay inside the scene.
    fn earliest_time(&self) -> f64 {
        std::iter::once(self.clip_start())
            .chain(self.keys.iter().map(|k| k.t))
            .chain(self.color_keys.iter().map(|k| k.t))
            .chain(motions(self).map(|(_, start, _)| start))
            .chain(self.rotation_clips.iter().map(|c| c.start_time))
            .chain(self.disappear.iter().map(|d| d.start_time))
            .fold(f64::INFINITY, f64::min)
    }

    /// The latest moment the layer's timing touches.
    pub fn latest_time(&self) -> f64 {
        std::iter::once(self.out_time.unwrap_or(0.0))
            .chain(self.keys.iter().map(|k| k.t))
            .chain(self.color_keys.iter().map(|k| k.t))
            .chain(motions(self).map(|(_, start, length)| start + length))
            .chain(self.rotation_clips.iter().map(|c| c.start_time + c.duration))
            .chain(self.disappear.iter().map(|d| d.start_time + d.duration))
            .chain(self.disappear.iter().filter(|d| d.reappear_mode == "time").filter_map(|d| d.reappear_time))
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// Where a video layer is in its file at scene time `t`: it starts at `mediaStart` when the clip starts, and loops.
    pub fn media_time(&self, t: f64, duration: f64) -> f64 {
        let local = (t - self.in_time.unwrap_or(0.0)).max(0.0) + self.extra.get("mediaStart").and_then(Value::as_f64).unwrap_or(0.0);
        if duration > 0.0 { local % duration } else { local }
    }

    pub fn rotation_at(&self, t: f64) -> f32 {
        self.rotation_clips.iter().filter(|clip| t > clip.start_time && clip.duration > 0.0).map(|clip| {
            let f = ((t - clip.start_time) / clip.duration).clamp(0.0, 1.0) as f32;
            clip.degrees * if clip.ease == "smooth" { f * f * (3.0 - 2.0 * f) } else { f }
        }).sum()
    }

    pub fn color_at(&self, t: f64) -> String {
        if self.color_keys.is_empty() { return self.color.clone(); }
        let before = self.color_keys.iter().rev().find(|key| key.t <= t).unwrap_or(&self.color_keys[0]);
        let after = self.color_keys.iter().find(|key| key.t >= t).unwrap_or_else(|| self.color_keys.last().unwrap());
        if before.t == after.t { return before.color.clone(); }
        let f = ease_at(&self.ease, ((t - before.t) / (after.t - before.t)).clamp(0.0, 1.0) as f32, None);
        let channel = |value: &str, i: usize| u8::from_str_radix(value.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f32;
        let (a, b) = (&before.color, &after.color);
        format!("#{:02x}{:02x}{:02x}", (channel(a, 1) + (channel(b, 1) - channel(a, 1)) * f).round() as u8,
            (channel(a, 3) + (channel(b, 3) - channel(a, 3)) * f).round() as u8,
            (channel(a, 5) + (channel(b, 5) - channel(a, 5)) * f).round() as u8)
    }

    pub fn pose(&self, t: f64) -> Pose {
        let before = self.keys.iter().rev().find(|key| key.t <= t).unwrap_or(&self.keys[0]);
        let after = self.keys.iter().find(|key| key.t >= t).unwrap_or_else(|| self.keys.last().unwrap());
        let span = after.t - before.t;
        let fraction = if span <= 0.0 { 0.0 } else { ((t - before.t) / span).clamp(0.0, 1.0) as f32 };
        let ease = before.ease.as_deref().unwrap_or(&self.ease);
        let fraction = ease_at(ease, fraction, before.curve);
        let mix = |a: f32, b: f32| a + (b - a) * fraction;
        Pose { x: mix(before.x, after.x), y: mix(before.y, after.y), w: mix(before.w, after.w),
            h: mix(before.h, after.h), r: mix(before.r, after.r), o: mix(before.o, after.o) }
    }

    /// Edits a pose at the playhead, inserting a keyframe when the playhead sits between keys. Like Electron's
    /// `keyAt`, keys sit on hundredths of a second and a key within one of them is the one edited.
    pub fn edit_pose(&mut self, t: f64, pose: Pose) {
        let t = (t * 100.0).round() / 100.0;
        let at = self.keys.iter().position(|key| (key.t - t).abs() < 0.011);
        let index = match at {
            Some(index) => index,
            None => {
                let index = self.keys.partition_point(|key| key.t < t);
                self.keys.insert(index, Key { t, x: pose.x, y: pose.y, w: pose.w, h: pose.h, r: pose.r, o: pose.o,
                    ease: None, curve: None, extra: Map::new() });
                index
            }
        };
        let key = &mut self.keys[index];
        (key.x, key.y, key.w, key.h, key.r, key.o) = (pose.x, pose.y, pose.w.max(1.0), pose.h.max(1.0), pose.r, pose.o.clamp(0.0, 100.0));
    }
}

fn ease_at(kind: &str, f: f32, curve: Option<[f32; 4]>) -> f32 {
    match kind {
        "linear" => f,
        "hold" => 0.0,
        "ease-in" => bezier(f, [0.42, 0.0, 1.0, 1.0]),
        "ease-out" => bezier(f, [0.0, 0.0, 0.58, 1.0]),
        "custom" if curve.is_some() => bezier(f, curve.unwrap()),
        _ => f * f * (3.0 - 2.0 * f),
    }
}

fn bezier(progress: f32, [x1, y1, x2, y2]: [f32; 4]) -> f32 {
    let coordinate = |t: f32, a: f32, b: f32| 3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t.powi(3);
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..25 {
        let mid = (lo + hi) / 2.0;
        if coordinate(mid, x1, x2) < progress { lo = mid; } else { hi = mid; }
    }
    coordinate((lo + hi) / 2.0, y1, y2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_scenes_round_trip_without_losing_unsupported_animation() {
        let source = br##"{"version":1,"projectId":"id","projectName":"Demo","width":960,"height":600,"duration":4,"bg":"#ffffff","shapes":[{"id":"a","name":"Circle","type":"circle","color":"#ff0000","ease":"linear","keys":[{"t":0,"x":0,"y":0,"w":100,"h":100,"r":0,"o":100},{"t":4,"x":400,"y":0,"w":100,"h":100,"r":0,"o":100}],"motions":[{"type":"arrow","distance":15}]}],"exportSetting":30}"##;
        let scene = Scene::read(source).unwrap();
        assert_eq!(scene.shapes[0].pose(2.0).x, 200.0);
        let output: Value = serde_json::to_value(&scene).unwrap();
        assert_eq!(output["shapes"][0]["motions"][0]["distance"], 15);
        assert_eq!(output["exportSetting"], 30);
        assert!(output["shapes"][0].get("disappear").is_none(), "Electron rejects null optional fields");
        assert!(output["shapes"][0]["keys"][0].get("ease").is_none());
    }

    #[test]
    fn hold_easing_and_clip_visibility_match_forma() {
        let mut scene = Scene::empty("test".into());
        let shape: Shape = serde_json::from_str(r##"{"id":"a","name":"A","type":"rect","color":"#ffffff","ease":"hold","inTime":1,"outTime":3,"keys":[{"t":0,"x":0,"y":0,"w":10,"h":10,"r":0,"o":100},{"t":4,"x":100,"y":0,"w":10,"h":10,"r":0,"o":100}]}"##).unwrap();
        assert!(!shape.visible(0.5, scene.duration));
        assert!(shape.visible(2.0, scene.duration));
        assert!(!shape.visible(3.0, scene.duration));
        let to_the_end = Shape { out_time: Some(4.0), ..shape.clone() };
        assert!(to_the_end.visible(4.0, scene.duration), "the final frame shows a layer that lasts to the end");
        assert_eq!(shape.pose(2.0).x, 0.0);
        scene.shapes.push(shape);
    }

    #[test]
    fn editing_between_keys_keeps_the_other_poses() {
        let mut scene = Scene::empty("test".into());
        let id = scene.add_shape("circle".into(), "circle");
        let shape = &mut scene.shapes[0];
        shape.edit_pose(4.0, Pose { x: 700.0, ..shape.pose(4.0) });
        shape.edit_pose(2.0, Pose { x: 500.0, ..shape.pose(2.0) });
        assert_eq!(shape.keys.iter().map(|k| k.t).collect::<Vec<_>>(), vec![0.0, 2.0, 4.0]);
        assert_eq!(shape.pose(4.0).x, 700.0);
        assert_eq!(shape.id, id);
    }

    #[test]
    fn rotation_disappearance_and_color_keys_are_sampled() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "rect");
        let shape = &mut scene.shapes[0];
        shape.rotation_clips.push(RotationClip { id: "spin".into(), start_time: 1.0, duration: 2.0, degrees: 180.0, ease: "linear".into(), extra: Map::new() });
        shape.disappear = Some(Disappear { start_time: 2.0, duration: 1.0, reappear_mode: "time".into(), reappear_time: Some(3.5), trigger_shape_id: None, trigger_clip_id: None, extra: Map::new() });
        shape.color_keys = vec![ColorKey { t: 0.0, color: "#000000".into(), extra: Map::new() }, ColorKey { t: 4.0, color: "#ffffff".into(), extra: Map::new() }];
        let shape = &scene.shapes[0];
        assert_eq!(scene.render_pose(shape, 2.5).r, 135.0);
        assert_eq!(scene.render_pose(shape, 2.5).o, 50.0);
        assert_eq!(shape.color_at(2.0), "#808080");
        assert_eq!(scene.render_pose(shape, 3.6).o, 100.0);
    }

    #[test]
    fn layers_can_reappear_with_a_drawn_move_but_not_before_they_have_gone() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("mover".into(), "circle");
        scene.add_shape("ghost".into(), "rect");
        scene.shapes[0].extra.insert("motions".into(), serde_json::json!([{"id": "m1", "type": "arrow", "startTime": 0.5, "duration": 1.0}]));
        scene.shapes[1].disappear = Some(Disappear { start_time: 1.0, duration: 1.0, reappear_mode: "clip".into(), reappear_time: None,
            trigger_shape_id: Some("mover".into()), trigger_clip_id: Some("m1".into()), extra: Map::new() });
        let ghost = &scene.shapes[1];
        assert_eq!(scene.render_pose(ghost, 1.5).o, 50.0, "still fading although the move ended at 1.5");
        assert_eq!(scene.render_pose(ghost, 2.0).o, 100.0);
    }

    #[test]
    fn handles_resize_like_electron() {
        let start = Pose { x: 100.0, y: 100.0, w: 100.0, h: 50.0, r: 0.0, o: 100.0 };
        // The right side moves right 20: 20 wider, the center 10 right, the left side where it was.
        let right = resized_pose(start, 0.0, (1, 0), (20.0, 5.0), false);
        assert_eq!((right.w, right.h, right.x, right.y), (120.0, 50.0, 110.0, 100.0));
        // A corner keeps the shape's proportions.
        let corner = resized_pose(start, 0.0, (1, 1), (25.0, 0.0), false);
        assert!((corner.w / corner.h - 2.0).abs() < 0.01 && corner.w > 100.0);
        // Turned a quarter, dragging down widens it.
        let turned = resized_pose(start, 90.0, (1, 0), (0.0, 20.0), false);
        assert!((turned.w - 120.0).abs() < 0.01 && (turned.y - 110.0).abs() < 0.01, "{turned:?}");
    }

    #[test]
    fn linked_text_moves_as_offsets_from_its_shape() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("c".into(), "circle");
        scene.add_text("t".into());
        scene.shapes[1].extra.insert("linkId".into(), "c".into());
        let mut pose = scene.editable_pose(&scene.shapes[1], 0.0);
        pose.x += 30.0;
        scene.set_pose(1, 0.0, pose);
        let drawn = scene.render_pose(&scene.shapes[1], 0.0);
        assert!((drawn.x - (scene.shapes[0].keys[0].x + 30.0)).abs() < 0.01, "{drawn:?}");
        assert_eq!(scene.shapes[1].keys.len(), 1, "no keyframe of its own");
    }

    #[test]
    fn keys_sit_on_hundredths_of_a_second() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "rect");
        let shape = &mut scene.shapes[0];
        shape.edit_pose(1.234, Pose { x: 1.0, ..shape.pose(0.0) });
        shape.edit_pose(1.236, Pose { x: 2.0, ..shape.pose(0.0) });
        assert_eq!(shape.keys.iter().map(|k| k.t).collect::<Vec<_>>(), vec![0.0, 1.23]);
        assert_eq!(shape.keys[1].x, 2.0);
    }

    #[test]
    fn moving_a_clip_moves_its_animation_but_not_before_zero() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "rect");
        let shape = &mut scene.shapes[0];
        shape.in_time = Some(1.0);
        shape.out_time = Some(2.0);
        shape.edit_pose(1.5, Pose { x: 10.0, ..shape.pose(0.0) });
        shape.rotation_clips.push(RotationClip { id: "r".into(), start_time: 1.2, duration: 0.5, degrees: 90.0, ease: "linear".into(), extra: Map::new() });
        shape.extra.insert("motions".into(), serde_json::json!([{"id": "m", "startTime": 1.1, "duration": 0.4, "baseKeys": [{"t": 1.1}]}]));
        assert_eq!(scene.shift_layer(0, 3.5), 3.5);
        let shape = &scene.shapes[0];
        assert_eq!((shape.in_time, shape.out_time), (Some(4.5), Some(5.5)));
        assert_eq!(shape.keys.iter().map(|k| k.t).collect::<Vec<_>>(), vec![3.5, 5.0]);
        assert_eq!(shape.rotation_clips[0].start_time, 4.7);
        assert_eq!(shape.extra["motions"][0]["startTime"], 4.6);
        assert_eq!(shape.extra["motions"][0]["baseKeys"][0]["t"], 4.6);
        assert_eq!(scene.duration, 5.5, "the scene grows to fit");
        // The first key sits at 3.5: it can go back no further than zero.
        assert_eq!(scene.shift_layer(0, -10.0), -3.5);
        assert_eq!(scene.shapes[0].keys[0].t, 0.0);
    }

    #[test]
    fn trimming_keeps_the_animation_where_it_is() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "rect");
        assert!(scene.trim_clip(0, 1.0, Some(1.02)).is_err(), "too short");
        scene.trim_clip(0, 0.5, Some(6.0)).unwrap();
        assert_eq!((scene.shapes[0].in_time, scene.shapes[0].out_time, scene.duration), (Some(0.5), Some(6.0), 6.0));
        scene.trim_clip(0, 0.0, None).unwrap();
        assert_eq!((scene.shapes[0].in_time, scene.shapes[0].out_time), (Some(0.0), None));
        assert_eq!(scene.shapes[0].keys[0].t, 0.0);
    }

    #[test]
    fn resizing_keeps_shape_positions_proportional() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "circle");
        scene.resize(1920.0, 1200.0);
        assert_eq!((scene.shapes[0].keys[0].x, scene.shapes[0].keys[0].w), (960.0, 320.0));
        assert_eq!((scene.width, scene.height), (1920.0, 1200.0));
    }

    #[test]
    fn new_text_has_the_electron_fields_and_round_trips() {
        let mut scene = Scene::empty("test".into());
        scene.add_text("words".into());
        let bytes = serde_json::to_vec(&scene).unwrap();
        let restored = Scene::read(&bytes).unwrap();
        let text = &restored.shapes[0];
        assert_eq!(text.kind, "text");
        assert_eq!(text.extra["text"], "Your text");
        assert!(text.extra["textWidth"].as_f64().unwrap() > 0.0);
        assert!(text.extra["fontSize"].as_f64().unwrap() > 0.0);
    }
}
