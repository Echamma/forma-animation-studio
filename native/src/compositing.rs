use crate::keyframes::{Lane, LaneKey, same_time};
use crate::model::{Scene, Shape, ease_at, hundredths};
use crate::transform::LayerFrame;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tiny_skia::{Path, PathBuilder, Rect, Transform};

macro_rules! choices {
    ($name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub fn label(self) -> &'static str {
                match self { $(Self::$variant => $label),+ }
            }
        }
    };
}

choices!(Blend {
    Normal => "Normal", Multiply => "Multiply", Screen => "Screen", Overlay => "Overlay",
    Darken => "Darken", Lighten => "Lighten", Add => "Add", Difference => "Difference",
    ColorDodge => "Color dodge", ColorBurn => "Color burn", SoftLight => "Soft light",
    HardLight => "Hard light", Hue => "Hue", Saturation => "Saturation", Color => "Color",
    Luminosity => "Luminosity",
});

impl Blend {
    pub(crate) fn unused(value: &Option<Self>) -> bool {
        value.is_none_or(|blend| blend == Self::Normal)
    }

    pub(crate) fn skia(self) -> tiny_skia::BlendMode {
        match self {
            Self::Normal => tiny_skia::BlendMode::SourceOver,
            Self::Add => tiny_skia::BlendMode::Plus,
            Self::Multiply => tiny_skia::BlendMode::Multiply,
            Self::Screen => tiny_skia::BlendMode::Screen,
            Self::Overlay => tiny_skia::BlendMode::Overlay,
            Self::Darken => tiny_skia::BlendMode::Darken,
            Self::Lighten => tiny_skia::BlendMode::Lighten,
            Self::Difference => tiny_skia::BlendMode::Difference,
            Self::ColorDodge => tiny_skia::BlendMode::ColorDodge,
            Self::ColorBurn => tiny_skia::BlendMode::ColorBurn,
            Self::SoftLight => tiny_skia::BlendMode::SoftLight,
            Self::HardLight => tiny_skia::BlendMode::HardLight,
            Self::Hue => tiny_skia::BlendMode::Hue,
            Self::Saturation => tiny_skia::BlendMode::Saturation,
            Self::Color => tiny_skia::BlendMode::Color,
            Self::Luminosity => tiny_skia::BlendMode::Luminosity,
        }
    }

    pub(crate) fn svg(self) -> Option<&'static str> {
        Some(match self {
            Self::Normal => "normal",
            Self::Multiply => "multiply",
            Self::Screen => "screen",
            Self::Overlay => "overlay",
            Self::Darken => "darken",
            Self::Lighten => "lighten",
            Self::Difference => "difference",
            Self::ColorDodge => "color-dodge",
            Self::ColorBurn => "color-burn",
            Self::SoftLight => "soft-light",
            Self::HardLight => "hard-light",
            Self::Hue => "hue",
            Self::Saturation => "saturation",
            Self::Color => "color",
            Self::Luminosity => "luminosity",
            Self::Add => return None,
        })
    }
}

choices!(MaskMode {
    Add => "Add", Subtract => "Subtract", Intersect => "Intersect", Difference => "Difference",
});

choices!(MatteMode {
    Alpha => "Alpha", AlphaInverted => "Alpha inverted",
    Luma => "Luma", LumaInverted => "Luma inverted",
});

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackMatte {
    pub source: String,
    pub mode: MatteMode,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_layer: bool,
}

impl Scene {
    pub fn can_use_matte(&self, layer: &str, source: &str) -> bool {
        let mut next = Some(source);
        let mut seen = HashSet::new();
        while let Some(id) = next {
            if id == layer || !seen.insert(id) {
                return false;
            }
            let Some(shape) = self.shapes.iter().find(|shape| shape.id == id) else {
                return id != source;
            };
            next = shape
                .track_matte
                .as_ref()
                .map(|matte| matte.source.as_str());
        }
        true
    }

    pub fn set_matte(&mut self, layer: &str, matte: Option<TrackMatte>) -> bool {
        if matte
            .as_ref()
            .is_some_and(|m| !self.can_use_matte(layer, &m.source))
        {
            return false;
        }
        let Some(shape) = self.shapes.iter_mut().find(|shape| shape.id == layer) else {
            return false;
        };
        shape.track_matte = matte;
        true
    }

    pub(crate) fn prune_missing_mattes(&mut self) {
        let ids: HashSet<_> = self.shapes.iter().map(|s| s.id.clone()).collect();
        for shape in &mut self.shapes {
            if shape
                .track_matte
                .as_ref()
                .is_some_and(|m| !ids.contains(&m.source))
            {
                shape.track_matte = None;
            }
        }
    }
}

pub(crate) struct MatteLinks {
    pub sources: Vec<Option<usize>>,
    pub hidden: Vec<bool>,
}

impl MatteLinks {
    pub fn new(scene: &Scene) -> Option<Self> {
        if !scene.shapes.iter().any(|shape| shape.track_matte.is_some()) {
            return None;
        }
        let ids: HashMap<_, _> = scene
            .shapes
            .iter()
            .enumerate()
            .map(|(i, shape)| (shape.id.as_str(), i))
            .collect();
        let mut sources: Vec<_> = scene
            .shapes
            .iter()
            .map(|shape| {
                let matte = shape.track_matte.as_ref()?;
                ids.get(matte.source.as_str()).copied()
            })
            .collect();
        // Public scene data can also come from callers that bypass Scene::read.
        let mut visited = vec![None; sources.len()];
        for start in 0..sources.len() {
            let mut at = start;
            while let Some(next) = sources[at] {
                if visited[at] == Some(start) {
                    sources[at] = None;
                    break;
                }
                if visited[at].is_some() {
                    break;
                }
                visited[at] = Some(start);
                at = next;
            }
        }
        let mut hidden = vec![false; sources.len()];
        let mut shown = vec![false; sources.len()];
        for (shape, source) in scene.shapes.iter().zip(sources.iter().copied()) {
            if let (Some(matte), Some(source)) = (&shape.track_matte, source) {
                hidden[source] = true;
                shown[source] |= matte.show_layer;
            }
        }
        for (hide, show) in hidden.iter_mut().zip(shown) {
            *hide &= !show;
        }
        Some(Self { sources, hidden })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MaskProperty {
    Position,
    Size,
    Rotation,
    Feather,
    Expansion,
    Opacity,
}

impl MaskProperty {
    pub const ALL: [Self; 6] = [
        Self::Position,
        Self::Size,
        Self::Rotation,
        Self::Feather,
        Self::Expansion,
        Self::Opacity,
    ];

    pub fn label(self) -> &'static str {
        self.labels().0
    }

    pub fn channels(self) -> &'static [&'static str] {
        self.labels().1
    }

    fn labels(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Position => ("Position", &["X", "Y"]),
            Self::Size => ("Size", &["Width", "Height"]),
            Self::Rotation => ("Rotation", &["Angle"]),
            Self::Feather => ("Feather", &["Feather"]),
            Self::Expansion => ("Expansion", &["Expansion"]),
            Self::Opacity => ("Opacity", &["Opacity"]),
        }
    }

    pub fn limits(self) -> std::ops::RangeInclusive<f32> {
        match self {
            Self::Position => -100_000.0..=100_000.0,
            Self::Size => 0.01..=100_000.0,
            Self::Rotation => -10_000.0..=10_000.0,
            Self::Feather => 0.0..=500.0,
            Self::Expansion => -1000.0..=1000.0,
            Self::Opacity => 0.0..=100.0,
        }
    }

    pub(crate) fn clamp(self, value: f32) -> f32 {
        let limits = self.limits();
        if value.is_nan() {
            return 0.0f32.clamp(*limits.start(), *limits.end());
        }
        value.clamp(*limits.start(), *limits.end())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskPose {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub rotation: f32,
    pub feather: f32,
    pub expansion: f32,
    pub opacity: f32,
}

impl MaskPose {
    pub fn new(position: [f32; 2], size: [f32; 2]) -> Self {
        Self {
            position,
            size,
            rotation: 0.0,
            feather: 0.0,
            expansion: 0.0,
            opacity: 100.0,
        }
    }

    pub fn values(self, property: MaskProperty) -> [f32; 2] {
        match property {
            MaskProperty::Position => self.position,
            MaskProperty::Size => self.size,
            MaskProperty::Rotation => [self.rotation, 0.0],
            MaskProperty::Feather => [self.feather, 0.0],
            MaskProperty::Expansion => [self.expansion, 0.0],
            MaskProperty::Opacity => [self.opacity, 0.0],
        }
    }

    pub fn set(&mut self, property: MaskProperty, values: [f32; 2]) {
        let [a, b] = values.map(|v| property.clamp(v));
        match property {
            MaskProperty::Position => self.position = [a, b],
            MaskProperty::Size => self.size = [a, b],
            MaskProperty::Rotation => self.rotation = a,
            MaskProperty::Feather => self.feather = a,
            MaskProperty::Expansion => self.expansion = a,
            MaskProperty::Opacity => self.opacity = a,
        }
    }

    pub(crate) fn placement(self) -> Transform {
        Transform::from_rotate(self.rotation).post_translate(self.position[0], self.position[1])
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskKey {
    pub t: f64,
    pub property: MaskProperty,
    pub values: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<[f32; 4]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum MaskShape {
    Rectangle,
    Ellipse,
    Path { points: Vec<MaskPoint> },
}

/// Points and handle offsets are relative to the mask's width and height, about its center.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskPoint {
    pub point: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incoming: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outgoing: Option<[f32; 2]>,
}

impl MaskShape {
    pub(crate) fn outline(&self, size: [f32; 2]) -> Option<Path> {
        let [w, h] = size;
        let mut path = PathBuilder::new();
        let bounds = Rect::from_ltrb(-w / 2.0, -h / 2.0, w / 2.0, h / 2.0)?;
        match self {
            Self::Rectangle => path.push_rect(bounds),
            Self::Ellipse => path.push_oval(bounds),
            Self::Path { points } => {
                let (first, rest) = points.split_first()?;
                if rest.len() < 2 {
                    return None;
                }
                path.move_to(first.point[0] * w, first.point[1] * h);
                for (a, b) in points.iter().zip(rest.iter().chain([first])) {
                    let [x, y] = b.point;
                    if a.outgoing.is_none() && b.incoming.is_none() {
                        path.line_to(x * w, y * h);
                    } else {
                        let [ax, ay] = a.outgoing.unwrap_or_default();
                        let [bx, by] = b.incoming.unwrap_or_default();
                        path.cubic_to(
                            (a.point[0] + ax) * w,
                            (a.point[1] + ay) * h,
                            (x + bx) * w,
                            (y + by) * h,
                            x * w,
                            y * h,
                        );
                    }
                }
                path.close();
            }
        }
        path.finish()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerMask {
    pub id: u64,
    pub shape: MaskShape,
    pub mode: MaskMode,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inverted: bool,
    // Layer size at creation, so coordinates stay in authored pixels as the layer scales.
    pub reference_size: [f32; 2],
    pub rest: MaskPose,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<MaskKey>,
}

impl LayerMask {
    pub(crate) fn prepare(&mut self) -> bool {
        let usable = self
            .reference_size
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0001)
            && MaskProperty::ALL
                .iter()
                .all(|&property| self.rest.values(property).iter().all(|v| v.is_finite()));
        let path_usable = match &self.shape {
            MaskShape::Path { points } => {
                points.len() >= 3
                    && points.iter().all(|point| {
                        [Some(point.point), point.incoming, point.outgoing]
                            .into_iter()
                            .flatten()
                            .flatten()
                            .all(f32::is_finite)
                    })
            }
            _ => true,
        };
        if !usable || !path_usable {
            return false;
        }
        self.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        let mut last = HashMap::new();
        self.keys.iter_mut().all(|key| {
            if !key.t.is_finite() || key.t < 0.0 || !key.values.iter().all(|v| v.is_finite()) {
                return false;
            }
            key.values = key.values.map(|v| key.property.clamp(v));
            last.insert(key.property, key.t)
                .is_none_or(|time| !same_time(time, key.t))
        })
    }

    pub fn space(&self, frame: LayerFrame) -> Transform {
        Transform::from_scale(
            frame.pose.w / self.reference_size[0].max(0.0001),
            frame.pose.h / self.reference_size[1].max(0.0001),
        )
        .post_concat(frame.transform)
    }

    pub fn pose(&self, t: f64, ease: &str) -> MaskPose {
        let mut pose = self.rest;
        for property in MaskProperty::ALL {
            pose.set(property, self.values(property, t, ease));
        }
        pose
    }

    pub(crate) fn values(&self, property: MaskProperty, t: f64, ease: &str) -> [f32; 2] {
        let keys = self.keys.iter().filter(|key| key.property == property);
        let Some(first) = keys.clone().next() else {
            return self.rest.values(property);
        };
        let before = keys.clone().rev().find(|key| key.t <= t).unwrap_or(first);
        let Some(after) = keys.clone().find(|key| key.t >= t) else {
            return before.values;
        };
        if after.t <= before.t {
            return before.values;
        }
        let fraction = ease_at(
            before.ease.as_deref().unwrap_or(ease),
            ((t - before.t) / (after.t - before.t)).clamp(0.0, 1.0) as f32,
            before.curve,
        );
        std::array::from_fn(|i| before.values[i] + (after.values[i] - before.values[i]) * fraction)
    }

    pub(crate) fn put_key(&mut self, mut key: MaskKey, t: f64) {
        self.keys
            .retain(|k| k.property != key.property || !same_time(k.t, t));
        key.t = t;
        let i = self.keys.partition_point(|k| k.t <= t);
        self.keys.insert(i, key);
    }
}

impl Shape {
    pub fn blend(&self) -> Blend {
        self.blend_mode.unwrap_or(Blend::Normal)
    }

    pub fn mask(&self, id: u64) -> Option<&LayerMask> {
        self.masks.iter().find(|mask| mask.id == id)
    }

    pub fn mask_mut(&mut self, id: u64) -> Option<&mut LayerMask> {
        self.masks.iter_mut().find(|mask| mask.id == id)
    }

    pub(crate) fn mask_times(&self) -> impl Iterator<Item = f64> + '_ {
        self.masks
            .iter()
            .flat_map(|mask| mask.keys.iter().map(|key| key.t))
    }

    pub(crate) fn freeze_masks(&mut self, t: f64) {
        for mask in &mut self.masks {
            mask.rest = mask.pose(t, &self.ease);
            mask.keys.clear();
        }
    }

    pub fn edit_mask(&mut self, id: u64, property: MaskProperty, t: f64, values: [f32; 2]) {
        if !t.is_finite() || t < 0.0 || !values.iter().all(|v| v.is_finite()) {
            return;
        }
        let t = hundredths(t);
        let lane = Lane::Mask(id, property);
        let Some(mask) = self.mask(id) else { return };
        let values = values.map(|v| property.clamp(v));
        if !mask.keys.iter().any(|key| key.property == property) {
            if let Some(mask) = self.mask_mut(id) {
                mask.rest.set(property, values);
            }
            return;
        }
        self.add_lane_key(lane, t);
        if let Some(LaneKey::Mask(id, mut key)) = self.lane_key(lane, t) {
            key.values = values;
            self.put_lane_key(t, LaneKey::Mask(id, key));
        }
    }
}
