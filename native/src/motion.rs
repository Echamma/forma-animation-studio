//! Drawn moves, as Electron Forma makes them (`src/animate/motion.js`, `src/model/keyframes.js`): an arrow drawn on
//! the canvas, or a path recorded with the mouse, becomes keyframes from the playhead. The move is kept in the layer's
//! `motions` (with the keyframes it replaced), so it can be retimed, rescaled or removed later. Its keyframes carry its
//! `motionId`.

use crate::model::{Key, Pose, Scene, Shape};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Motion {
    pub id: String,
    /// "arrow" or "path".
    #[serde(rename = "type")]
    pub kind: String,
    pub start_time: f64,
    pub duration: f64,
    /// How far the move goes, in percent of what was drawn.
    pub distance: f32,
    /// Degrees it turns along the way.
    pub spin: f32,
    /// Whether it turns to face where it's going.
    pub face: bool,
    pub start_pose: Pose,
    pub source_points: Vec<Point>,
    #[serde(default)]
    pub base_keys: Vec<Key>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// How a new move is made: the options beside Electron's Draw arrow and Record path tools.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub seconds: f64,
    pub distance: f32,
    pub spin: f32,
    pub face: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { seconds: 1.0, distance: 100.0, spin: 0.0, face: false }
    }
}

pub fn motions(shape: &Shape) -> Vec<Motion> {
    shape.extra.get("motions").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn set_motions(shape: &mut Shape, motions: &[Motion]) {
    shape.extra.insert("motions".into(), serde_json::to_value(motions).unwrap());
}

fn motion_id(key: &Key) -> Option<&str> {
    key.extra.get("motionId").and_then(Value::as_str)
}

/// The drawn points, scaled by the move's distance around where it starts.
pub fn scaled_points(m: &Motion) -> Vec<Point> {
    let scale = m.distance / 100.0;
    let start = m.start_pose;
    m.source_points.iter().map(|p| Point { x: start.x + (p.x - start.x) * scale, y: start.y + (p.y - start.y) * scale }).collect()
}

/// Puts back the keyframes a move replaced (Electron's `restoreMotionKeys`).
fn restore_keys(shape: &mut Shape, m: &Motion) {
    shape.keys.retain(|k| motion_id(k) != Some(&m.id));
    let near = |a: f32, b: f32| (a - b).abs() < 0.011;
    if !m.base_keys.iter().any(|k| (k.t - m.start_time).abs() < 0.011) {
        let p = m.start_pose;
        shape.keys.retain(|k| !((k.t - m.start_time).abs() < 0.011 && near(k.x, p.x) && near(k.y, p.y) && near(k.w, p.w) && near(k.h, p.h) && near(k.r, p.r) && near(k.o, p.o)));
    }
    for key in &m.base_keys {
        shape.keys.retain(|x| (x.t - key.t).abs() > 0.001);
        shape.keys.push(key.clone());
    }
    shape.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
}

/// Writes a move's keyframes (Electron's `applyMotion`): one at its start, then one per point (paths drop points closer
/// than 3 px), spread evenly over its length, turning by its spin and, if it faces its way, along the path.
fn apply(shape: &mut Shape, m: &Motion, scene_duration: f64) {
    shape.keys.retain(|k| motion_id(k) != Some(&m.id));
    let points = scaled_points(m);
    let end = scene_duration.min(m.start_time + m.duration);
    let span = end - m.start_time;
    if span <= 0.01 {
        return;
    }
    let key = |t: f64, p: Pose| {
        let mut extra = Map::new();
        extra.insert("motionId".into(), m.id.clone().into());
        Key { t, x: p.x, y: p.y, w: p.w, h: p.h, r: p.r, o: p.o, ease: None, curve: None, extra }
    };
    shape.keys.retain(|k| k.t < m.start_time - 0.001 || k.t > end + 0.001);
    shape.keys.push(key(m.start_time, m.start_pose));
    let samples: Vec<Point> = if m.kind == "arrow" {
        points
    } else {
        points.iter().enumerate().filter(|&(i, p)| i == 0 || i == points.len() - 1 || (p.x - points[i - 1].x).hypot(p.y - points[i - 1].y) > 3.0).map(|(_, p)| *p).collect()
    };
    for (i, p) in samples.iter().enumerate().skip(1) {
        let f = i as f32 / (samples.len() - 1) as f32;
        let (next, previous) = (samples[(i + 1).min(samples.len() - 1)], samples[i.saturating_sub(1)]);
        let mut pose = Pose { x: p.x, y: p.y, r: m.start_pose.r + m.spin * f, ..m.start_pose };
        if m.face {
            pose.r = (next.y - previous.y).atan2(next.x - previous.x).to_degrees() + m.spin * f;
        }
        shape.keys.push(key(((m.start_time + span * f as f64) * 100.0).round() / 100.0, pose));
    }
    shape.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut i = 0;
    while i + 1 < shape.keys.len() {
        if (shape.keys[i].t - shape.keys[i + 1].t).abs() <= 0.001 {
            shape.keys.remove(i);
        } else {
            i += 1;
        }
    }
}

/// The keyframes a move at `start`..`end` replaces: the layer's own ones (not another move's) inside it.
fn base_keys(shape: &Shape, start: f64, end: f64) -> Vec<Key> {
    shape.keys.iter().filter(|k| k.t >= start - 0.001 && k.t <= end + 0.001 && motion_id(k).is_none()).cloned().collect()
}

/// Makes a move for the layer at `index` from what was drawn (`kind` "arrow": its start and end; "path": every
/// point), starting at `time` from the layer's `start` pose (Electron's `finishMotion`). Moves it overlaps are
/// replaced. Returns the new move's id.
pub fn draw(scene: &mut Scene, index: usize, kind: &str, start: Pose, drawn: &[Point], time: f64, options: Options, id: String) -> Result<String, String> {
    let window = (time, time + options.seconds);
    if !window.1.is_finite() || window.1 - window.0 < 0.05 {
        return Err("Enter a valid animation length".into());
    }
    scene.duration = scene.duration.max(window.1);
    let raw: Vec<Point> = if kind == "arrow" {
        vec![Point { x: start.x, y: start.y }, *drawn.last().ok_or("Nothing was drawn")?]
    } else {
        drawn.iter().enumerate().filter(|&(i, p)| i == 0 || (p.x - drawn[i - 1].x).hypot(p.y - drawn[i - 1].y) > 3.0).map(|(_, p)| *p).collect()
    };
    let step = raw.len().div_ceil(90).max(1);
    let source_points = raw.iter().enumerate().filter(|&(i, _)| i % step == 0 || i == raw.len() - 1).map(|(_, p)| *p).collect();
    let shape = &mut scene.shapes[index];
    let mut list = motions(shape);
    let (overlapping, kept): (Vec<_>, Vec<_>) = list.drain(..).partition(|m| m.start_time < window.1 && m.start_time + m.duration > window.0);
    for old in &overlapping {
        restore_keys(shape, old);
    }
    let m = Motion {
        id: id.clone(),
        kind: kind.into(),
        start_time: window.0,
        duration: window.1 - window.0,
        distance: options.distance.clamp(0.0, 500.0),
        spin: options.spin.clamp(-10000.0, 10000.0),
        face: options.face,
        start_pose: start,
        source_points,
        base_keys: base_keys(shape, window.0, window.1),
        extra: Map::new(),
    };
    let mut list = kept;
    list.push(m.clone());
    set_motions(shape, &list);
    apply(shape, &m, scene.duration);
    Ok(id)
}

/// Retimes or reshapes a move (Electron's `editMotion`): it can't overlap another move on the layer.
pub fn edit(scene: &mut Scene, index: usize, id: &str, start: f64, length: f64, options: Options) -> Result<(), String> {
    if !start.is_finite() || start < 0.0 || !length.is_finite() || length < 0.1 {
        return Err("Enter a valid start and animation length".into());
    }
    let (start, length) = ((start * 100.0).round() / 100.0, (length * 100.0).round() / 100.0);
    let shape = &mut scene.shapes[index];
    let mut list = motions(shape);
    let at = list.iter().position(|m| m.id == id).ok_or("That animation is gone")?;
    if list.iter().any(|other| other.id != id && start < other.start_time + other.duration - 0.001 && start + length > other.start_time + 0.001) {
        return Err("This animation overlaps another animation on the same shape".into());
    }
    scene.duration = scene.duration.max(start + length);
    let shape = &mut scene.shapes[index];
    restore_keys(shape, &list[at]);
    let m = &mut list[at];
    (m.start_time, m.duration) = (start, length);
    m.distance = options.distance.clamp(0.0, 500.0);
    m.spin = options.spin.clamp(-10000.0, 10000.0);
    m.face = options.face;
    m.base_keys = base_keys(shape, start, start + length);
    let m = m.clone();
    set_motions(shape, &list);
    apply(shape, &m, scene.duration);
    Ok(())
}

/// Removes a move and puts back the keyframes it replaced.
pub fn remove(scene: &mut Scene, index: usize, id: &str) {
    let shape = &mut scene.shapes[index];
    let mut list = motions(shape);
    if let Some(at) = list.iter().position(|m| m.id == id) {
        let m = list.remove(at);
        restore_keys(shape, &m);
        set_motions(shape, &list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "circle");
        scene
    }

    #[test]
    fn an_arrow_moves_the_layer_from_the_playhead() {
        let mut scene = scene();
        let start = scene.shapes[0].pose(1.0);
        let options = Options { seconds: 2.0, distance: 50.0, ..Default::default() };
        draw(&mut scene, 0, "arrow", start, &[Point { x: 680.0, y: 300.0 }], 1.0, options, "m".into()).unwrap();
        let shape = &scene.shapes[0];
        assert_eq!(shape.keys.iter().map(|k| k.t).collect::<Vec<_>>(), vec![0.0, 1.0, 3.0]);
        assert_eq!(shape.pose(3.0).x, 580.0, "half of the 200 px drawn");
        assert_eq!(motions(shape)[0].base_keys.len(), 0);
        // Reshaped to the full distance, the end key moves; removed, only the original keyframe is left.
        edit(&mut scene, 0, "m", 1.0, 2.0, Options { distance: 100.0, ..options }).unwrap();
        assert_eq!(scene.shapes[0].pose(3.0).x, 680.0);
        remove(&mut scene, 0, "m");
        assert_eq!(scene.shapes[0].keys.len(), 1);
        assert!(motions(&scene.shapes[0]).is_empty());
    }

    #[test]
    fn a_path_follows_the_mouse_and_can_face_its_way() {
        let mut scene = scene();
        let start = scene.shapes[0].pose(0.0);
        let drawn: Vec<Point> = (0..=20).map(|i| Point { x: 480.0, y: 300.0 + i as f32 * 10.0 }).collect();
        draw(&mut scene, 0, "path", start, &drawn, 0.0, Options { face: true, ..Default::default() }, "p".into()).unwrap();
        let shape = &scene.shapes[0];
        assert_eq!(shape.keys.len(), 21);
        assert_eq!(shape.pose(1.0).y, 500.0);
        assert!((shape.keys[5].r - 90.0).abs() < 0.01, "facing down the screen");
        assert!(shape.keys.iter().all(|k| k.extra.get("motionId").is_some()));
    }

    #[test]
    fn drawing_over_a_move_replaces_it_and_keeps_the_layers_own_keys() {
        let mut scene = scene();
        let shape = &mut scene.shapes[0];
        shape.edit_pose(0.5, Pose { y: 100.0, ..shape.pose(0.0) });
        let start = scene.shapes[0].pose(0.0);
        draw(&mut scene, 0, "arrow", start, &[Point { x: 600.0, y: 300.0 }], 0.0, Options::default(), "one".into()).unwrap();
        assert!(!scene.shapes[0].keys.iter().any(|k| k.t == 0.5), "inside the move");
        draw(&mut scene, 0, "arrow", start, &[Point { x: 300.0, y: 300.0 }], 0.0, Options::default(), "two".into()).unwrap();
        assert_eq!(motions(&scene.shapes[0]).iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["two"]);
        remove(&mut scene, 0, "two");
        assert!(scene.shapes[0].keys.iter().any(|k| k.t == 0.5 && k.y == 100.0), "its own keyframe is back");
        assert!(edit(&mut scene, 0, "two", 0.0, 1.0, Options::default()).is_err());
    }
}
