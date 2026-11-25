use crate::compositing::{MaskKey, MaskProperty};
use crate::model::{ColorKey, Key, Pose, Shape, ease_at, hundredths, is_linked};
use crate::shape_style::{VectorKey, VectorProperty, VectorValue};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Property {
    Position,
    Size,
    Rotation,
    Opacity,
}

impl Property {
    pub const ALL: [Self; 4] = [Self::Position, Self::Size, Self::Rotation, Self::Opacity];

    pub fn values(self, pose: Pose) -> [f32; 2] {
        match self {
            Self::Position => [pose.x, pose.y],
            Self::Size => [pose.w, pose.h],
            Self::Rotation => [pose.r, 0.0],
            Self::Opacity => [pose.o, 0.0],
        }
    }

    pub fn set(self, pose: &mut Pose, [a, b]: [f32; 2]) {
        match self {
            Self::Position => (pose.x, pose.y) = (a, b),
            Self::Size => (pose.w, pose.h) = (a.max(1.0), b.max(1.0)),
            Self::Rotation => pose.r = a,
            Self::Opacity => pose.o = a.clamp(0.0, 100.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lane {
    Pose(Property),
    Color,
    Mask(u64, MaskProperty),
    Vector(VectorProperty),
}

impl Lane {
    pub const ALL: [Self; 5] = [
        Self::Pose(Property::Position),
        Self::Pose(Property::Size),
        Self::Pose(Property::Rotation),
        Self::Pose(Property::Opacity),
        Self::Color,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pose(Property::Position) => "Position",
            Self::Pose(Property::Size) => "Size",
            Self::Pose(Property::Rotation) => "Rotation",
            Self::Pose(Property::Opacity) => "Opacity",
            Self::Color => "Color",
            Self::Mask(_, property) => property.label(),
            Self::Vector(property) => property.label(),
        }
    }

    pub fn channels(self) -> &'static [&'static str] {
        match self {
            Self::Pose(Property::Position) => &["X", "Y"],
            Self::Pose(Property::Size) => &["Width", "Height"],
            Self::Pose(Property::Rotation) => &["Angle"],
            Self::Pose(Property::Opacity) => &["Opacity"],
            Self::Color => &["Red", "Green", "Blue"],
            Self::Mask(_, property) => property.channels(),
            Self::Vector(property) => property.channels(),
        }
    }

    pub fn value(self, shape: &Shape, t: f64, channel: usize) -> f32 {
        match self {
            Self::Pose(property) => property.values(shape.lane_pose(t))[channel],
            Self::Color => rgb(&shape.color_at(t))[channel],
            Self::Mask(id, property) => shape.mask(id).map_or(0.0, |mask| {
                property.clamp(mask.values(property, t, &shape.ease)[channel])
            }),
            Self::Vector(VectorProperty::Path) => t as f32,
            Self::Vector(property) => shape.vector.as_ref()
                .and_then(|vector| vector.numbers_at(property, t, &shape.ease))
                .map_or(0.0, |values| values[channel]),
        }
    }

    pub(crate) fn exists_on(self, shape: &Shape) -> bool {
        match self {
            Self::Mask(id, _) => shape.mask(id).is_some(),
            Self::Vector(property) => shape.vector.as_ref().is_some_and(|vector| vector.exists(property)),
            _ => true,
        }
    }
}

pub const EASINGS: [(&str, &str); 5] = [
    ("Smooth", "smooth"),
    ("Steady", "linear"),
    ("Hold", "hold"),
    ("Speed up", "ease-in"),
    ("Slow down", "ease-out"),
];

pub fn same_time(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.000_01
}

fn rgb(color: &str) -> [f32; 3] {
    [1, 3, 5]
        .map(|i| u8::from_str_radix(color.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f32)
}

impl Key {
    pub(crate) fn from_pose(t: f64, p: Pose) -> Self {
        Self {
            t,
            x: p.x,
            y: p.y,
            w: p.w.max(1.0),
            h: p.h.max(1.0),
            r: p.r,
            o: p.o,
            ease: None,
            curve: None,
            props: None,
            linked_space: false,
            extra: Default::default(),
        }
    }

    pub fn sets(&self, property: Property) -> bool {
        self.props
            .as_ref()
            .is_none_or(|props| props.contains(&property))
    }

    pub(crate) fn exclude(&mut self, property: Property) {
        self.props
            .get_or_insert_with(|| Property::ALL.to_vec())
            .retain(|p| *p != property);
    }

    fn set_value(&mut self, property: Property, values: [f32; 2]) {
        if property == Property::Size && values.iter().all(|v| v.is_finite() && *v > 0.0) {
            // A subpixel layer can still be several canvas pixels wide under a scaled parent.
            [self.w, self.h] = values;
            return;
        }
        let mut p = Pose::from(&*self);
        property.set(&mut p, values);
        (self.x, self.y, self.w, self.h, self.r, self.o) = (p.x, p.y, p.w, p.h, p.r, p.o);
    }
}

#[derive(Clone, Debug)]
pub enum LaneKey {
    Pose(Property, Key),
    Color(ColorKey),
    Mask(u64, MaskKey),
    Vector(VectorKey),
}

impl LaneKey {
    pub fn lane(&self) -> Lane {
        match self {
            Self::Pose(property, _) => Lane::Pose(*property),
            Self::Color(_) => Lane::Color,
            Self::Mask(id, key) => Lane::Mask(*id, key.property),
            Self::Vector(key) => Lane::Vector(key.property),
        }
    }

    pub fn time(&self) -> f64 {
        match self {
            Self::Pose(_, key) => key.t,
            Self::Color(key) => key.t,
            Self::Mask(_, key) => key.t,
            Self::Vector(key) => key.t,
        }
    }

    pub fn value(&self, channel: usize) -> f32 {
        match self {
            Self::Pose(property, key) => property.values(Pose::from(key))[channel],
            Self::Color(key) => rgb(&key.color)[channel],
            Self::Mask(_, key) => key.values[channel],
            Self::Vector(key) => match &key.value {
                VectorValue::Path(_) => key.t as f32,
                VectorValue::Numbers(values) => values[channel],
            },
        }
    }

    pub fn set_value(&mut self, channel: usize, value: f32, lock_ratio: bool) {
        match self {
            Self::Pose(property, key) => {
                let mut values = property.values(Pose::from(&*key));
                if *property == Property::Size && lock_ratio {
                    values[1 - channel] =
                        (values[1 - channel] * value.max(1.0) / values[channel]).max(1.0);
                }
                values[channel] = value;
                key.set_value(*property, values);
            }
            Self::Color(key) => {
                let mut channels = rgb(&key.color);
                channels[channel] = value;
                let [r, g, b] = channels.map(|v| v.round().clamp(0.0, 255.0) as u8);
                key.color = format!("#{r:02x}{g:02x}{b:02x}");
            }
            Self::Mask(_, key) => key.values[channel] = key.property.clamp(value),
            Self::Vector(key) => {
                if let VectorValue::Numbers(values) = &mut key.value {
                    values[channel] = key.property.clamp(channel, value);
                }
            }
        }
    }

    pub fn easing<'a>(&'a self, default: &'a str) -> (&'a str, Option<[f32; 4]>) {
        let (ease, curve) = match self {
            Self::Pose(_, key) => (&key.ease, key.curve),
            Self::Color(key) => (&key.ease, key.curve),
            Self::Mask(_, key) => (&key.ease, key.curve),
            Self::Vector(key) => (&key.ease, key.curve),
        };
        (ease.as_deref().unwrap_or(default), curve)
    }

    pub fn set_easing(&mut self, kind: &str, handles: Option<[f32; 4]>) {
        let (ease, curve) = match self {
            Self::Pose(_, key) => (&mut key.ease, &mut key.curve),
            Self::Color(key) => (&mut key.ease, &mut key.curve),
            Self::Mask(_, key) => (&mut key.ease, &mut key.curve),
            Self::Vector(key) => (&mut key.ease, &mut key.curve),
        };
        *ease = Some(kind.into());
        *curve = handles;
    }

    pub fn handles(&self, default: &str) -> [f32; 4] {
        match self.easing(default) {
            ("custom", Some(handles)) => handles,
            ("linear", _) => [1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0],
            ("ease-in", _) => [0.42, 0.0, 1.0, 1.0],
            ("ease-out", _) => [0.0, 0.0, 0.58, 1.0],
            _ => [1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0],
        }
    }

    pub fn move_handle(&mut self, default: &str, handle: usize, x: f32, y: f32) {
        let mut handles = self.handles(default);
        handles[2 * handle] = x.clamp(0.0, 1.0);
        handles[2 * handle + 1] = y.clamp(-4.0, 5.0);
        self.set_easing("custom", Some(handles));
    }

    pub fn detach_from_motion(&mut self) {
        if let Self::Pose(_, key) = self {
            key.extra.remove("motionId");
        }
    }

    pub(crate) fn for_lane(mut self, lane: Lane) -> Self {
        if let (Self::Mask(id, _), Lane::Mask(target, _)) = (&mut self, lane) {
            *id = target;
        }
        self
    }
}

impl Shape {
    pub(crate) fn property_pose<'a>(
        &self,
        t: f64,
        keys: impl DoubleEndedIterator<Item = &'a Key> + Clone,
        mut pose: Pose,
    ) -> Pose {
        for property in Property::ALL {
            let keys = keys.clone().filter(|key| key.sets(property));
            let Some(before) = keys
                .clone()
                .rev()
                .find(|key| key.t <= t)
                .or_else(|| keys.clone().next())
            else {
                continue;
            };
            let after = keys
                .clone()
                .find(|key| key.t >= t)
                .or_else(|| keys.last())
                .unwrap();
            let span = after.t - before.t;
            let fraction = if span > 0.0 {
                let f = ((t - before.t) / span).clamp(0.0, 1.0) as f32;
                ease_at(
                    before.ease.as_deref().unwrap_or(&self.ease),
                    f,
                    before.curve,
                )
            } else {
                0.0
            };
            let [a, b] = property.values(Pose::from(before));
            let [c, d] = property.values(Pose::from(after));
            let values = [a + (c - a) * fraction, b + (d - b) * fraction];
            // Legacy keys can contain subpixel sizes and overshooting opacity curves.
            match property {
                Property::Size => [pose.w, pose.h] = values,
                Property::Opacity => pose.o = values[0],
                _ => property.set(&mut pose, values),
            }
        }
        pose
    }

    pub fn lane_pose(&self, t: f64) -> Pose {
        if is_linked(self) {
            self.property_pose(
                t,
                self.keys.iter().filter(|key| key.linked_space),
                self.linked_rest.unwrap_or(LINKED_NEUTRAL),
            )
        } else {
            self.pose(t)
        }
    }

    pub(crate) fn has_link_animation(&self) -> bool {
        self.linked_rest.is_some() || self.keys.iter().any(|key| key.linked_space)
    }

    fn transform_lane_keys(&self) -> impl DoubleEndedIterator<Item = &Key> {
        let linked = is_linked(self);
        self.keys
            .iter()
            .filter(move |key| key.linked_space == linked)
    }

    pub fn lane_keys(&self, lane: Lane) -> Vec<LaneKey> {
        match lane {
            Lane::Pose(property) => self
                .transform_lane_keys()
                .filter(|key| key.sets(property))
                .map(|key| LaneKey::Pose(property, key.clone()))
                .collect(),
            Lane::Color => self
                .color_keys
                .iter()
                .cloned()
                .map(LaneKey::Color)
                .collect(),
            Lane::Mask(id, property) => self
                .mask(id)
                .into_iter()
                .flat_map(|mask| &mask.keys)
                .filter(|key| key.property == property)
                .cloned()
                .map(|key| LaneKey::Mask(id, key))
                .collect(),
            Lane::Vector(property) => self.vector.iter().flat_map(|vector| &vector.keys)
                .filter(|key| key.property == property).cloned().map(LaneKey::Vector).collect(),
        }
    }

    pub fn lane_key(&self, lane: Lane, t: f64) -> Option<LaneKey> {
        match lane {
            Lane::Pose(property) => self
                .transform_lane_keys()
                .rev()
                .find(|key| key.sets(property) && same_time(key.t, t))
                .map(|key| LaneKey::Pose(property, key.clone())),
            Lane::Color => self
                .color_keys
                .iter()
                .rev()
                .find(|key| same_time(key.t, t))
                .cloned()
                .map(LaneKey::Color),
            Lane::Mask(id, property) => self
                .mask(id)?
                .keys
                .iter()
                .rev()
                .find(|key| key.property == property && same_time(key.t, t))
                .cloned()
                .map(|key| LaneKey::Mask(id, key)),
            Lane::Vector(property) => self.vector.as_ref()?.keys.iter().rev()
                .find(|key| key.property == property && same_time(key.t, t))
                .cloned().map(LaneKey::Vector),
        }
    }

    pub fn key_property(&mut self, t: f64, property: Property, pose: Pose) {
        let t = hundredths(t);
        let values = property.values(pose);
        let linked = is_linked(self);
        if let Some(existing) = self
            .keys
            .iter_mut()
            .rev()
            .find(|key| key.linked_space == linked && key.sets(property) && same_time(key.t, t))
        {
            existing.set_value(property, values);
        } else {
            let mut key = Key::from_pose(t, pose);
            key.set_value(property, values);
            self.put_lane_key(t, LaneKey::Pose(property, key));
        }
    }

    pub fn add_lane_key(&mut self, lane: Lane, t: f64) {
        let at = hundredths(t);
        if self.lane_key(lane, at).is_some() {
            return;
        }
        match lane {
            Lane::Pose(property) => self.key_property(at, property, self.lane_pose(t)),
            Lane::Color => self.put_lane_key(
                at,
                LaneKey::Color(ColorKey {
                    t: at,
                    color: self.color_at(t),
                    ease: None,
                    curve: None,
                    extra: Default::default(),
                }),
            ),
            Lane::Mask(id, property) => {
                if let Some(mask) = self.mask(id) {
                    let values = mask.pose(t, &self.ease).values(property);
                    self.put_lane_key(
                        at,
                        LaneKey::Mask(
                            id,
                            MaskKey {
                                t: at,
                                property,
                                values,
                                ease: None,
                                curve: None,
                            },
                        ),
                    );
                }
            }
            Lane::Vector(property) => {
                let value = if property == VectorProperty::Path {
                    let pose = self.pose(t);
                    self.path_at(t, [pose.w, pose.h]).map(VectorValue::Path)
                } else {
                    self.vector.as_ref().and_then(|vector| vector.value_at(property, t, &self.ease))
                };
                if let Some(value) = value {
                    self.put_lane_key(at, LaneKey::Vector(VectorKey::new(at, property, value)));
                }
            }
        }
    }

    pub fn key_color(&mut self, t: f64, color: String) {
        let t = hundredths(t);
        if self.color_keys.is_empty() && t > 0.0 {
            self.add_lane_key(Lane::Color, 0.0);
        }
        self.add_lane_key(Lane::Color, t);
        if let Some(key) = self.color_keys.iter_mut().find(|key| same_time(key.t, t)) {
            key.color = color;
        }
    }

    pub fn remove_lane_key(&mut self, lane: Lane, t: f64) {
        match lane {
            Lane::Pose(property) => {
                let still = self.lane_pose(t);
                self.exclude_keys(property, |key| same_time(key.t, t));
                if !self.transform_lane_keys().any(|key| key.sets(property)) {
                    let rest = if is_linked(self) {
                        self.linked_rest.get_or_insert(LINKED_NEUTRAL)
                    } else {
                        self.rest_pose.get_or_insert(still)
                    };
                    match property {
                        Property::Size => (rest.w, rest.h) = (still.w, still.h),
                        Property::Opacity => rest.o = still.o,
                        _ => property.set(rest, property.values(still)),
                    }
                }
            }
            Lane::Color => {
                let color = self.color_at(t);
                self.color_keys.retain(|key| !same_time(key.t, t));
                if self.color_keys.is_empty() {
                    self.color = color;
                }
            }
            Lane::Mask(id, property) => {
                let ease = self.ease.clone();
                if let Some(mask) = self.mask_mut(id) {
                    let value = mask.values(property, t, &ease);
                    mask.keys
                        .retain(|key| key.property != property || !same_time(key.t, t));
                    if !mask.keys.iter().any(|key| key.property == property) {
                        mask.rest.set(property, value);
                    }
                }
            }
            Lane::Vector(property) => {
                if let Some(vector) = &mut self.vector {
                    let value = vector.value_at(property, t, &self.ease);
                    vector.keys.retain(|key| key.property != property || !same_time(t, key.t));
                    if !vector.has_keys(property) && let Some(value) = value {
                        vector.set_rest(property, value);
                    }
                }
            }
        }
    }

    pub(crate) fn exclude_keys(
        &mut self,
        property: Property,
        mut matches: impl FnMut(&Key) -> bool,
    ) {
        let linked = is_linked(self);
        self.keys.retain_mut(|key| {
            if key.linked_space == linked && key.sets(property) && matches(key) {
                key.exclude(property);
            }
            key.props.as_ref().is_none_or(|props| !props.is_empty())
        });
    }

    pub fn put_lane_key(&mut self, t: f64, key: LaneKey) {
        match key {
            LaneKey::Pose(property, mut key) => {
                self.exclude_keys(property, |existing| same_time(existing.t, t));
                key.t = t;
                key.props = Some(vec![property]);
                key.linked_space = is_linked(self);
                let i = self.keys.partition_point(|existing| existing.t <= t);
                self.keys.insert(i, key);
            }
            LaneKey::Color(mut key) => {
                self.color_keys.retain(|existing| !same_time(existing.t, t));
                key.t = t;
                let i = self.color_keys.partition_point(|existing| existing.t < t);
                self.color_keys.insert(i, key);
            }
            LaneKey::Mask(id, key) => {
                if let Some(mask) = self.mask_mut(id) {
                    mask.put_key(key, t);
                }
            }
            LaneKey::Vector(key) => {
                if let Some(vector) = &mut self.vector {
                    vector.key(key, t);
                }
            }
        }
    }

    /// Moves one lane's selected keys together, keeping the gaps between them.
    pub fn retime_lane_keys(&mut self, lane: Lane, times: &[f64], delta: f64) -> bool {
        let selected: Vec<_> = times
            .iter()
            .filter_map(|&t| self.lane_key(lane, t))
            .collect();
        let others = self.lane_keys(lane);
        if selected.is_empty()
            || !delta.is_finite()
            || selected.iter().any(|key| {
                let to = key.time() + delta;
                (to < 0.0 && !same_time(to, 0.0))
                    || others.iter().any(|other| {
                        same_time(other.time(), to)
                            && !times.iter().any(|&t| same_time(t, other.time()))
                    })
            })
        {
            return false;
        }
        if same_time(delta, 0.0) {
            return true;
        }
        // Remove and insert as a group so one selected key can move into another's old slot.
        let rest = self.rest_pose;
        let linked_rest = self.linked_rest;
        let color = self.color.clone();
        let vector_rest = match lane {
            Lane::Vector(property) => self.vector.as_ref().and_then(|vector| vector.rest(property).map(|value| (property, value))),
            _ => None,
        };
        let mask_rest = match lane {
            Lane::Mask(id, _) => self.mask(id).map(|mask| (id, mask.rest)),
            _ => None,
        };
        for key in &selected {
            self.remove_lane_key(lane, key.time());
        }
        for key in selected {
            self.put_lane_key((key.time() + delta).max(0.0), key);
        }
        self.rest_pose = rest;
        self.linked_rest = linked_rest;
        self.color = color;
        if let Some((property, value)) = vector_rest && let Some(vector) = &mut self.vector {
            vector.set_rest(property, value);
        }
        if let Some((id, rest)) = mask_rest
            && let Some(mask) = self.mask_mut(id)
        {
            mask.rest = rest;
        }
        true
    }
}

pub(crate) const LINKED_NEUTRAL: Pose = Pose {
    x: 0.0,
    y: 0.0,
    w: 100.0,
    h: 100.0,
    r: 0.0,
    o: 100.0,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Scene;

    #[test]
    fn independent_properties_and_easing_do_not_add_stops_to_other_lanes() {
        let mut scene = Scene::empty("keys".into());
        scene.add_shape("a".into(), "rect");
        let shape = &mut scene.shapes[0];
        let start = shape.pose(0.0);
        let position = Lane::Pose(Property::Position);
        let mut key = shape.lane_key(position, 0.0).unwrap();
        key.set_easing("linear", None);
        shape.put_lane_key(0.0, key);
        shape.key_property(
            4.0,
            Property::Position,
            Pose {
                x: start.x + 100.0,
                ..start
            },
        );
        shape.key_property(2.0, Property::Rotation, Pose { r: 180.0, ..start });
        assert_eq!(shape.pose(1.0).x, start.x + 25.0);
        assert_eq!(shape.pose(0.5).r, 28.125);
        assert_eq!(shape.pose(3.0).r, 180.0);
        assert_eq!(shape.pose(4.0).w, start.w);
    }

    #[test]
    fn moving_a_handle_only_changes_its_lane_and_retime_stays_sorted() {
        let mut scene = Scene::example("keys".into());
        let shape = &mut scene.shapes[1];
        let lane = Lane::Pose(Property::Rotation);
        let before = shape.pose(1.0);
        let mut key = shape.lane_key(lane, 0.0).unwrap();
        key.move_handle(&shape.ease, 0, 0.25, 1.0);
        shape.put_lane_key(0.0, key);
        assert_eq!(
            shape.lane_key(lane, 0.0).unwrap().easing(&shape.ease).0,
            "custom"
        );
        assert_ne!(shape.pose(1.0).r, before.r);
        assert_eq!(shape.pose(1.0).x, before.x);
        assert!(shape.retime_lane_keys(lane, &[0.0, 2.0], 2.5));
        assert_eq!(
            shape
                .lane_keys(lane)
                .iter()
                .map(LaneKey::time)
                .collect::<Vec<_>>(),
            [2.5, 4.0, 4.5]
        );
        assert!(!shape.retime_lane_keys(lane, &[2.5], 1.5));
    }

    #[test]
    fn removing_all_lanes_preserves_a_static_pose_and_round_trips_extras() {
        let mut scene = Scene::empty("keys".into());
        scene.add_shape("a".into(), "rect");
        let shape = &mut scene.shapes[0];
        shape
            .extra
            .insert("future".into(), serde_json::json!({"enabled": true}));
        let before = shape.pose(0.0);
        for lane in Lane::ALL {
            shape.remove_lane_key(lane, 0.0);
        }
        assert!(shape.keys.is_empty());
        assert_eq!(shape.pose(2.0), before);
        assert_eq!(
            Scene::read(&serde_json::to_vec(&scene).unwrap()).unwrap(),
            scene
        );
    }
}
