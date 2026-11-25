use crate::keyframes::Property;
use crate::model::{MAX_BLUR, Pose, Scene, Shape, is_linked};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tiny_skia::{Point, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub x: f32,
    pub y: f32,
}

impl Default for Anchor {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5 }
    }
}

impl Anchor {
    pub(crate) fn offset_in(self, pose: Pose) -> (f32, f32) {
        ((self.x - 0.5) * pose.w, (self.y - 0.5) * pose.h)
    }
}

#[derive(Clone, Copy)]
pub struct LayerFrame {
    pub pose: Pose,
    /// Maps centered layer pixels to scene pixels, including the anchor and all ancestors.
    pub transform: Transform,
    /// Maps the layer's position-key coordinates to scene pixels.
    pub space: Transform,
}

impl LayerFrame {
    pub fn anchor(self) -> (f32, f32) {
        point(self.space, self.pose.x, self.pose.y)
    }

    pub fn box_transform(self) -> Transform {
        Transform::from_scale(self.pose.w, self.pose.h).post_concat(self.transform)
    }

    pub(crate) fn scene_pose(self) -> Pose {
        let mut pose = self.pose;
        (pose.x, pose.y) = point(self.transform, 0.0, 0.0);
        if !self.space.is_identity() {
            let (sx, sy) = axis_scales(self.transform);
            pose.w *= sx;
            pose.h *= sy;
            let angle = self.transform.ky.atan2(self.transform.sx).to_degrees();
            let orientation = (self.space.ky - self.space.kx)
                .atan2(self.space.sx + self.space.sy)
                .to_degrees();
            let reference = pose.r + orientation;
            pose.r = reference + angle_delta(angle - reference);
        }
        pose
    }
}

pub(crate) fn axis_scales(t: Transform) -> (f32, f32) {
    (t.sx.hypot(t.ky), t.kx.hypot(t.sy))
}

/// The greatest stretch in any direction, independent of translation and rotation.
pub(crate) fn max_scale(t: Transform) -> f32 {
    ((t.sx + t.sy).hypot(t.ky - t.kx) + (t.sx - t.sy).hypot(t.ky + t.kx)) * 0.5
}

pub(crate) fn angle_delta(angle: f32) -> f32 {
    (angle + 180.0).rem_euclid(360.0) - 180.0
}

pub fn point(transform: Transform, x: f32, y: f32) -> (f32, f32) {
    let mut p = Point::from_xy(x, y);
    transform.map_point(&mut p);
    (p.x, p.y)
}

pub fn vector(transform: Transform, x: f32, y: f32) -> (f32, f32) {
    (
        x * transform.sx + y * transform.kx,
        x * transform.ky + y * transform.sy,
    )
}

pub(crate) fn matrix(transform: Transform) -> Option<[f32; 6]> {
    (!transform.is_identity()).then_some([
        transform.sx,
        transform.ky,
        transform.kx,
        transform.sy,
        transform.tx,
        transform.ty,
    ])
}

fn from_matrix([sx, ky, kx, sy, tx, ty]: [f32; 6]) -> Transform {
    Transform::from_row(sx, ky, kx, sy, tx, ty)
}

fn centered(pose: Pose) -> Transform {
    Transform::from_rotate(pose.r).post_translate(pose.x, pose.y)
}

fn placement(shape: &Shape, pose: Pose) -> Transform {
    let transform = centered(pose);
    let (x, y) = shape.anchor.unwrap_or_default().offset_in(pose);
    if (x, y) == (0.0, 0.0) {
        transform
    } else {
        Transform::from_translate(-x, -y).post_concat(transform)
    }
}

fn link_target(shape: &Shape) -> Option<&str> {
    is_linked(shape)
        .then(|| shape.extra.get("linkId")?.as_str())
        .flatten()
}

impl Scene {
    pub fn layer_frame(&self, shape: &Shape, t: f64) -> LayerFrame {
        self.frame_in_chain(shape, t, &mut Vec::new())
    }

    fn frame_in_chain<'a>(
        &'a self,
        shape: &'a Shape,
        t: f64,
        chain: &mut Vec<&'a str>,
    ) -> LayerFrame {
        let pose = self.local_pose(shape, t);
        let own = placement(shape, pose);
        // Files can be edited outside Forma. Ignore a cyclic edge even if it bypassed the picker.
        if chain.contains(&shape.id.as_str()) {
            return LayerFrame {
                pose,
                transform: own,
                space: Transform::identity(),
            };
        }
        chain.push(&shape.id);
        let mut space = self.parent_in_chain(shape, t, chain);
        if let Some(target) = link_target(shape).and_then(|id| self.layer(id)) {
            let frame = self.frame_in_chain(target, t, chain);
            space = link_effect(target, t, frame).post_concat(space);
        }
        chain.pop();
        let transform = if space.is_identity() {
            own
        } else {
            own.post_concat(space)
        };
        LayerFrame {
            pose,
            transform,
            space,
        }
    }

    /// A screen-space pose for callers that need an approximate box. Drawing uses the full affine frame.
    pub fn render_pose(&self, shape: &Shape, t: f64) -> Pose {
        self.layer_frame(shape, t).scene_pose()
    }

    /// Canvas coordinates for the anchor, the lengths of the layer's edges and the angle of its x-axis.
    pub(crate) fn inspector_pose(&self, shape: &Shape, t: f64) -> Pose {
        let frame = self.layer_frame(shape, t);
        let mut pose = frame.scene_pose();
        (pose.x, pose.y) = frame.anchor();
        pose.o = self.editable_opacity(shape, t);
        pose
    }

    fn layer(&self, id: &str) -> Option<&Shape> {
        self.shapes.iter().find(|shape| shape.id == id)
    }

    fn following_frame<'a>(
        &'a self,
        parent: Option<&str>,
        t: f64,
        chain: &mut Vec<&'a str>,
    ) -> Transform {
        parent
            .and_then(|id| self.layer(id))
            .filter(|p| !chain.contains(&p.id.as_str()))
            .map_or_else(Transform::identity, |p| {
                self.frame_in_chain(p, t, chain).box_transform()
            })
    }

    fn parent_in_chain<'a>(
        &'a self,
        shape: &'a Shape,
        t: f64,
        chain: &mut Vec<&'a str>,
    ) -> Transform {
        let following = self.following_frame(shape.parent.as_deref(), t, chain);
        shape
            .parent_space
            .map_or(following, |basis| from_matrix(basis).post_concat(following))
    }

    pub(crate) fn parent_space_at(&self, shape: &Shape, t: f64) -> Transform {
        self.parent_in_chain(shape, t, &mut vec![&shape.id])
    }

    pub fn can_follow(&self, child: usize, candidate: usize) -> bool {
        let child = &self.shapes[child].id;
        let mut pending = vec![self.shapes[candidate].id.as_str()];
        let mut visited = Vec::new();
        while let Some(id) = pending.pop() {
            if id == child {
                return false;
            }
            if visited.contains(&id) {
                continue;
            }
            visited.push(id);
            if let Some(shape) = self.layer(id) {
                pending.extend(shape.parent.as_deref());
                pending.extend(link_target(shape));
            }
        }
        true
    }

    pub fn ancestor_depth(&self, index: usize) -> usize {
        let mut seen = vec![self.shapes[index].id.as_str()];
        let mut shape = &self.shapes[index];
        loop {
            let Some(parent) = shape
                .parent
                .as_deref()
                .or_else(|| link_target(shape))
                .and_then(|id| self.layer(id))
                .filter(|p| !seen.contains(&p.id.as_str()))
            else {
                return seen.len() - 1;
            };
            seen.push(&parent.id);
            shape = parent;
        }
    }

    pub(crate) fn unlink_text_space(
        &self,
        shape: &Shape,
        t: f64,
        space: Transform,
    ) -> Option<[f32; 6]> {
        let parent = self.following_frame(shape.parent.as_deref(), t, &mut vec![&shape.id]);
        parent
            .invert()
            .and_then(|inverse| matrix(space.post_concat(inverse)))
    }

    pub(crate) fn relink_text_space(
        &self,
        shape: &Shape,
        target: &Shape,
        t: f64,
        before: Transform,
    ) -> Option<[f32; 6]> {
        let effect = link_effect(target, t, self.layer_frame(target, t));
        let parent = self.following_frame(shape.parent.as_deref(), t, &mut vec![&shape.id]);
        let basis = effect
            .invert()?
            .post_concat(before)
            .post_concat(parent.invert()?);
        matrix(basis)
    }

    pub fn set_parent(
        &mut self,
        child: usize,
        parent: Option<usize>,
        t: f64,
    ) -> Result<(), &'static str> {
        if parent.is_some_and(|i| !self.can_follow(child, i)) {
            return Err("Following that layer would create a loop.");
        }
        let shape = &self.shapes[child];
        let id = parent.map(|i| self.shapes[i].id.clone());
        if shape.parent == id {
            return Ok(());
        }
        let old = self.parent_space_at(shape, t);
        let new = self.following_frame(id.as_deref(), t, &mut vec![&shape.id]);
        let inverse = new
            .invert()
            .filter(Transform::is_finite)
            .ok_or("The parent is too small at the playhead.")?;
        // Use a fixed space when attaching so the child stays in place at the playhead.
        let basis = old.post_concat(inverse);
        let shape = &mut self.shapes[child];
        shape.parent = id;
        shape.parent_space = matrix(basis);
        if shape.parent.is_none() {
            shape.bake_parent_space();
        }
        Ok(())
    }

    pub fn set_anchor(&mut self, index: usize, anchor: Anchor, t: f64) {
        let shape = &self.shapes[index];
        let old = shape.anchor.unwrap_or_default();
        if old == anchor || !anchor.x.is_finite() || !anchor.y.is_finite() {
            return;
        }
        let frame = self.layer_frame(shape, t);
        let (dx, dy) = vector(
            Transform::from_rotate(frame.pose.r),
            (anchor.x - old.x) * frame.pose.w,
            (anchor.y - old.y) * frame.pose.h,
        );
        if is_linked(shape) {
            self.shift_linked_position(index, t, (dx, dy));
        } else {
            self.shapes[index].translate_keys(dx, dy);
        }
        self.shapes[index].anchor = (anchor != Anchor::default()).then_some(anchor);
    }
}

fn link_effect(target: &Shape, t: f64, frame: LayerFrame) -> Transform {
    if target.anchor.is_none() && frame.space.is_identity() {
        return Transform::identity();
    }
    let mut old = target.pose(t);
    old.r += target.rotation_at(t) + target.base_rotation;
    centered(old)
        .invert()
        .map_or_else(Transform::identity, |inverse| {
            inverse.post_concat(frame.transform)
        })
}

impl Shape {
    pub(crate) fn bake_parent_space(&mut self) {
        let Some(basis) = self.parent_space.map(from_matrix) else {
            return;
        };
        // Legacy linked text derives its pose from an independent target. Its affine
        // space cannot generally be moved into its keys while that link is live.
        if is_linked(self) {
            return;
        }
        let Some((sx, sy, rotation)) = self.bake_components(basis) else {
            return;
        };
        // A change of local scale alters pixel widths, dashes and the shape of radial paint.
        if self.vector.is_some() && ((sx - 1.0).abs() > 1e-5 || (sy - 1.0).abs() > 1e-5) {
            return;
        }
        self.map_positions(basis);
        if let Some(pose) = &mut self.rest_pose {
            (pose.w, pose.h, pose.r) = (pose.w * sx, pose.h * sy, pose.r + rotation);
        }
        for key in self.keys.iter_mut().filter(|key| !key.linked_space) {
            (key.w, key.h, key.r) = (key.w * sx, key.h * sy, key.r + rotation);
        }
        self.map_motion_points(
            |pose| {
                map_number(pose, "w", |w| w * sx);
                map_number(pose, "h", |h| h * sy);
                map_number(pose, "r", |r| r + rotation);
            },
            |motion| {
                if motion.get("face").and_then(Value::as_bool) == Some(true) {
                    let offset = number(motion, "faceRotation").unwrap_or(0.0);
                    motion["faceRotation"] = serde_json::json!(offset + rotation);
                }
            },
        );
        if let Some(blur) = self.extra.get_mut("blur")
            && let Some(value) = blur.as_f64()
        {
            let scaled = value.clamp(0.0, MAX_BLUR) * f64::from(max_scale(basis));
            *blur = serde_json::json!(scaled.clamp(0.0, MAX_BLUR));
        }
        self.parent_space = None;
    }

    fn bake_components(&self, basis: Transform) -> Option<(f32, f32, f32)> {
        const EPSILON: f32 = 1e-5;
        let decompose = |axes: Transform| {
            let (sx, sy) = axis_scales(axes);
            let dot = (axes.sx / sx) * (axes.kx / sy) + (axes.ky / sx) * (axes.sy / sy);
            let det = axes.sx * axes.sy - axes.ky * axes.kx;
            (dot.abs() <= EPSILON && det > 0.0)
                .then(|| (sx, sy, axes.ky.atan2(axes.sx).to_degrees()))
        };
        let (sx, sy, rotation) = decompose(basis)?;
        if (sx - sy).abs() <= EPSILON * sx.max(sy) {
            return Some((sx, sy, rotation));
        }
        let angle = self.pose(0.0).r;
        let same_axis = |r: f32| angle_delta(2.0 * (r - angle)).abs() <= EPSILON;
        let mut rotation_keys = self
            .keys
            .iter()
            .filter(|k| !k.linked_space && k.sets(Property::Rotation))
            .peekable();
        while let Some(key) = rotation_keys.next() {
            if !same_axis(key.r)
                || rotation_keys.peek().is_some_and(|next| {
                    key.r != next.r && key.ease.as_deref().unwrap_or(&self.ease) != "hold"
                })
            {
                return None;
            }
        }
        if self
            .rotation_clips
            .iter()
            .any(|c| c.duration > 0.0 && c.degrees != 0.0)
            || crate::motion::motions(self).iter().any(|motion| {
                !motion.linked_space
                    && (motion.spin != 0.0
                        || !same_axis(motion.start_pose.r)
                        || motion
                            .base_keys
                            .iter()
                            .any(|k| !k.linked_space && k.sets(Property::Rotation) && k.r != angle))
            })
        {
            return None;
        }
        let own_rotation = angle + self.base_rotation;
        let axes = Transform::from_rotate(own_rotation).post_concat(basis);
        let (sx, sy, rotation) = decompose(axes)?;
        Some((sx, sy, angle_delta(rotation - own_rotation)))
    }

    pub(crate) fn resize_world_space(&mut self, sx: f32, sy: f32) {
        let Some(basis) = self.parent_space.map(from_matrix) else {
            return;
        };
        let linear = Transform {
            tx: 0.0,
            ty: 0.0,
            ..basis
        };
        // Move the anchors by the canvas ratios while retaining the basis of the artwork.
        // Convert the canvas scale back into key coordinates to leave the linear basis intact.
        if let Some(inverse) = linear.invert() {
            self.map_positions(linear.post_scale(sx, sy).post_concat(inverse));
        }
        self.parent_space = matrix(Transform {
            tx: basis.tx * sx,
            ty: basis.ty * sy,
            ..basis
        });
    }

    fn map_positions(&mut self, transform: Transform) {
        if let Some(pose) = &mut self.rest_pose {
            (pose.x, pose.y) = point(transform, pose.x, pose.y);
        }
        for key in self.keys.iter_mut().filter(|key| !key.linked_space) {
            (key.x, key.y) = point(transform, key.x, key.y);
        }
        self.map_motion_points(
            |p| {
                if let (Some(x), Some(y)) = (number(p, "x"), number(p, "y")) {
                    let (x, y) = point(transform, x, y);
                    p["x"] = serde_json::json!(x);
                    p["y"] = serde_json::json!(y);
                }
            },
            |_| {},
        );
    }

    fn map_motion_points(
        &mut self,
        mut map: impl FnMut(&mut Value),
        mut adjust_motion: impl FnMut(&mut Value),
    ) {
        for motion in self
            .extra
            .get_mut("motions")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
            .filter(|motion| !linked_space(motion))
        {
            adjust_motion(motion);
            if let Some(start) = motion.get_mut("startPose") {
                map(start);
            }
            for field in ["sourcePoints", "baseKeys"] {
                for p in motion
                    .get_mut(field)
                    .and_then(Value::as_array_mut)
                    .into_iter()
                    .flatten()
                    .filter(|p| !linked_space(p))
                {
                    map(p);
                }
            }
        }
    }

    pub(crate) fn translate_keys(&mut self, dx: f32, dy: f32) {
        if let Some(pose) = &mut self.rest_pose {
            (pose.x, pose.y) = (pose.x + dx, pose.y + dy);
        }
        for key in &mut self.keys {
            (key.x, key.y) = (key.x + dx, key.y + dy);
        }
        let nudge = |point: &mut Value| {
            for (axis, delta) in [("x", dx), ("y", dy)] {
                if let Some(value) = point.get(axis).and_then(Value::as_f64) {
                    point[axis] = serde_json::json!(value + f64::from(delta));
                }
            }
        };
        for motion in self
            .extra
            .get_mut("motions")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if let Some(start) = motion.get_mut("startPose") {
                nudge(start);
            }
            for key in ["sourcePoints", "baseKeys"] {
                for p in motion
                    .get_mut(key)
                    .and_then(Value::as_array_mut)
                    .into_iter()
                    .flatten()
                {
                    nudge(p);
                }
            }
        }
    }
}

fn linked_space(value: &Value) -> bool {
    value.get("linkedSpace").and_then(Value::as_bool) == Some(true)
}

fn number(value: &Value, field: &str) -> Option<f32> {
    value.get(field)?.as_f64().map(|v| v as f32)
}

fn map_number(value: &mut Value, field: &str, map: impl FnOnce(f32) -> f32) {
    if let Some(v) = number(value, field) {
        value[field] = serde_json::json!(map(v));
    }
}

pub(crate) fn resize_about_anchor(
    pose: Pose,
    anchor: Anchor,
    handle: (i8, i8),
    (dx, dy): (f32, f32),
    height_factor: f32,
    lock_ratio: bool,
) -> (f32, f32) {
    let x = if handle.0 == 0 {
        0.0
    } else {
        f32::from(handle.0) / 2.0 + 0.5 - anchor.x
    };
    let y = if handle.1 == 0 {
        0.0
    } else {
        f32::from(handle.1) * height_factor / 2.0 + 0.5 - anchor.y
    };
    let (w, h) = (pose.w, pose.h);
    if lock_ratio {
        let (a, b) = (x * w, y * h);
        let length_sq = a * a + b * b;
        if length_sq <= f32::EPSILON {
            return (w, h);
        }
        let scale = (1.0 + (a * dx + b * dy) / length_sq).clamp(
            (1.0 / w).max(1.0 / h),
            (4000.0 / w).min(4000.0 / h).max(1.0),
        );
        return (w * scale, h * scale);
    }
    let resized = |size: f32, delta: f32, factor: f32| {
        if factor.abs() < 0.0001 {
            size
        } else {
            (size + delta / factor).clamp(1.0, size.max(4000.0))
        }
    };
    (resized(w, dx, x), resized(h, dy, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyframes::Property;

    fn assert_near(a: (f32, f32), b: (f32, f32)) {
        assert!(
            (a.0 - b.0).abs() < 0.002 && (a.1 - b.1).abs() < 0.002,
            "{a:?} != {b:?}"
        );
    }

    fn scene() -> Scene {
        let mut scene = Scene::empty("hierarchy".into());
        for id in ["parent", "child", "grandchild"] {
            scene.add_shape(id.into(), "rect");
        }
        for (i, shape) in scene.shapes.iter_mut().enumerate() {
            let key = &mut shape.keys[0];
            (key.x, key.y, key.w, key.h) = (200.0 + 40.0 * i as f32, 200.0, 80.0, 40.0);
        }
        scene
    }

    #[test]
    fn nested_layers_follow_position_rotation_and_non_uniform_scale_without_inheriting_opacity() {
        let mut scene = scene();
        scene.set_parent(1, Some(0), 0.0).unwrap();
        scene.set_parent(2, Some(1), 0.0).unwrap();
        assert!(scene.set_parent(0, Some(2), 0.0).is_err());
        let parent = &mut scene.shapes[0];
        let mut pose = parent.pose(0.0);
        (pose.x, pose.y, pose.w, pose.h, pose.r, pose.o) = (300.0, 250.0, 160.0, 40.0, 90.0, 0.0);
        parent.out_time = Some(0.5);
        parent.edit_pose(2.0, pose);
        let child = scene.layer_frame(&scene.shapes[1], 2.0);
        let grandchild = scene.layer_frame(&scene.shapes[2], 2.0);
        assert_near(child.anchor(), (300.0, 330.0));
        assert_near(grandchild.anchor(), (300.0, 410.0));
        assert_near(point(grandchild.transform, 40.0, 20.0), (280.0, 490.0));
        assert_eq!((child.pose.o, grandchild.pose.o), (100.0, 100.0));
    }

    #[test]
    fn reparenting_clearing_and_deletion_preserve_all_corners_at_the_playhead() {
        let mut scene = scene();
        scene.shapes[1].keys[0].r = 35.0;
        scene.shapes[0].keys[0].w = 160.0;
        scene.shapes[0].keys[0].r = 76.0;
        let corner = |scene: &Scene, index| {
            point(
                scene.layer_frame(&scene.shapes[index], 1.0).transform,
                40.0,
                -20.0,
            )
        };
        let original = corner(&scene, 1);
        scene.set_parent(1, Some(0), 1.0).unwrap();
        assert_near(corner(&scene, 1), original);
        scene.set_parent(1, None, 1.0).unwrap();
        assert_near(corner(&scene, 1), original);
        scene.set_parent(1, Some(0), 1.0).unwrap();
        let parent = &mut scene.shapes[0];
        parent.keys[0].w = 322.0;
        parent.keys[0].r = -22.0;
        let seen = corner(&scene, 1);
        assert_eq!(scene.delete_layers(&["parent".into()], 1.0), 1);
        assert!(scene.shapes[0].parent.is_none());
        assert_near(corner(&scene, 0), seen);
    }

    #[test]
    fn clearing_follows_and_deleting_a_parent_bake_a_non_uniform_scale() {
        for (delete, angle, size) in [(false, 0.0, (160.0, 40.0)), (true, 90.0, (80.0, 80.0))] {
            let mut scene = scene();
            scene.shapes[1].keys[0].r = angle;
            scene.set_parent(1, Some(0), 0.0).unwrap();
            scene.shapes[0].keys[0].w *= 2.0;
            let before = scene.layer_frame(&scene.shapes[1], 0.0).box_transform();
            let index = if delete {
                scene.delete_layers(&["parent".into()], 0.0);
                0
            } else {
                scene.set_parent(1, None, 0.0).unwrap();
                1
            };
            let child = &scene.shapes[index];
            assert!(child.parent.is_none() && child.parent_space.is_none());
            let p = child.pose(0.0);
            assert_near((p.w, p.h), size);
            let after = scene.layer_frame(child, 0.0).box_transform();
            for (x, y) in [(0.5, 0.5), (-0.5, 0.5), (0.5, -0.5)] {
                assert_near(point(after, x, y), point(before, x, y));
            }
            scene.resize(600.0, 960.0);
            let p = scene.shapes[index].pose(0.0);
            assert_near((p.w, p.h), (size.0 * 0.625, size.1 * 0.625));
        }
    }

    #[test]
    fn unfollowing_bakes_blur_within_electrons_limit() {
        for (blur, expected) in [(40, 80.0), (60, 100.0)] {
            let mut scene = scene();
            scene.shapes[1].extra.insert("blur".into(), blur.into());
            scene.set_parent(1, Some(0), 0.0).unwrap();
            let parent = &mut scene.shapes[0].keys[0];
            parent.w *= 2.0;
            parent.h *= 2.0;

            scene.set_parent(1, None, 0.0).unwrap();
            let child = &scene.shapes[1];
            assert!(child.parent.is_none() && child.parent_space.is_none());
            assert_eq!(child.extra["blur"].as_f64(), Some(expected));

            let saved = serde_json::to_value(&scene).unwrap();
            // Electron's src/model/validate.js allows an absent blur or a finite number in [0, 100].
            for layer in saved["shapes"].as_array().unwrap() {
                if let Some(blur) = layer.get("blur") {
                    assert!(blur.as_f64().is_some_and(|v| (0.0..=100.0).contains(&v)));
                }
            }
            assert_eq!(saved["shapes"][1]["blur"].as_f64(), Some(expected));
        }
    }

    #[test]
    fn detaching_a_rotated_and_uniformly_scaled_parent_bakes_rest_and_motion_data() {
        use crate::motion::{self, Options, Point};

        let mut scene = scene();
        scene.set_anchor(1, Anchor { x: 0.2, y: 0.8 }, 0.0);
        let start = scene.shapes[1].pose(0.0);
        scene.shapes[1].rest_pose = Some(start);
        scene.shapes[1].keys[0].props = Some(vec![Property::Position, Property::Rotation]);
        scene.shapes[1].key_property(1.0, Property::Rotation, Pose { r: 18.0, ..start });
        motion::draw(
            &mut scene,
            1,
            "arrow",
            start,
            &[Point {
                x: start.x + 60.0,
                y: start.y + 20.0,
            }],
            0.0,
            Options {
                seconds: 2.0,
                spin: 45.0,
                face: true,
                ..Default::default()
            },
            "move".into(),
        )
        .unwrap();
        scene.set_parent(1, Some(0), 0.0).unwrap();
        let key = &mut scene.shapes[0].keys[0];
        (key.w, key.h, key.r) = (160.0, 80.0, 55.0);
        let mut detached = scene.clone();
        detached.set_parent(1, None, 0.0).unwrap();
        let child = &detached.shapes[1];
        assert!(child.parent_space.is_none());
        let rest = child.rest_pose.unwrap();
        assert_near((rest.w, rest.h), (160.0, 80.0));
        let before = &motion::motions(&scene.shapes[1])[0];
        let after = &motion::motions(child)[0];
        let space = scene.layer_frame(&scene.shapes[1], 0.0).space;
        for (a, b) in after.source_points.iter().zip(&before.source_points) {
            assert_near((a.x, a.y), point(space, b.x, b.y));
        }
        assert_near(
            (after.start_pose.x, after.start_pose.y),
            point(space, before.start_pose.x, before.start_pose.y),
        );
        assert_near((after.base_keys[1].r, after.start_pose.r), (73.0, 55.0));
        let compare = |a: &Scene, b: &Scene| {
            for t in [0.0, 0.5, 1.0, 1.5, 2.0] {
                let a = a.layer_frame(&a.shapes[1], t).box_transform();
                let b = b.layer_frame(&b.shapes[1], t).box_transform();
                for (x, y) in [(0.5, 0.5), (-0.5, 0.5), (0.5, -0.5)] {
                    assert_near(point(a, x, y), point(b, x, y));
                }
            }
        };
        compare(&scene, &detached);
        motion::remove(&mut scene, 1, "move");
        motion::remove(&mut detached, 1, "move");
        compare(&scene, &detached);
    }

    #[test]
    fn resizing_a_detached_sheared_layer_moves_its_anchor_and_preserves_its_shape() {
        let mut scene = scene();
        scene.set_parent(1, Some(0), 0.0).unwrap();
        scene.shapes[0].keys[0].w *= 2.0;
        scene.shapes[0].keys[0].r = 35.0;
        let pose = scene.shapes[1].pose(0.0);
        scene.shapes[1].key_property(2.0, Property::Rotation, Pose { r: 180.0, ..pose });
        scene.shapes[1].key_property(
            2.0,
            Property::Position,
            Pose {
                x: pose.x + 30.0,
                ..pose
            },
        );
        scene.set_parent(1, None, 1.0).unwrap();
        assert!(scene.shapes[1].parent_space.is_some());
        let old = scene.clone();
        scene.resize(600.0, 960.0);
        for t in [0.0, 0.5, 1.0, 2.0] {
            let before = old.layer_frame(&old.shapes[1], t);
            let after = scene.layer_frame(&scene.shapes[1], t);
            let (x, y) = before.anchor();
            assert_near(after.anchor(), (x * 0.625, y * 1.6));
            for (x, y) in [(1.0, 0.0), (0.0, 1.0)] {
                let (vx, vy) = vector(before.box_transform(), x, y);
                assert_near(
                    vector(after.box_transform(), x, y),
                    (vx * 0.625, vy * 0.625),
                );
            }
        }
    }

    #[test]
    fn inspector_edits_canvas_position_size_and_angle_through_a_parent() {
        let mut scene = scene();
        scene.set_parent(1, Some(0), 0.0).unwrap();
        let key = &mut scene.shapes[0].keys[0];
        (key.w, key.h, key.r) = (160.0, 40.0, 80.0);
        scene.shapes[1].base_rotation = 15.0;
        let initial = scene.inspector_pose(&scene.shapes[1], 1.0);
        let wanted = Pose {
            x: 310.0,
            y: 200.0,
            w: 1.0,
            h: 95.0,
            r: 455.0,
            ..initial
        };
        scene.set_inspector_pose(
            1,
            1.0,
            wanted,
            &[Property::Position, Property::Size, Property::Rotation],
        );
        let seen = scene.inspector_pose(&scene.shapes[1], 1.0);
        assert_near((seen.x, seen.y), (wanted.x, wanted.y));
        assert_near((seen.w, seen.h), (wanted.w, wanted.h));
        assert_near((seen.r, seen.o), (wanted.r, wanted.o));
        scene.set_inspector_pose(1, 1.0, Pose { r: 25.0, ..seen }, &[Property::Rotation]);
        let turned = scene.inspector_pose(&scene.shapes[1], 1.0);
        assert_near((turned.w, turned.h), (seen.w, seen.h));
        assert_near((turned.r, turned.o), (25.0, seen.o));
    }

    #[test]
    fn rotating_and_resizing_around_a_corner_keeps_that_corner_fixed() {
        let mut scene = scene();
        let corner = |scene: &Scene, t| {
            let f = scene.layer_frame(&scene.shapes[0], t);
            point(f.transform, -f.pose.w / 2.0, -f.pose.h / 2.0)
        };
        let first = corner(&scene, 0.0);
        scene.set_anchor(0, Anchor { x: 0.0, y: 0.0 }, 0.0);
        assert_near(corner(&scene, 0.0), first);
        let shape = &mut scene.shapes[0];
        let pose = shape.pose(0.0);
        shape.key_property(2.0, Property::Rotation, Pose { r: 90.0, ..pose });
        shape.key_property(
            2.0,
            Property::Size,
            Pose {
                w: 100.0,
                h: 50.0,
                ..pose
            },
        );
        assert_near(corner(&scene, 2.0), first);
        let at = scene.layer_frame(&scene.shapes[0], 2.0);
        assert_near(point(at.transform, 50.0, -25.0), (first.0, first.1 + 100.0));
        scene.set_anchor(0, Anchor::default(), 2.0);
        assert_near(corner(&scene, 2.0), first);
    }

    #[test]
    fn duplicates_follow_the_duplicated_parent_when_it_is_in_the_group() {
        let mut scene = scene();
        scene.set_parent(1, Some(0), 0.0).unwrap();
        scene.duplicate_layer(1, "lone-copy".into());
        assert_eq!(
            scene.layer("lone-copy").unwrap().parent.as_deref(),
            Some("parent")
        );
        scene.duplicate_layers(&[
            ("parent".into(), "new-parent".into()),
            ("child".into(), "new-child".into()),
        ]);
        let child = scene.layer("new-child").unwrap();
        assert_eq!(child.parent.as_deref(), Some("new-parent"));
        assert_near(scene.layer_frame(child, 0.0).anchor(), (260.0, 220.0));
    }

    #[test]
    fn legacy_linked_text_tracks_its_layers_keyed_and_parented_movement() {
        let mut scene = scene();
        scene.add_linked_text("caption".into(), 1);
        let text = scene.shapes[3].clone();
        scene.set_parent(1, Some(0), 0.0).unwrap();
        assert!(scene.set_parent(0, Some(3), 0.0).is_err());
        scene.shapes[0].keys[0].x += 100.0;
        let p = scene.shapes[1].pose(0.0);
        scene.shapes[1].key_property(2.0, Property::Position, Pose { x: p.x + 40.0, ..p });
        assert_near(scene.layer_frame(&text, 2.0).anchor(), (380.0, 138.0));
        assert_eq!(scene.shapes[3], text);
    }

    #[test]
    fn linked_text_can_key_an_offset_without_changing_its_link() {
        let mut scene = scene();
        scene.add_linked_text("caption".into(), 1);
        let initial = scene.render_pose(&scene.shapes[3], 0.0);
        let pose = scene.editable_pose(&scene.shapes[3], 2.0);
        scene.set_property_pose(
            3,
            2.0,
            Pose {
                x: pose.x + 30.0,
                ..pose
            },
            Property::Position,
        );
        assert_eq!(scene.render_pose(&scene.shapes[3], 0.0), initial);
        assert_eq!(scene.render_pose(&scene.shapes[3], 1.0).x, initial.x + 15.0);
        assert_eq!(scene.render_pose(&scene.shapes[3], 2.0).x, initial.x + 30.0);
        assert_eq!(scene.shapes[3].extra["linkId"], "child");
        assert_eq!(
            scene.shapes[3]
                .keys
                .iter()
                .filter(|k| !k.linked_space)
                .count(),
            1
        );
        scene.shapes[1].keys[0].x += 20.0;
        assert_eq!(scene.render_pose(&scene.shapes[3], 2.0).x, initial.x + 50.0);
    }
}
