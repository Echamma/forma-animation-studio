use crate::keyframes::{Lane, same_time};
use crate::model::{Shape, hundredths};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Alignment {
    Left,
    #[default]
    Center,
    Right,
}

impl Alignment {
    pub(crate) fn inset(self, free: f32) -> f32 {
        match self {
            Self::Left => 0.0,
            Self::Center => free * 0.5,
            Self::Right => free,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Units {
    #[default]
    Characters,
    VisibleCharacters,
    Words,
    Lines,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RangeShape {
    #[default]
    Square,
    RampUp,
    RampDown,
    Triangle,
    Round,
    Smooth,
}

impl RangeShape {
    pub fn amount(self, at: f32, start: f32, end: f32) -> f32 {
        let (start, end) = (start.min(end), start.max(end));
        let inside = at >= start && at < end;
        if self == Self::Square {
            return f32::from(inside);
        }
        let progress = if end > start {
            ((at - start) / (end - start)).clamp(0.0, 1.0)
        } else {
            f32::from(at >= start)
        };
        let peak = (1.0 - (2.0 * progress - 1.0).abs()).max(0.0);
        match self {
            Self::Square => f32::from(inside),
            Self::RampUp => progress,
            Self::RampDown => 1.0 - progress,
            Self::Triangle => peak,
            Self::Round => (1.0 - (2.0 * progress - 1.0).powi(2)).max(0.0).sqrt(),
            Self::Smooth => 0.5 - 0.5 * (std::f32::consts::TAU * progress).cos(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextProperty {
    Start,
    End,
    Offset,
    EaseLow,
    EaseHigh,
    Position,
    Scale,
    Rotation,
    Opacity,
    Fill,
    Stroke,
    Tracking,
    Blur,
    Skew,
    LineHeight,
    PathOffset,
}

impl TextProperty {
    pub const RANGE: [Self; 5] = [
        Self::Start,
        Self::End,
        Self::Offset,
        Self::EaseLow,
        Self::EaseHigh,
    ];
    pub const EFFECTS: [Self; 9] = [
        Self::Position,
        Self::Scale,
        Self::Rotation,
        Self::Opacity,
        Self::Fill,
        Self::Stroke,
        Self::Tracking,
        Self::Blur,
        Self::Skew,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "Range start",
            Self::End => "Range end",
            Self::Offset => "Range offset",
            Self::EaseLow => "Ease low",
            Self::EaseHigh => "Ease high",
            Self::Position => "Position offset",
            Self::Scale => "Scale",
            Self::Rotation => "Rotation",
            Self::Opacity => "Opacity",
            Self::Fill => "Fill color",
            Self::Stroke => "Stroke color",
            Self::Tracking => "Tracking",
            Self::Blur => "Blur",
            Self::Skew => "Skew",
            Self::LineHeight => "Line height",
            Self::PathOffset => "Path offset",
        }
    }

    pub fn channels(self) -> &'static [&'static str] {
        match self {
            Self::Position => &["X", "Y"],
            Self::Scale => &["Width", "Height"],
            Self::Fill | Self::Stroke => &["Red", "Green", "Blue"],
            _ => &["Value"],
        }
    }

    pub fn initial(self) -> [f32; 3] {
        match self {
            Self::Scale => [100.0, 100.0, 0.0],
            Self::End | Self::Opacity => [100.0, 0.0, 0.0],
            Self::LineHeight => [120.0, 0.0, 0.0],
            Self::Fill | Self::Stroke => [255.0, 255.0, 255.0],
            _ => [0.0; 3],
        }
    }

    pub fn limits(self) -> std::ops::RangeInclusive<f32> {
        match self {
            Self::Start | Self::End | Self::Opacity | Self::Blur => 0.0..=100.0,
            Self::Scale => 0.0..=1000.0,
            Self::Fill | Self::Stroke => 0.0..=255.0,
            Self::Offset => -200.0..=200.0,
            Self::EaseLow | Self::EaseHigh => -100.0..=100.0,
            Self::Rotation => -3600.0..=3600.0,
            Self::Skew => -85.0..=85.0,
            Self::LineHeight => 20.0..=500.0,
            Self::Tracking => -400.0..=400.0,
            Self::Position | Self::PathOffset => -10000.0..=10000.0,
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Self::Rotation | Self::Skew => "°",
            Self::Position | Self::Tracking | Self::Blur | Self::PathOffset => " px",
            _ => "%",
        }
    }

    pub fn clamp(self, value: f32) -> f32 {
        if value.is_nan() {
            self.initial()[0]
        } else {
            value.clamp(*self.limits().start(), *self.limits().end())
        }
    }

    fn values(self, values: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|i| {
            if i < self.channels().len() {
                self.clamp(values[i])
            } else {
                0.0
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextLane {
    Style(TextProperty),
    Animator(u64, TextProperty),
}

impl TextLane {
    pub fn property(self) -> TextProperty {
        match self {
            Self::Style(property) | Self::Animator(_, property) => property,
        }
    }

    pub fn track(self, shape: &Shape) -> Option<&TextTrack> {
        let text = shape.text_layout.as_ref()?;
        match self {
            Self::Style(property) => text.style.get(&property),
            Self::Animator(id, property) => text.animator(id)?.tracks.get(&property),
        }
    }

    pub fn track_mut(self, shape: &mut Shape) -> Option<&mut TextTrack> {
        let text = shape.text_layout.as_mut()?;
        let property = self.property();
        if !self.exists(text) {
            return None;
        }
        let tracks = match self {
            Self::Style(_) => &mut text.style,
            Self::Animator(id, _) => &mut text.animator_mut(id)?.tracks,
        };
        Some(
            tracks
                .entry(property)
                .or_insert_with(|| TextTrack::new(property.initial())),
        )
    }

    pub fn exists(self, text: &TextLayout) -> bool {
        match self {
            Self::Style(TextProperty::LineHeight | TextProperty::Tracking) => true,
            Self::Style(TextProperty::PathOffset) => text.path.is_some(),
            Self::Style(_) => false,
            Self::Animator(id, property) => text.animator(id).is_some_and(|animator| {
                TextProperty::RANGE.contains(&property) || animator.tracks.contains_key(&property)
            }),
        }
    }

    pub fn values(self, shape: &Shape, t: f64) -> [f32; 3] {
        let property = self.property();
        self.track(shape).map_or_else(
            || property.initial(),
            |track| track.at(property, t, &shape.ease),
        )
    }

    pub fn set(self, shape: &mut Shape, t: f64, value: [f32; 3]) {
        let at = hundredths(t);
        let property = self.property();
        let value = property.values(value);
        if self
            .track(shape)
            .is_some_and(|track| !track.keys.is_empty())
        {
            let mut key = self
                .track(shape)
                .and_then(|track| track.key(at))
                .cloned()
                .unwrap_or_else(|| TextKey::new(at, value));
            key.value = value;
            if let Some(track) = self.track_mut(shape) {
                track.put(at, key, property);
            }
        } else if let Some(track) = self.track_mut(shape) {
            track.rest = value;
        }
    }
}

pub use crate::value_track::{ValueKey as TextKey, ValueTrack as TextTrack};

impl TextTrack {
    pub fn at(&self, property: TextProperty, t: f64, ease: &str) -> [f32; 3] {
        property.values(self.sample(t, ease))
    }

    pub fn put(&mut self, t: f64, mut key: TextKey, property: TextProperty) {
        key.value = property.values(key.value);
        self.insert(t, key);
    }

    pub fn remove(&mut self, t: f64, property: TextProperty, ease: &str) {
        self.erase(t, self.at(property, t, ease));
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Animator {
    pub id: u64,
    pub name: String,
    pub units: Units,
    pub range_shape: RangeShape,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<TextProperty, TextTrack>,
}

impl Default for Animator {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Animator".into(),
            units: Units::default(),
            range_shape: RangeShape::default(),
            seed: None,
            tracks: BTreeMap::new(),
        }
    }
}

impl Animator {
    pub fn enable(&mut self, property: TextProperty, value: [f32; 3]) {
        self.tracks
            .entry(property)
            .or_insert_with(|| TextTrack::new(value));
    }

    pub fn sample(&self, property: TextProperty, t: f64, ease: &str) -> [f32; 3] {
        self.tracks
            .get(&property)
            .map_or_else(|| property.initial(), |track| track.at(property, t, ease))
    }

    pub fn weights(&self, count: usize, t: f64, ease: &str) -> Vec<f32> {
        let value = |p| self.sample(p, t, ease)[0];
        let offset = value(TextProperty::Offset);
        let start = value(TextProperty::Start) + offset;
        let end = value(TextProperty::End) + offset;
        let low = value(TextProperty::EaseLow) / 100.0;
        let high = value(TextProperty::EaseHigh) / 100.0;
        let mut ranks: Vec<_> = (0..count).collect();
        if let Some(seed) = self.seed {
            ranks.sort_unstable_by_key(|&rank| (shuffle_key(seed, rank), rank));
        }
        let mut weights = vec![0.0; count];
        for (rank, unit) in ranks.into_iter().enumerate() {
            let center = (rank as f32 + 0.5) * 100.0 / count as f32;
            let amount = self.range_shape.amount(center, start, end);
            // Endpoint slopes change independently, while 0 and 1 stay pinned.
            weights[unit] = (amount
                + amount * (1.0 - amount) * (-low * (1.0 - amount) + high * amount))
                .clamp(0.0, 1.0);
        }
        weights
    }
}

fn shuffle_key(seed: u32, rank: usize) -> u64 {
    let mut bits = (u64::from(seed) << 32 | rank as u64).wrapping_add(0x9e3779b97f4a7c15);
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d049bb133111eb);
    bits ^ (bits >> 31)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextLayout {
    pub alignment: Alignment,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub style: BTreeMap<TextProperty, TextTrack>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub animators: Vec<Animator>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl TextLayout {
    pub fn animator(&self, id: u64) -> Option<&Animator> {
        self.animators.iter().find(|animator| animator.id == id)
    }

    pub fn animator_mut(&mut self, id: u64) -> Option<&mut Animator> {
        self.animators.iter_mut().find(|animator| animator.id == id)
    }

    pub fn add(&mut self, mut animator: Animator) -> Option<u64> {
        if self.animators.len() >= 64 {
            return None;
        }
        let id = (1..).find(|&id| self.animator(id).is_none())?;
        animator.id = id;
        self.animators.push(animator);
        Some(id)
    }

    pub fn style_lanes(&self) -> impl Iterator<Item = Lane> + '_ {
        [
            TextProperty::LineHeight,
            TextProperty::Tracking,
            TextProperty::PathOffset,
        ]
        .into_iter()
        .map(TextLane::Style)
        .filter(|lane| lane.exists(self))
        .map(Lane::Text)
    }

    pub fn animator_lanes(&self, id: u64) -> impl Iterator<Item = Lane> + '_ {
        TextProperty::RANGE
            .into_iter()
            .chain(TextProperty::EFFECTS)
            .map(move |property| TextLane::Animator(id, property))
            .filter(|lane| lane.exists(self))
            .map(Lane::Text)
    }

    pub fn lanes(&self) -> impl Iterator<Item = Lane> + '_ {
        self.style_lanes().chain(
            self.animators
                .iter()
                .flat_map(|a| self.animator_lanes(a.id)),
        )
    }

    pub fn tracks(&self) -> impl Iterator<Item = &TextTrack> {
        self.style
            .values()
            .chain(self.animators.iter().flat_map(|a| a.tracks.values()))
    }

    pub fn tracks_mut(&mut self) -> impl Iterator<Item = &mut TextTrack> {
        self.style.values_mut().chain(
            self.animators
                .iter_mut()
                .flat_map(|a| a.tracks.values_mut()),
        )
    }

    pub fn prepare(&mut self, duration: f64) -> bool {
        let mut ids = HashSet::new();
        self.animators.len() <= 64
            && self.animators.iter().all(|a| a.id != 0 && ids.insert(a.id))
            && prepare_tracks(&mut self.style, duration, |p| {
                matches!(
                    p,
                    TextProperty::Tracking | TextProperty::LineHeight | TextProperty::PathOffset
                )
            })
            && self.animators.iter_mut().all(|a| {
                prepare_tracks(&mut a.tracks, duration, |p| {
                    TextProperty::RANGE.contains(&p) || TextProperty::EFFECTS.contains(&p)
                })
            })
    }

    pub fn freeze(&mut self, t: f64, ease: &str) {
        let freeze = |tracks: &mut BTreeMap<TextProperty, TextTrack>| {
            for (&property, track) in tracks.iter_mut() {
                track.rest = track.at(property, t, ease);
                track.keys.clear();
            }
        };
        freeze(&mut self.style);
        for animator in &mut self.animators {
            freeze(&mut animator.tracks);
        }
    }
}

fn prepare_tracks(
    tracks: &mut BTreeMap<TextProperty, TextTrack>,
    duration: f64,
    allowed: impl Fn(TextProperty) -> bool,
) -> bool {
    tracks.iter_mut().all(|(&property, track)| {
        track.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        if !allowed(property)
            || track.keys.len() > 10_000
            || track
                .keys
                .windows(2)
                .any(|pair| same_time(pair[0].t, pair[1].t))
        {
            return false;
        }
        track.rest = property.values(track.rest);
        track.keys.iter_mut().all(|key| {
            key.value = property.values(key.value);
            key.t.is_finite()
                && key.t >= 0.0
                && key.t <= duration + 0.001
                && key.ease.as_deref().is_none_or(|ease| {
                    matches!(
                        ease,
                        "smooth" | "linear" | "hold" | "ease-in" | "ease-out" | "custom"
                    )
                })
                && key.curve.is_none_or(|[x1, y1, x2, y2]| {
                    [x1, x2].into_iter().all(|x| (0.0..=1.0).contains(&x))
                        && [y1, y2].into_iter().all(|y| (-5.0..=5.0).contains(&y))
                })
        })
    })
}

impl Shape {
    pub(crate) fn text_times(&self) -> impl Iterator<Item = f64> + '_ {
        self.text_layout
            .iter()
            .flat_map(TextLayout::tracks)
            .flat_map(|track| track.keys.iter().map(|key| key.t))
    }

    pub(crate) fn freeze_text(&mut self, t: f64) {
        if let Some(text) = &mut self.text_layout {
            text.freeze(t, &self.ease);
        }
    }
}

impl crate::model::Scene {
    pub(crate) fn prune_missing_text_paths(&mut self) {
        let paths: HashSet<_> = self
            .shapes
            .iter()
            .filter(|shape| shape.kind == "path")
            .map(|shape| shape.id.clone())
            .collect();
        for text in self
            .shapes
            .iter_mut()
            .filter_map(|shape| shape.text_layout.as_mut())
        {
            if text.path.as_ref().is_some_and(|id| !paths.contains(id)) {
                text.path = None;
                text.style.remove(&TextProperty::PathOffset);
            }
        }
    }
}
