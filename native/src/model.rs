//! Version-one Forma scenes, preserving fields the native editor does not handle yet on every save.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub version: u32,
    pub project_id: String,
    pub project_name: String,
    pub width: f32,
    pub height: f32,
    pub duration: f64,
    pub bg: String,
    pub shapes: Vec<Shape>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    #[serde(default)]
    pub in_time: Option<f64>,
    #[serde(default)]
    pub out_time: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn smooth() -> String { "smooth".into() }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key {
    pub t: f64,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub o: f32,
    #[serde(default)]
    pub ease: Option<String>,
    #[serde(default)]
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
        let scene: Self = serde_json::from_slice(bytes).map_err(|e| format!("Invalid Forma project: {e}"))?;
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
                ease: None, curve: None, extra: Map::new() }], in_time: None, out_time: None, extra: Map::new() });
        id
    }
}

impl Shape {
    pub fn visible(&self, t: f64, scene_duration: f64) -> bool {
        t >= self.in_time.unwrap_or(0.0) && t < self.out_time.unwrap_or(scene_duration)
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

    /// Edits a pose at the playhead, inserting a keyframe when the playhead sits between keys.
    pub fn edit_pose(&mut self, t: f64, pose: Pose) {
        let at = self.keys.iter().position(|key| (key.t - t).abs() < 1e-6);
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
    }

    #[test]
    fn hold_easing_and_clip_visibility_match_forma() {
        let mut scene = Scene::empty("test".into());
        let shape: Shape = serde_json::from_str(r##"{"id":"a","name":"A","type":"rect","color":"#ffffff","ease":"hold","inTime":1,"outTime":3,"keys":[{"t":0,"x":0,"y":0,"w":10,"h":10,"r":0,"o":100},{"t":4,"x":100,"y":0,"w":10,"h":10,"r":0,"o":100}]}"##).unwrap();
        assert!(!shape.visible(0.5, scene.duration));
        assert!(shape.visible(2.0, scene.duration));
        assert!(!shape.visible(3.0, scene.duration));
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
}
