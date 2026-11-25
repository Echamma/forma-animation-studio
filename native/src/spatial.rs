use crate::keyframes::Lane;
use crate::model::{Scene, Shape};
use crate::transform::{LayerFrame, point};
use crate::value_track::ValueTrack;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Property {
    Depth,
    RotationX,
    RotationY,
    Orientation,
    Interest,
    FieldOfView,
    FocusDistance,
    Aperture,
}

impl Property {
    pub const LAYER: [Self; 4] = [
        Self::Depth,
        Self::RotationX,
        Self::RotationY,
        Self::Orientation,
    ];
    pub const CAMERA: [Self; 8] = [
        Self::Depth,
        Self::RotationX,
        Self::RotationY,
        Self::Orientation,
        Self::Interest,
        Self::FieldOfView,
        Self::FocusDistance,
        Self::Aperture,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Depth => "Z position",
            Self::RotationX => "X rotation",
            Self::RotationY => "Y rotation",
            Self::Orientation => "Orientation",
            Self::Interest => "Point of interest",
            Self::FieldOfView => "Field of view",
            Self::FocusDistance => "Focus distance",
            Self::Aperture => "Aperture",
        }
    }

    pub fn channels(self) -> &'static [&'static str] {
        match self {
            Self::Orientation | Self::Interest => &["X", "Y", "Z"],
            _ => &["Value"],
        }
    }

    pub(crate) fn clamp(self, value: f32) -> f32 {
        match self {
            Self::FieldOfView => value.clamp(1.0, 170.0),
            Self::FocusDistance => value.clamp(1.0, 1_000_000.0),
            Self::Aperture => value.clamp(0.0, 100.0),
            Self::RotationX | Self::RotationY | Self::Orientation => {
                value.clamp(-360_000.0, 360_000.0)
            }
            _ => value.clamp(-1_000_000.0, 1_000_000.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spatial {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub one_node: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub depth_of_field: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<Property, ValueTrack>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Default for Spatial {
    fn default() -> Self {
        Self {
            enabled: true,
            one_node: false,
            depth_of_field: false,
            tracks: BTreeMap::new(),
        }
    }
}

impl Spatial {
    pub(crate) fn resize(&mut self, [sx, sy]: [f32; 2], sz: f32) {
        for (&property, track) in &mut self.tracks {
            let factor = match property {
                Property::Interest => [sx, sy, sz],
                Property::Depth | Property::FocusDistance => [sz, 1.0, 1.0],
                _ => continue,
            };
            for value in std::iter::once(&mut track.rest)
                .chain(track.keys.iter_mut().map(|key| &mut key.value))
            {
                for (value, factor) in value.iter_mut().zip(factor) {
                    *value = property.clamp(*value * factor);
                }
            }
        }
    }

    pub(crate) fn prepare(&mut self, duration: f64) -> bool {
        self.tracks.iter_mut().all(|(_, track)| {
            track.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
            track.rest.iter().all(|v| v.is_finite())
                && track
                    .keys
                    .windows(2)
                    .all(|keys| !crate::keyframes::same_time(keys[0].t, keys[1].t))
                && track.keys.iter().all(|key| {
                    key.t.is_finite()
                        && key.t >= 0.0
                        && key.t <= duration + 0.001
                        && key.value.iter().all(|v| v.is_finite())
                        && key.ease.as_deref().is_none_or(|ease| {
                            matches!(
                                ease,
                                "smooth" | "linear" | "hold" | "ease-in" | "ease-out" | "custom"
                            )
                        })
                        && key.curve.is_none_or(|[x0, y0, x1, y1]| {
                            [x0, x1].iter().all(|x| (0.0..=1.0).contains(x))
                                && [y0, y1].iter().all(|y| (-4.0..=5.0).contains(y))
                        })
                })
        })
    }
}

impl Shape {
    pub fn is_camera(&self) -> bool {
        self.kind == "camera"
    }

    pub fn is_3d(&self) -> bool {
        self.is_camera() || self.spatial.as_ref().is_some_and(|spatial| spatial.enabled)
    }

    pub fn spatial_lanes(&self) -> impl Iterator<Item = Lane> + '_ {
        let properties = if self.is_camera() {
            &Property::CAMERA[..]
        } else if self.is_3d() {
            &Property::LAYER[..]
        } else {
            &[]
        };
        properties.iter().copied().map(Lane::Spatial)
    }

    pub(crate) fn spatial_times(&self) -> impl Iterator<Item = f64> + '_ {
        self.spatial
            .iter()
            .flat_map(|s| s.tracks.values())
            .flat_map(|track| track.keys.iter().map(|key| key.t))
    }

    pub(crate) fn freeze_spatial(&mut self, at: f64) {
        if let Some(spatial) = &mut self.spatial {
            for (&property, track) in &mut spatial.tracks {
                track.rest = track.sample(at, &self.ease).map(|v| property.clamp(v));
                track.keys.clear();
            }
        }
    }

    pub(crate) fn spatial_value(&self, property: Property, time: f64) -> [f32; 3] {
        let rest = match property {
            Property::FieldOfView => [60.0, 0.0, 0.0],
            Property::FocusDistance => [1_000.0, 0.0, 0.0],
            Property::Aperture => [12.0, 0.0, 0.0],
            _ => [0.0; 3],
        };
        self.spatial
            .as_ref()
            .and_then(|s| s.tracks.get(&property))
            .map_or(rest, |track| track.sample(time, &self.ease))
            .map(|v| property.clamp(v))
    }

    pub(crate) fn set_spatial(&mut self, property: Property, values: [f32; 3], time: f64) {
        if !values.iter().all(|v| v.is_finite()) || !time.is_finite() {
            return;
        }
        let values = values.map(|v| property.clamp(v));
        let track = self
            .spatial
            .get_or_insert_with(Spatial::default)
            .tracks
            .entry(property)
            .or_insert_with(|| ValueTrack::new(values));
        if track.keys.is_empty() {
            track.rest = values;
        } else {
            let at = crate::model::hundredths(time);
            let mut key = track
                .key(at)
                .cloned()
                .unwrap_or_else(|| crate::value_track::ValueKey::new(at, values));
            key.value = values;
            track.insert(at, key);
        }
    }
}

impl Scene {
    pub fn add_camera(&mut self, id: String) -> Result<String, String> {
        if self.shapes.iter().any(Shape::is_camera) {
            return Err("This composition already has a camera.".into());
        }
        let id = self.add_shape(id, "rect");
        let distance = focal(self.height, 60.0);
        if let Some(shape) = self.shapes.iter_mut().find(|shape| shape.id == id) {
            shape.kind = "camera".into();
            shape.name = "Camera".into();
            if let Some(key) = shape.keys.first_mut() {
                (key.x, key.y) = (self.width * 0.5, self.height * 0.5);
            }
            shape.set_spatial(Property::Depth, [-distance, 0.0, 0.0], 0.0);
            shape.set_spatial(
                Property::Interest,
                [self.width * 0.5, self.height * 0.5, 0.0],
                0.0,
            );
            shape.set_spatial(Property::FocusDistance, [distance, 0.0, 0.0], 0.0);
        }
        Ok(id)
    }
    pub(crate) fn move_camera(
        &mut self,
        id: &str,
        time: f64,
        [x, y]: [f32; 2],
        pan: bool,
        scale: f32,
    ) {
        let Some(index) = self
            .shapes
            .iter()
            .position(|shape| shape.id == id && shape.is_camera())
        else {
            return;
        };
        let shape = &self.shapes[index];
        let camera = Camera::at(self, time);
        let frame = self.layer_frame(shape, time);
        let basis = |v: Vec3| followed(self, shape, frame, time, v);
        let mut eye = shape.lane_pose(time);
        let mut z = shape.spatial_value(Property::Depth, time);
        let mut target = shape.spatial_value(Property::Interest, time);
        let one_node = shape.spatial.as_ref().is_some_and(|s| s.one_node);
        let rotation = angles(shape, time);
        let mut orbit = None;
        if pan {
            let distance = if one_node {
                camera.focus
            } else {
                camera.depth(basis(target)).max(1.0)
            };
            let units = distance / (camera.focal * scale.max(0.01));
            let world_delta = add(
                crate::spatial::scale(camera.right, -x * units),
                crate::spatial::scale(camera.down, -y * units),
            );
            let Some([dx, dy, dz]) = camera_local_delta(basis, world_delta) else {
                return;
            };
            eye.x += dx;
            eye.y += dy;
            z[0] += dz;
            target = add(target, [dx, dy, dz]);
        } else {
            let pivot = if one_node {
                let focus = shape.spatial_value(Property::FocusDistance, time)[0];
                add(
                    [eye.x, eye.y, z[0]],
                    crate::spatial::scale(rotate([0.0, 0.0, 1.0], rotation), focus),
                )
            } else {
                target
            };
            let turn = [y * 0.3, -x * 0.3, 0.0];
            let [px, py, pz] = add(pivot, rotate(sub([eye.x, eye.y, z[0]], pivot), turn));
            (eye.x, eye.y, z[0]) = (px, py, pz);
            if one_node {
                orbit = Some([rotation[0] + turn[0], rotation[1] + turn[1], 0.0]);
            }
        }
        let shape = &mut self.shapes[index];
        if let Some(rotation) = orbit {
            shape.set_spatial(Property::RotationX, [rotation[0], 0.0, 0.0], time);
            shape.set_spatial(Property::RotationY, [rotation[1], 0.0, 0.0], time);
        }
        shape.key_property(time, crate::keyframes::Property::Position, eye);
        shape.set_spatial(Property::Depth, z, time);
        if pan && !one_node {
            shape.set_spatial(Property::Interest, target, time);
        }
    }
}

pub(crate) type Vec3 = [f32; 3];
fn add(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|i| a[i] - b[i])
}
fn scale(a: Vec3, k: f32) -> Vec3 {
    a.map(|v| v * k)
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}
fn cross([x, y, z]: Vec3, [a, b, c]: Vec3) -> Vec3 {
    [y * c - z * b, z * a - x * c, x * b - y * a]
}
fn unit(v: Vec3) -> Option<Vec3> {
    let n = dot(v, v).sqrt();
    (n.is_finite() && n > 0.0001).then(|| scale(v, n.recip()))
}
fn rotate([x, y, z]: Vec3, [a, b, c]: Vec3) -> Vec3 {
    let (sa, ca) = a.to_radians().sin_cos();
    let (sb, cb) = b.to_radians().sin_cos();
    let (sc, cc) = c.to_radians().sin_cos();
    let (y, z) = (y * ca - z * sa, y * sa + z * ca);
    let (x, z) = (x * cb + z * sb, -x * sb + z * cb);
    [x * cc - y * sc, x * sc + y * cc, z]
}

pub(crate) fn focal(height: f32, fov: f32) -> f32 {
    height * 0.5 / (fov.to_radians() * 0.5).tan()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Camera {
    eye: Vec3,
    right: Vec3,
    down: Vec3,
    forward: Vec3,
    center: [f32; 2],
    focal: f32,
    focus: f32,
    aperture: f32,
}

fn angles(shape: &Shape, time: f64) -> Vec3 {
    [
        shape.spatial_value(Property::RotationX, time)[0],
        shape.spatial_value(Property::RotationY, time)[0],
        0.0,
    ]
}

impl Camera {
    pub(crate) fn at(scene: &Scene, time: f64) -> Self {
        let center = [scene.width * 0.5, scene.height * 0.5];
        let nominal = focal(scene.height, 60.0);
        let mut camera = Self {
            eye: [center[0], center[1], -nominal],
            right: [1.0, 0.0, 0.0],
            down: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, 1.0],
            center,
            focal: nominal,
            focus: nominal,
            aperture: 0.0,
        };
        let Some(shape) = scene
            .shapes
            .iter()
            .rev()
            .find(|shape| shape.is_camera() && shape.visible(time, scene.duration))
        else {
            return camera;
        };
        let frame = scene.layer_frame(shape, time);
        let z = shape.spatial_value(Property::Depth, time)[0];
        camera.eye = followed(scene, shape, frame, time, [frame.pose.x, frame.pose.y, z]);
        camera.focal = focal(
            scene.height,
            shape.spatial_value(Property::FieldOfView, time)[0],
        );
        let mut rotation = angles(shape, time);
        let (sx, sy) = crate::transform::vector(frame.transform, 1.0, 0.0);
        rotation[2] = sy.atan2(sx).to_degrees();
        let orientation = shape.spatial_value(Property::Orientation, time);
        let turn = |v| rotate(rotate(v, orientation), rotation);
        let target = shape.spatial_value(Property::Interest, time);
        let target = followed(scene, shape, frame, time, target);
        let one_node = shape.spatial.as_ref().is_some_and(|s| s.one_node);
        let forward = if one_node {
            None
        } else {
            unit(sub(target, camera.eye))
        }
        .unwrap_or(camera.forward);
        let right = unit(cross([0.0, 1.0, 0.0], forward))
            .or_else(|| unit(cross([0.0, 0.0, 1.0], forward)))
            .unwrap_or(camera.right);
        let down = cross(forward, right);
        let world = |[a, b, c]: Vec3| add(add(scale(right, a), scale(down, b)), scale(forward, c));
        camera.right = world(turn([1.0, 0.0, 0.0]));
        camera.down = world(turn([0.0, 1.0, 0.0]));
        camera.forward = world(turn([0.0, 0.0, 1.0]));
        camera.focus = shape.spatial_value(Property::FocusDistance, time)[0];
        if shape.spatial.as_ref().is_some_and(|s| s.depth_of_field) {
            camera.aperture = shape.spatial_value(Property::Aperture, time)[0];
        }
        camera
    }

    pub(crate) fn depth(self, p: Vec3) -> f32 {
        dot(sub(p, self.eye), self.forward)
    }

    pub(crate) fn homogeneous(self, p: Vec3) -> Vec3 {
        let v = sub(p, self.eye);
        let depth = dot(v, self.forward);
        [
            self.center[0] * depth + dot(v, self.right) * self.focal,
            self.center[1] * depth + dot(v, self.down) * self.focal,
            depth,
        ]
    }

    pub(crate) fn project(self, p: Vec3) -> Option<[f32; 2]> {
        let [x, y, depth] = self.homogeneous(p);
        let xy = [x / depth, y / depth];
        (depth > 0.5 && xy.iter().all(|v| v.is_finite())).then_some(xy)
    }

    pub(crate) fn blur(self, depth: f32) -> f32 {
        (self.aperture * (depth - self.focus).abs() / self.focus.max(1.0)).clamp(0.0, 100.0)
    }
}

fn followed(scene: &Scene, shape: &Shape, frame: LayerFrame, time: f64, [x, y, z]: Vec3) -> Vec3 {
    let (x, y) = point(frame.space, x, y);
    follow_point(scene, shape, time, [x, y, z])
}

fn camera_local_delta(world: impl Fn(Vec3) -> Vec3, delta: Vec3) -> Option<Vec3> {
    let origin = world([0.0; 3]);
    let [x, y, z] =
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|axis| sub(world(axis), origin));
    let det = dot(x, cross(y, z));
    if !det.is_finite() || det.abs() < 1e-6 {
        return None;
    }
    Some(
        [
            dot(delta, cross(y, z)),
            dot(delta, cross(z, x)),
            dot(delta, cross(x, y)),
        ]
        .map(|v| v / det),
    )
}

fn follow_point(scene: &Scene, shape: &Shape, time: f64, mut p: Vec3) -> Vec3 {
    let mut parent = shape.parent.as_deref();
    for _ in 0..32 {
        let Some(layer) = parent.and_then(|id| {
            scene
                .shapes
                .iter()
                .find(|other| other.id == id && other.id != shape.id)
        }) else {
            break;
        };
        if layer.is_3d() {
            let (x, y) = scene.layer_frame(layer, time).anchor();
            let anchor = [x, y, 0.0];
            p = add(
                rotate(
                    rotate(
                        sub(p, anchor),
                        layer.spatial_value(Property::Orientation, time),
                    ),
                    angles(layer, time),
                ),
                anchor,
            );
            p[2] += layer.spatial_value(Property::Depth, time)[0];
        }
        parent = layer.parent.as_deref();
    }
    p
}

#[derive(Clone, Copy)]
pub(crate) struct Plane<'a> {
    pub(crate) scene: &'a Scene,
    pub(crate) shape: &'a Shape,
    pub(crate) frame: LayerFrame,
    pub(crate) time: f64,
}

impl Plane<'_> {
    pub(crate) fn world(self, [x, y, z]: Vec3) -> Vec3 {
        let (ax, ay) = self.frame.anchor();
        // Undo the local Z turn before tilting, then put it back in the plane's Euler order.
        let unrotate = self.frame.transform.invert();
        let (u, v) = unrotate.map_or((0.0, 0.0), |inverse| point(inverse, x, y));
        let (ox, oy) = unrotate.map_or((0.0, 0.0), |inverse| point(inverse, ax, ay));
        let [u, v, z] = rotate(
            rotate(
                [u - ox, v - oy, z],
                self.shape.spatial_value(Property::Orientation, self.time),
            ),
            angles(self.shape, self.time),
        );
        let (x, y) = point(self.frame.transform, u + ox, v + oy);
        let z = z + self.shape.spatial_value(Property::Depth, self.time)[0];
        follow_point(self.scene, self.shape, self.time, [x, y, z])
    }

    pub(crate) fn center(self) -> Vec3 {
        let (x, y) = point(self.frame.transform, 0.0, 0.0);
        self.world([x, y, 0.0])
    }
}

pub(crate) fn depth_order(depths: &[Option<f32>]) -> Vec<usize> {
    let mut sorted: Vec<_> = depths
        .iter()
        .enumerate()
        .filter_map(|(i, d)| d.map(|d| (i, d)))
        .collect();
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut layers = sorted.into_iter();
    depths
        .iter()
        .enumerate()
        .map(|(i, depth)| {
            if depth.is_some() {
                layers.next().map_or(i, |(i, _)| i)
            } else {
                i
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Scene {
        let mut scene = Scene::empty("spatial".into());
        (scene.width, scene.height) = (400.0, 300.0);
        scene.add_camera("camera".into()).unwrap();
        scene
    }

    fn near(a: [f32; 2], b: [f32; 2]) {
        assert!(
            a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < 0.001),
            "{a:?} vs {b:?}"
        );
    }

    #[test]
    fn camera_projects_a_known_tilted_quad_and_preserves_the_default_plane() {
        let mut scene = setup();
        scene.add_shape("card".into(), "rect");
        let shape = &mut scene.shapes[1];
        (
            shape.keys[0].x,
            shape.keys[0].y,
            shape.keys[0].w,
            shape.keys[0].h,
        ) = (200.0, 150.0, 100.0, 60.0);
        shape.set_spatial(Property::RotationY, [60.0, 0.0, 0.0], 0.0);
        let shape = &scene.shapes[1];
        let plane = Plane {
            scene: &scene,
            shape,
            frame: scene.layer_frame(shape, 0.0),
            time: 0.0,
        };
        let camera = Camera::at(&scene, 0.0);
        near(camera.project([80.0, 170.0, 0.0]).unwrap(), [80.0, 170.0]);
        for (x, y) in [
            (150.0, 120.0),
            (250.0, 120.0),
            (250.0, 180.0),
            (150.0, 180.0),
        ] {
            let dx = x - 200.0;
            let focal = 150.0 * 3.0_f32.sqrt();
            let gain = focal / (focal - dx * 3.0_f32.sqrt() * 0.5);
            near(
                camera.project(plane.world([x, y, 0.0])).unwrap(),
                [200.0 + dx * 0.5 * gain, 150.0 + (y - 150.0) * gain],
            );
        }
    }

    #[test]
    fn field_of_view_and_focus_change_perspective_and_blur() {
        let mut scene = setup();
        let base = Camera::at(&scene, 0.0);
        let camera = &mut scene.shapes[0];
        camera.set_spatial(Property::FieldOfView, [90.0, 0.0, 0.0], 0.0);
        camera.spatial.as_mut().unwrap().depth_of_field = true;
        let wide = Camera::at(&scene, 0.0);
        let point = [260.0, 150.0, 0.0];
        near(
            wide.project(point).unwrap(),
            [
                200.0 + 60.0 * focal(300.0, 90.0) / focal(300.0, 60.0),
                150.0,
            ],
        );
        assert!(wide.project(point).unwrap()[0] < base.project(point).unwrap()[0]);
        let depth = wide.depth(point);
        assert_eq!(wide.blur(depth), 0.0);
        assert!(wide.blur(depth + 100.0) > wide.blur(depth + 20.0));
        assert!(wide.blur(depth - 100.0) > wide.blur(depth - 20.0));
    }

    #[test]
    fn painter_sorts_farther_3d_layers_first_and_preserves_flat_slots() {
        assert_eq!(
            depth_order(&[Some(10.0), None, Some(20.0), Some(20.0), None]),
            [2, 1, 3, 0, 4]
        );
    }

    #[test]
    fn camera_orbit_and_pan_key_the_camera_and_it_can_follow_a_layer() {
        let mut scene = setup();
        scene.add_shape("target".into(), "rect");
        scene.set_parent(0, Some(1), 0.0).unwrap();
        let p = [200.0, 150.0, 0.0];
        near(Camera::at(&scene, 0.0).project(p).unwrap(), [200.0, 150.0]);
        let target = &mut scene.shapes[1];
        let prior = target.pose(0.0);
        target.key_property(
            1.0,
            crate::keyframes::Property::Position,
            crate::model::Pose {
                x: prior.x + 40.0,
                ..prior
            },
        );
        near(Camera::at(&scene, 1.0).project(p).unwrap(), [160.0, 150.0]);
        scene.move_camera("camera", 1.0, [20.0, 0.0], false, 1.0);
        assert_ne!(Camera::at(&scene, 1.0).eye, Camera::at(&scene, 0.0).eye);
        let before = Camera::at(&scene, 1.0).eye;
        scene.move_camera("camera", 1.0, [3.0, 5.0], true, 1.0);
        assert_ne!(Camera::at(&scene, 1.0).eye, before);
    }
}
