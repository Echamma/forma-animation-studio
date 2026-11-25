use crate::keyframes::same_time;
use crate::model::{Shape, ease_at, hundredths};
use crate::paths::PathData;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaintTarget {
    Fill,
    Stroke,
}

impl PaintTarget {
    pub const ALL: [Self; 2] = [Self::Fill, Self::Stroke];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaintMode {
    None,
    #[default]
    Solid,
    Linear,
    Radial,
}

impl PaintMode {
    pub const ALL: [Self; 4] = [Self::None, Self::Solid, Self::Linear, Self::Radial];

    pub fn gradient(self) -> bool {
        matches!(self, Self::Linear | Self::Radial)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaintStyle {
    pub mode: PaintMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient: Option<Gradient>,
}

impl PaintStyle {
    pub fn new(mode: PaintMode) -> Self {
        Self {
            mode,
            gradient: mode.gradient().then(Gradient::default),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    /// Start and end in centered layer-box coordinates. Radial paint uses their distance as its radius.
    pub ends: [f32; 4],
    pub stops: Vec<Stop>,
}

impl Default for Gradient {
    fn default() -> Self {
        Self {
            ends: [-0.5, 0.0, 0.5, 0.0],
            stops: vec![
                Stop {
                    offset: 0.0,
                    rgba: [255.0, 255.0, 255.0, 100.0],
                },
                Stop {
                    offset: 100.0,
                    rgba: [40.0, 48.0, 64.0, 100.0],
                },
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    pub offset: f32,
    pub rgba: [f32; 4],
}

impl Stop {
    fn values(self) -> [f32; 5] {
        let [r, g, b, a] = self.rgba;
        [self.offset, r, g, b, a]
    }

    fn from_values([offset, r, g, b, a]: [f32; 5]) -> Self {
        Self {
            offset,
            rgba: [r, g, b, a],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cap {
    #[default]
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Join {
    #[default]
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    pub paint: PaintStyle,
    pub rgba: [f32; 4],
    pub width: f32,
    pub cap: Cap,
    pub join: Join,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<Dash>,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            paint: PaintStyle::new(PaintMode::Solid),
            rgba: [255.0, 255.0, 255.0, 100.0],
            width: 4.0,
            cap: Cap::Butt,
            join: Join::Miter,
            dash: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dash {
    pub length: f32,
    pub gap: f32,
    pub offset: f32,
}

impl Default for Dash {
    fn default() -> Self {
        Self {
            length: 12.0,
            gap: 8.0,
            offset: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trim {
    pub start: f32,
    pub end: f32,
    pub offset: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fill: bool,
}

impl Default for Trim {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 100.0,
            offset: 0.0,
            fill: false,
        }
    }
}

impl Trim {
    pub fn full(self) -> bool {
        (self.end - self.start).abs() >= 100.0
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathData>,
    // Retains the primitive's exact outline at every size until the first point edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_shape: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<PaintStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<StrokeStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<Trim>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<VectorKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VectorProperty {
    Path,
    GradientEnds(PaintTarget),
    GradientStop(PaintTarget, usize),
    StrokeColor,
    StrokeWidth,
    DashOffset,
    TrimStart,
    TrimEnd,
    TrimOffset,
}

impl VectorProperty {
    pub fn label(self) -> &'static str {
        use PaintTarget::{Fill, Stroke};
        match self {
            Self::Path => "Path",
            Self::GradientEnds(Fill) => "Fill gradient",
            Self::GradientEnds(Stroke) => "Stroke gradient",
            Self::GradientStop(Fill, _) => "Fill stop",
            Self::GradientStop(Stroke, _) => "Stroke stop",
            Self::StrokeColor => "Stroke color",
            Self::StrokeWidth => "Stroke width",
            Self::DashOffset => "Dash offset",
            Self::TrimStart => "Trim start",
            Self::TrimEnd => "Trim end",
            Self::TrimOffset => "Trim offset",
        }
    }

    pub fn channels(self) -> &'static [&'static str] {
        match self {
            Self::Path => &["Progress"],
            Self::GradientEnds(_) => &["Start X", "Start Y", "End X", "End Y"],
            Self::GradientStop(..) => &["Position", "Red", "Green", "Blue", "Opacity"],
            Self::StrokeColor => &["Red", "Green", "Blue", "Opacity"],
            Self::StrokeWidth => &["Width"],
            Self::DashOffset | Self::TrimOffset => &["Offset"],
            Self::TrimStart => &["Start"],
            Self::TrimEnd => &["End"],
        }
    }

    pub fn clamp(self, channel: usize, value: f32) -> f32 {
        if !value.is_finite() {
            return 0.0;
        }
        let (min, max) = match self {
            Self::GradientStop(..) if channel == 0 || channel == 4 => (0.0, 100.0),
            Self::StrokeColor if channel == 3 => (0.0, 100.0),
            Self::StrokeColor | Self::GradientStop(..) => (0.0, 255.0),
            Self::StrokeWidth => (0.0, 4000.0),
            Self::TrimStart | Self::TrimEnd => (0.0, 100.0),
            Self::GradientEnds(_) => (-100.0, 100.0),
            Self::TrimOffset | Self::DashOffset | Self::Path => (-36_000.0, 36_000.0),
        };
        value.clamp(min, max)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VectorValue {
    Path(PathData),
    Numbers([f32; 5]),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorKey {
    pub t: f64,
    pub property: VectorProperty,
    pub value: VectorValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<[f32; 4]>,
}

impl VectorKey {
    pub fn new(t: f64, property: VectorProperty, value: VectorValue) -> Self {
        Self {
            t,
            property,
            value,
            ease: None,
            curve: None,
        }
    }
}

impl VectorLayer {
    pub fn paint(&self, target: PaintTarget) -> Option<&PaintStyle> {
        match target {
            PaintTarget::Fill => self.fill.as_ref(),
            PaintTarget::Stroke => self.stroke.as_ref().map(|s| &s.paint),
        }
    }

    pub fn paint_mut(&mut self, target: PaintTarget) -> Option<&mut PaintStyle> {
        match target {
            PaintTarget::Fill => self.fill.as_mut(),
            PaintTarget::Stroke => self.stroke.as_mut().map(|s| &mut s.paint),
        }
    }

    pub fn properties(&self) -> Vec<VectorProperty> {
        use VectorProperty::*;
        let mut properties = vec![
            Path,
            StrokeColor,
            StrokeWidth,
            DashOffset,
            TrimStart,
            TrimEnd,
            TrimOffset,
        ];
        for target in PaintTarget::ALL {
            if let Some(gradient) = self.paint(target).and_then(|paint| paint.gradient.as_ref()) {
                properties.push(GradientEnds(target));
                properties.extend((0..gradient.stops.len()).map(|i| GradientStop(target, i)));
            }
        }
        properties.retain(|&property| self.exists(property));
        properties
    }

    pub fn exists(&self, property: VectorProperty) -> bool {
        if property == VectorProperty::Path {
            return self.path.is_some();
        }
        self.rest_numbers(property).is_some()
    }

    pub fn rest(&self, property: VectorProperty) -> Option<VectorValue> {
        if property == VectorProperty::Path {
            return self.path.clone().map(VectorValue::Path);
        }
        self.rest_numbers(property).map(VectorValue::Numbers)
    }

    fn rest_numbers(&self, property: VectorProperty) -> Option<[f32; 5]> {
        use VectorProperty::*;
        let scalar = |n| [n, 0.0, 0.0, 0.0, 0.0];
        let color = |[r, g, b, a]: [f32; 4]| [r, g, b, a, 0.0];
        Some(match property {
            Path => return None,
            GradientEnds(target) => color(self.paint(target)?.gradient.as_ref()?.ends),
            GradientStop(target, index) => self
                .paint(target)?
                .gradient
                .as_ref()?
                .stops
                .get(index)?
                .values(),
            StrokeColor => color(self.stroke.as_ref()?.rgba),
            StrokeWidth => scalar(self.stroke.as_ref()?.width),
            DashOffset => scalar(self.stroke.as_ref()?.dash?.offset),
            TrimStart => scalar(self.trim?.start),
            TrimEnd => scalar(self.trim?.end),
            TrimOffset => scalar(self.trim?.offset),
        })
    }

    pub fn set_rest(&mut self, property: VectorProperty, value: VectorValue) {
        use VectorProperty::*;
        if let VectorValue::Path(path) = value {
            if property == Path && path.valid() {
                self.path = Some(path);
                self.source_shape = None;
            }
            return;
        }
        let VectorValue::Numbers(values) = value else {
            return;
        };
        let [a, b, c, d, e] = std::array::from_fn(|i| property.clamp(i, values[i]));
        match property {
            Path => {}
            GradientEnds(target) => {
                if let Some(gradient) = self.paint_mut(target).and_then(|p| p.gradient.as_mut()) {
                    gradient.ends = [a, b, c, d];
                }
            }
            GradientStop(target, index) => {
                if let Some(stop) = self
                    .paint_mut(target)
                    .and_then(|p| p.gradient.as_mut())
                    .and_then(|g| g.stops.get_mut(index))
                {
                    *stop = Stop::from_values([a, b, c, d, e]);
                }
            }
            StrokeColor | StrokeWidth | DashOffset => {
                if let Some(stroke) = &mut self.stroke {
                    match property {
                        StrokeColor => stroke.rgba = [a, b, c, d],
                        StrokeWidth => stroke.width = a,
                        _ => {
                            if let Some(dash) = &mut stroke.dash {
                                dash.offset = a;
                            }
                        }
                    }
                }
            }
            TrimStart | TrimEnd | TrimOffset => {
                if let Some(trim) = &mut self.trim {
                    match property {
                        TrimStart => trim.start = a,
                        TrimEnd => trim.end = a,
                        _ => trim.offset = a,
                    }
                }
            }
        }
    }

    pub fn has_keys(&self, property: VectorProperty) -> bool {
        self.keys.iter().any(|key| key.property == property)
    }

    pub fn bracket(&self, property: VectorProperty, t: f64) -> Option<(&VectorKey, &VectorKey)> {
        let keys = self.keys.iter().filter(|key| key.property == property);
        let first = keys.clone().next()?;
        let before = keys.clone().rev().find(|key| key.t <= t).unwrap_or(first);
        let after = keys
            .clone()
            .find(|key| key.t >= t)
            .or_else(|| keys.last())?;
        Some((before, after))
    }

    pub fn fraction(a: &VectorKey, b: &VectorKey, t: f64, ease: &str) -> f32 {
        if b.t <= a.t {
            return 0.0;
        }
        ease_at(
            a.ease.as_deref().unwrap_or(ease),
            ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0) as f32,
            a.curve,
        )
    }

    pub fn path_at(&self, t: f64, ease: &str) -> Option<PathData> {
        if let Some((a, b)) = self.bracket(VectorProperty::Path, t)
            && let (VectorValue::Path(a_path), VectorValue::Path(b_path)) = (&a.value, &b.value)
        {
            let f = Self::fraction(a, b, t, ease);
            return Some(if f == 0.0 {
                a_path.clone()
            } else if f == 1.0 {
                b_path.clone()
            } else {
                a_path.morph(b_path, f)
            });
        }
        self.path.clone()
    }

    pub fn numbers_at(&self, property: VectorProperty, t: f64, ease: &str) -> Option<[f32; 5]> {
        if let Some((a, b)) = self.bracket(property, t)
            && let (VectorValue::Numbers(x), VectorValue::Numbers(y)) = (&a.value, &b.value)
        {
            let f = Self::fraction(a, b, t, ease);
            return Some(std::array::from_fn(|i| {
                property.clamp(i, x[i] + (y[i] - x[i]) * f)
            }));
        }
        self.rest_numbers(property)
    }

    pub fn value_at(&self, property: VectorProperty, t: f64, ease: &str) -> Option<VectorValue> {
        if property == VectorProperty::Path {
            self.path_at(t, ease).map(VectorValue::Path)
        } else {
            self.numbers_at(property, t, ease).map(VectorValue::Numbers)
        }
    }

    pub fn key(&mut self, mut key: VectorKey, t: f64) {
        if !self.exists(key.property)
            || !t.is_finite()
            || t < 0.0
            || !valid_value(key.property, &key.value)
        {
            return;
        }
        key.t = t;
        self.keys
            .retain(|old| old.property != key.property || !same_time(old.t, t));
        let index = self.keys.partition_point(|old| old.t <= t);
        if key.property == VectorProperty::Path {
            self.source_shape = None;
        }
        self.keys.insert(index, key);
    }

    pub fn edit(&mut self, property: VectorProperty, value: VectorValue, t: f64) {
        if self.has_keys(property) {
            let at = hundredths(t);
            let mut key = self
                .keys
                .iter()
                .find(|k| k.property == property && same_time(k.t, at))
                .cloned()
                .unwrap_or_else(|| VectorKey::new(at, property, value.clone()));
            key.value = value;
            self.key(key, at);
        } else {
            self.set_rest(property, value);
        }
    }

    pub fn remove_stop(&mut self, target: PaintTarget, index: usize) {
        let Some(gradient) = self
            .paint_mut(target)
            .and_then(|paint| paint.gradient.as_mut())
        else {
            return;
        };
        if index >= gradient.stops.len() || gradient.stops.len() <= 2 {
            return;
        }
        gradient.stops.remove(index);
        self.keys.retain_mut(|key| {
            let VectorProperty::GradientStop(t, stop) = &mut key.property else {
                return true;
            };
            if *t != target {
                return true;
            }
            if *stop == index {
                return false;
            }
            if *stop > index {
                *stop -= 1;
            }
            true
        });
    }

    pub fn prune_keys(&mut self) {
        let properties = self.properties();
        self.keys.retain(|key| properties.contains(&key.property));
    }

    pub(crate) fn prepare(&mut self, duration: f64) -> bool {
        if self.path.as_ref().is_some_and(|path| !path.valid())
            || self
                .source_shape
                .as_deref()
                .is_some_and(|kind| !primitive(kind))
        {
            return false;
        }
        for target in PaintTarget::ALL {
            let Some(paint) = self.paint_mut(target) else {
                continue;
            };
            if paint.mode.gradient() && paint.gradient.is_none() {
                return false;
            }
            if let Some(gradient) = &mut paint.gradient {
                if !(2..=64).contains(&gradient.stops.len()) {
                    return false;
                }
                let ends = VectorProperty::GradientEnds(target);
                for (i, value) in gradient.ends.iter_mut().enumerate() {
                    *value = ends.clamp(i, *value);
                }
                for (index, stop) in gradient.stops.iter_mut().enumerate() {
                    let property = VectorProperty::GradientStop(target, index);
                    let values = stop.values();
                    *stop =
                        Stop::from_values(std::array::from_fn(|i| property.clamp(i, values[i])));
                }
            }
        }
        for property in self.properties() {
            if property != VectorProperty::Path
                && let Some(value) = self.rest(property)
            {
                self.set_rest(property, value);
            }
        }
        if let Some(dash) = self.stroke.as_mut().and_then(|s| s.dash.as_mut()) {
            dash.length = dash.length.clamp(0.1, 4000.0);
            dash.gap = dash.gap.clamp(0.1, 4000.0);
        }
        self.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        let mut last = std::collections::HashMap::new();
        self.keys.iter().all(|key| {
            (0.0..=duration + 0.001).contains(&key.t)
                && self.exists(key.property)
                && valid_value(key.property, &key.value)
                && last
                    .insert(key.property, key.t)
                    .is_none_or(|t| !same_time(t, key.t))
        })
    }

    pub fn gradient_at(&self, target: PaintTarget, t: f64, ease: &str) -> Option<Gradient> {
        let original = self.paint(target)?.gradient.as_ref()?;
        let [a, b, c, d, _] = self.numbers_at(VectorProperty::GradientEnds(target), t, ease)?;
        let mut stops: Vec<_> = original
            .stops
            .iter()
            .enumerate()
            .map(|(i, stop)| {
                self.numbers_at(VectorProperty::GradientStop(target, i), t, ease)
                    .map_or(*stop, Stop::from_values)
            })
            .collect();
        stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        Some(Gradient {
            ends: [a, b, c, d],
            stops,
        })
    }

    pub fn stroke_at(&self, t: f64, ease: &str) -> Option<StrokeStyle> {
        let mut stroke = self.stroke.clone()?;
        let a = |p, default| self.numbers_at(p, t, ease).map_or(default, |v| v[0]);
        stroke.width = a(VectorProperty::StrokeWidth, stroke.width);
        if let Some([r, g, b, a, _]) = self.numbers_at(VectorProperty::StrokeColor, t, ease) {
            stroke.rgba = [r, g, b, a];
        }
        if let Some(dash) = &mut stroke.dash {
            dash.offset = a(VectorProperty::DashOffset, dash.offset);
        }
        Some(stroke)
    }

    pub fn trim_at(&self, t: f64, ease: &str) -> Option<Trim> {
        let mut trim = self.trim?;
        let a = |p, default| self.numbers_at(p, t, ease).map_or(default, |v| v[0]);
        trim.start = a(VectorProperty::TrimStart, trim.start);
        trim.end = a(VectorProperty::TrimEnd, trim.end);
        trim.offset = a(VectorProperty::TrimOffset, trim.offset);
        Some(trim)
    }
}

fn valid_value(property: VectorProperty, value: &VectorValue) -> bool {
    match value {
        VectorValue::Path(path) => property == VectorProperty::Path && path.valid(),
        VectorValue::Numbers(values) => {
            property != VectorProperty::Path && values.iter().all(|v| v.is_finite())
        }
    }
}

pub fn primitive(kind: &str) -> bool {
    matches!(kind, "rect" | "circle" | "triangle" | "diamond" | "arrow")
}

impl Shape {
    pub fn is_vector(&self) -> bool {
        primitive(&self.kind) || self.kind == "path"
    }

    pub fn convert_to_path(&mut self, size: [f32; 2]) -> bool {
        if !primitive(&self.kind) {
            return false;
        }
        let Some(outline) = crate::render::shape_outline(&self.kind, size[0] / 2.0, size[1] / 2.0)
        else {
            return false;
        };
        let path = PathData::from_outline(&outline, size);
        if !path.valid() {
            return false;
        }
        let vector = self.vector.get_or_insert_with(VectorLayer::default);
        vector.path = Some(path);
        vector.source_shape = Some(std::mem::replace(&mut self.kind, "path".into()));
        true
    }

    pub fn path_at(&self, time: f64, size: [f32; 2]) -> Option<PathData> {
        let vector = self.vector.as_ref()?;
        if let Some(kind) = &vector.source_shape
            && let Some(outline) = crate::render::shape_outline(kind, size[0] / 2.0, size[1] / 2.0)
        {
            return Some(PathData::from_outline(&outline, size));
        }
        vector.path_at(time, &self.ease)
    }

    pub(crate) fn vector_times(&self) -> impl Iterator<Item = f64> + '_ {
        self.vector
            .iter()
            .flat_map(|vector| vector.keys.iter().map(|key| key.t))
    }

    pub fn draw_on(&mut self, start: f64, end: f64) -> bool {
        if !self.is_vector() || same_time(start, end) || start >= end {
            return false;
        }
        let color = crate::render::rgb(&self.color_at(start));
        let vector = self.vector.get_or_insert_with(VectorLayer::default);
        let stroke = vector.stroke.get_or_insert_with(|| StrokeStyle {
            rgba: [
                f32::from(color[0]),
                f32::from(color[1]),
                f32::from(color[2]),
                100.0,
            ],
            ..StrokeStyle::default()
        });
        if stroke.paint.mode == PaintMode::None {
            stroke.paint.mode = PaintMode::Solid;
        }
        vector.trim.get_or_insert_with(Trim::default);
        for (property, from, to) in [
            (VectorProperty::TrimStart, 0.0, 0.0),
            (VectorProperty::TrimEnd, 0.0, 100.0),
        ] {
            vector
                .keys
                .retain(|key| key.property != property || key.t < start || key.t > end);
            for (time, value) in [(start, from), (end, to)] {
                let mut key = VectorKey::new(
                    time,
                    property,
                    VectorValue::Numbers([value, 0.0, 0.0, 0.0, 0.0]),
                );
                key.ease = Some("linear".into());
                vector.key(key, time);
            }
        }
        true
    }

    pub(crate) fn freeze_vector(&mut self, t: f64) {
        if let Some(vector) = &mut self.vector {
            for property in vector.properties() {
                if vector.has_keys(property)
                    && let Some(value) = vector.value_at(property, t, &self.ease)
                {
                    vector.set_rest(property, value);
                }
            }
            vector.keys.clear();
        }
    }
}
