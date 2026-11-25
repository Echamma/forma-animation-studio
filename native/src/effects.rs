use crate::model::Shape;
use crate::value_track::{ValueKey, ValueTrack};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

macro_rules! parameters {
    ($($name:ident: $label:literal, $min:literal..=$max:literal;)+) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub enum Parameter { $($name,)+ }

        impl Parameter {
            pub(crate) const COUNT: usize = [$(Self::$name,)+].len();

            pub fn label(self) -> &'static str {
                match self { $(Self::$name => $label,)+ }
            }

            pub fn range(self) -> std::ops::RangeInclusive<f32> {
                match self { $(Self::$name => $min..=$max,)+ }
            }
        }
    };
}

parameters! {
    Radius: "Radius", 0.0..=200.0;
    Angle: "Angle", -360.0..=360.0;
    Zoom: "Amount", 0.0..=100.0;
    Color: "Color", 0.0..=255.0;
    Opacity: "Opacity", 0.0..=100.0;
    Distance: "Distance", 0.0..=240.0;
    Softness: "Softness", 0.0..=200.0;
    Threshold: "Threshold", 0.0..=100.0;
    Intensity: "Intensity", 0.0..=400.0;
    Brightness: "Brightness", -100.0..=100.0;
    Contrast: "Contrast", -100.0..=100.0;
    Hue: "Hue", -360.0..=360.0;
    Saturation: "Saturation", -100.0..=100.0;
    Lightness: "Lightness", -100.0..=100.0;
    BlackColor: "Black maps to", 0.0..=255.0;
    WhiteColor: "White maps to", 0.0..=255.0;
    Amount: "Amount", 0.0..=100.0;
    InputBlack: "Input black", 0.0..=255.0;
    InputWhite: "Input white", 0.0..=255.0;
    OutputBlack: "Output black", 0.0..=255.0;
    OutputWhite: "Output white", 0.0..=255.0;
    Gamma: "Gamma", 0.1..=10.0;
    StartColor: "Start color", 0.0..=255.0;
    EndColor: "End color", 0.0..=255.0;
    GrainSize: "Size", 1.0..=64.0;
    Seed: "Seed", 0.0..=100000.0;
    Spread: "Spread", 0.0..=100.0;
    PixelSize: "Size", 1.0..=200.0;
    WaveHeight: "Height", 0.0..=200.0;
    WaveWidth: "Width", 1.0..=2000.0;
    Speed: "Speed", -10.0..=10.0;
    Direction: "Direction", -360.0..=360.0;
    Displacement: "Amount", 0.0..=200.0;
    TurbulenceSize: "Size", 1.0..=1000.0;
    Evolution: "Evolution", -3600.0..=3600.0;
    Bulge: "Amount", -100.0..=100.0;
    Twist: "Angle", -720.0..=720.0;
}

impl Parameter {
    pub fn is_color(self) -> bool {
        matches!(
            self,
            Self::Color | Self::BlackColor | Self::WhiteColor | Self::StartColor | Self::EndColor
        )
    }

    pub fn channels(self) -> &'static [&'static str] {
        if self.is_color() {
            &["Red", "Green", "Blue"]
        } else {
            &["Value"]
        }
    }

    pub fn clamp(self, value: f32) -> f32 {
        let range = self.range();
        if value.is_finite() {
            value.clamp(*range.start(), *range.end())
        } else {
            0.0_f32.clamp(*range.start(), *range.end())
        }
    }

    fn clamped(self, value: [f32; 3]) -> [f32; 3] {
        if self.is_color() {
            value.map(|channel| self.clamp(channel))
        } else {
            [self.clamp(value[0]), 0.0, 0.0]
        }
    }
}

#[derive(Clone, Copy)]
pub struct Control {
    pub parameter: Parameter,
    pub default: [f32; 3],
}

const fn number(parameter: Parameter, value: f32) -> Control {
    Control {
        parameter,
        default: [value, 0.0, 0.0],
    }
}

const fn color(parameter: Parameter, r: f32, g: f32, b: f32) -> Control {
    Control {
        parameter,
        default: [r, g, b],
    }
}

macro_rules! effects {
    ($($kind:ident: $label:literal, $group:literal, [$($control:expr),*];)+) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub enum Kind { $($kind,)+ }

        impl Kind {
            pub const ALL: &'static [Self] = &[$(Self::$kind,)+];

            pub fn label(self) -> &'static str {
                match self { $(Self::$kind => $label,)+ }
            }

            pub fn group(self) -> &'static str {
                match self { $(Self::$kind => $group,)+ }
            }

            pub fn controls(self) -> &'static [Control] {
                match self { $(Self::$kind => const { &[$($control,)*] },)+ }
            }
        }
    };
}

use Parameter::*;

effects! {
    GaussianBlur: "Gaussian blur", "Blur", [number(Radius, 8.0)];
    DirectionalBlur: "Directional blur", "Blur", [number(Distance, 24.0), number(Angle, 0.0)];
    ZoomBlur: "Zoom blur", "Blur", [number(Zoom, 30.0)];
    DropShadow: "Drop shadow", "Light", [color(Color, 0.0, 0.0, 0.0), number(Opacity, 60.0), number(Angle, 45.0), number(Distance, 12.0), number(Softness, 6.0)];
    OuterGlow: "Outer glow", "Light", [color(Color, 255.0, 221.0, 140.0), number(Opacity, 90.0), number(Radius, 10.0)];
    InnerGlow: "Inner glow", "Light", [color(Color, 255.0, 221.0, 140.0), number(Opacity, 90.0), number(Radius, 10.0)];
    Glow: "Glow", "Light", [number(Threshold, 55.0), number(Radius, 12.0), number(Intensity, 100.0), color(Color, 255.0, 232.0, 173.0)];
    BrightnessContrast: "Brightness & contrast", "Color", [number(Brightness, 15.0), number(Contrast, 0.0)];
    HueSaturationLightness: "Hue / saturation / lightness", "Color", [number(Hue, 45.0), number(Saturation, 0.0), number(Lightness, 0.0)];
    Tint: "Tint", "Color", [color(BlackColor, 30.0, 36.0, 55.0), color(WhiteColor, 255.0, 215.0, 171.0), number(Amount, 100.0)];
    Fill: "Fill", "Color", [color(Color, 237.0, 121.0, 94.0), number(Amount, 100.0)];
    Levels: "Levels", "Color", [number(InputBlack, 0.0), number(InputWhite, 255.0), number(Gamma, 1.0), number(OutputBlack, 0.0), number(OutputWhite, 255.0)];
    Invert: "Invert", "Color", [number(Amount, 100.0)];
    GradientRamp: "Gradient ramp", "Color", [color(StartColor, 237.0, 121.0, 94.0), color(EndColor, 170.0, 160.0, 206.0), number(Angle, 90.0), number(Amount, 100.0)];
    Outline: "Outline", "Stylize", [number(Radius, 3.0), color(Color, 0.0, 0.0, 0.0), number(Opacity, 100.0)];
    Noise: "Noise / grain", "Stylize", [number(Amount, 14.0), number(GrainSize, 2.0), number(Seed, 0.0)];
    Vignette: "Vignette", "Stylize", [number(Amount, 55.0), number(Spread, 50.0), color(Color, 0.0, 0.0, 0.0)];
    ChromaticAberration: "Chromatic aberration", "Stylize", [number(Distance, 5.0), number(Angle, 0.0)];
    Pixelate: "Pixelate", "Stylize", [number(PixelSize, 10.0)];
    WaveWarp: "Wave warp", "Distort", [number(WaveHeight, 12.0), number(WaveWidth, 90.0), number(Speed, 1.0), number(Direction, 0.0)];
    TurbulentDisplace: "Turbulent displace", "Distort", [number(Displacement, 20.0), number(TurbulenceSize, 45.0), number(Evolution, 0.0)];
    Bulge: "Bulge", "Distort", [number(Bulge, 50.0), number(Radius, 80.0)];
    Twirl: "Twirl", "Distort", [number(Twist, 90.0), number(Radius, 80.0)];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub id: u64,
    pub kind: Kind,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub radial: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_edge_pixels: Option<bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<Parameter, ValueTrack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frozen_wave_phase: Option<f32>,
}

fn enabled() -> bool {
    true
}

pub(crate) fn is_false(value: &bool) -> bool {
    !*value
}

impl Effect {
    pub fn new(id: u64, kind: Kind) -> Self {
        Self {
            id,
            kind,
            enabled: true,
            radial: false,
            repeat_edge_pixels: None,
            tracks: BTreeMap::new(),
            frozen_wave_phase: None,
        }
    }

    pub fn control(&self, parameter: Parameter) -> Option<Control> {
        self.kind
            .controls()
            .iter()
            .copied()
            .find(|control| control.parameter == parameter)
    }

    pub(crate) fn repeats_edges(&self, adjustment: bool) -> bool {
        self.kind.samples_neighbors() && self.repeat_edge_pixels.unwrap_or(adjustment)
    }

    pub fn values(&self, parameter: Parameter, time: f64, ease: &str) -> [f32; 3] {
        self.tracks.get(&parameter).map_or_else(
            || {
                self.control(parameter)
                    .map_or([0.0; 3], |control| control.default)
            },
            |track| parameter.clamped(track.sample(time, ease)),
        )
    }

    pub(crate) fn track_mut(&mut self, parameter: Parameter) -> Option<&mut ValueTrack> {
        let control = self.control(parameter)?;
        Some(
            self.tracks
                .entry(parameter)
                .or_insert_with(|| ValueTrack::new(control.default)),
        )
    }

    pub(crate) fn put(&mut self, parameter: Parameter, time: f64, mut key: ValueKey) {
        if let Some(track) = self.track_mut(parameter) {
            key.value = parameter.clamped(key.value);
            track.insert(time, key);
        }
    }

    pub(crate) fn remove(&mut self, parameter: Parameter, time: f64, ease: &str) {
        let values = self.values(parameter, time, ease);
        if let Some(track) = self.tracks.get_mut(&parameter) {
            track.erase(time, values);
        }
    }

    pub fn edit(&mut self, parameter: Parameter, time: f64, values: [f32; 3]) {
        let Some(track) = self.track_mut(parameter) else {
            return;
        };
        let values = parameter.clamped(values);
        if track.keys.is_empty() {
            track.rest = values;
        } else {
            let time = crate::model::hundredths(time);
            let mut key = track
                .key(time)
                .cloned()
                .unwrap_or_else(|| ValueKey::new(time, values));
            key.value = values;
            track.insert(time, key);
        }
    }

    pub(crate) fn sample(&self, time: f64, ease: &str) -> Sample {
        let mut sample = Sample {
            kind: self.kind,
            radial: self.radial,
            repeat_edges: self.repeats_edges(false),
            phase: 0.0,
            values: [[0.0; 3]; Parameter::COUNT],
        };
        for control in self.kind.controls() {
            sample.values[control.parameter as usize] = self.values(control.parameter, time, ease);
        }
        if self.kind == Kind::WaveWarp {
            sample.phase = self.wave_phase(time, ease);
        }
        sample
    }

    fn freeze(&mut self, time: f64, ease: &str) {
        // Capture the integrated speed before clearing the keys that produced it.
        if self.kind == Kind::WaveWarp {
            self.frozen_wave_phase = Some(self.wave_phase(time, ease));
        }
        for (&parameter, track) in &mut self.tracks {
            track.rest = parameter.clamped(track.sample(time, ease));
            track.keys.clear();
        }
    }

    fn wave_phase(&self, time: f64, ease: &str) -> f32 {
        let cycles = if let Some(frozen) = self.frozen_wave_phase {
            let first = self.tracks.get(&Speed).and_then(|track| track.keys.first());
            // A frozen pose stays still until its first new speed key. That key starts
            // from the captured phase so adding animation never jumps at the key.
            let resumed = first.filter(|key| time > key.t).map_or(0.0, |key| {
                self.wave_cycles(time, ease) - self.wave_cycles(key.t, ease)
            });
            f64::from(frozen) + resumed
        } else {
            self.wave_cycles(time, ease)
        };
        let phase = cycles.rem_euclid(1.0) as f32;
        if phase.is_finite() { phase } else { 0.0 }
    }

    fn wave_cycles(&self, time: f64, ease: &str) -> f64 {
        let Some(track) = self
            .tracks
            .get(&Speed)
            .filter(|track| !track.keys.is_empty())
        else {
            return time * f64::from(self.values(Speed, time, ease)[0]);
        };
        let Some(first) = track.keys.first() else {
            return 0.0;
        };
        let speed = |key: &ValueKey| f64::from(Speed.clamp(key.value[0]));
        let mut cycles = time.min(first.t) * speed(first);
        for pair in track.keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if time <= a.t {
                return cycles;
            }
            let span = b.t - a.t;
            if span <= 0.0 {
                continue;
            }
            let fraction = ((time - a.t) / span).min(1.0);
            let kind = a.ease.as_deref().unwrap_or(ease);
            let eased = |f: f64| {
                let progress = crate::model::ease_at(kind, f as f32, a.curve);
                f64::from(
                    Speed.clamp((speed(a) + (speed(b) - speed(a)) * f64::from(progress)) as f32),
                )
            };
            // Integrate speed through the outgoing curve so a speed key never resets the wave.
            let area = match kind {
                "hold" => speed(a),
                "linear" => (speed(a) + eased(fraction)) * 0.5,
                _ => {
                    let interior: f64 = (1..8)
                        .map(|i| {
                            let weight = if i % 2 == 0 { 2.0 } else { 4.0 };
                            weight * eased(f64::from(i) * fraction / 8.0)
                        })
                        .sum();
                    (speed(a) + interior + eased(fraction)) / 24.0
                }
            };
            cycles += span * fraction * area;
        }
        if let Some(last) = track.keys.last() {
            cycles += (time - last.t).max(0.0) * speed(last);
        }
        cycles
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sample {
    pub kind: Kind,
    pub radial: bool,
    pub repeat_edges: bool,
    pub phase: f32,
    values: [[f32; 3]; Parameter::COUNT],
}

impl Sample {
    pub fn get(&self, p: Parameter) -> f32 {
        self.values[p as usize][0]
    }

    pub fn rgb(&self, p: Parameter) -> [f32; 3] {
        self.values[p as usize].map(|v| v / 255.0)
    }

    pub fn active(&self) -> bool {
        let n = |p| self.get(p) != 0.0;
        match self.kind {
            Kind::GaussianBlur => n(Radius),
            Kind::DirectionalBlur | Kind::ChromaticAberration => n(Distance),
            Kind::ZoomBlur => n(Zoom),
            Kind::DropShadow => n(Opacity) && (n(Distance) || n(Softness)),
            Kind::OuterGlow | Kind::InnerGlow | Kind::Outline => n(Radius) && n(Opacity),
            Kind::Glow => n(Intensity),
            Kind::BrightnessContrast => n(Brightness) || n(Contrast),
            Kind::HueSaturationLightness => {
                self.get(Hue) % 360.0 != 0.0 || n(Saturation) || n(Lightness)
            }
            Kind::Levels => {
                n(InputBlack)
                    || n(OutputBlack)
                    || self.get(InputWhite) != 255.0
                    || self.get(OutputWhite) != 255.0
                    || self.get(Gamma) != 1.0
            }
            Kind::Pixelate => self.get(PixelSize) >= 1.5,
            Kind::WaveWarp => n(WaveHeight),
            Kind::TurbulentDisplace => n(Displacement),
            Kind::Bulge => n(Bulge) && n(Radius),
            Kind::Twirl => n(Twist) && n(Radius),
            _ => n(Amount),
        }
    }
}

impl Shape {
    pub fn effect(&self, id: u64) -> Option<&Effect> {
        self.effects.iter().find(|effect| effect.id == id)
    }

    pub fn effect_mut(&mut self, id: u64) -> Option<&mut Effect> {
        self.effects.iter_mut().find(|effect| effect.id == id)
    }

    pub(crate) fn next_effect_id(&self) -> u64 {
        self.effects
            .iter()
            .map(|effect| effect.id)
            .max()
            .and_then(|id| id.checked_add(1))
            .unwrap_or_else(|| (1..).find(|&id| self.effect(id).is_none()).unwrap_or(1))
    }

    pub(crate) fn effect_times(&self) -> impl Iterator<Item = f64> + '_ {
        self.effects
            .iter()
            .flat_map(|effect| effect.tracks.values())
            .flat_map(|track| track.keys.iter().map(|key| key.t))
    }

    pub(crate) fn freeze_effects(&mut self, time: f64) {
        for effect in &mut self.effects {
            effect.freeze(time, &self.ease);
        }
    }

    pub(crate) fn sampled_effects(&self, time: f64) -> Vec<Sample> {
        self.effects
            .iter()
            .filter(|effect| effect.enabled)
            .map(|effect| {
                let mut sample = effect.sample(time, &self.ease);
                sample.repeat_edges = effect.repeats_edges(self.kind == "adjustment");
                sample
            })
            .filter(Sample::active)
            .collect()
    }
}

impl Kind {
    pub(crate) fn samples_neighbors(self) -> bool {
        matches!(
            self,
            Self::GaussianBlur
                | Self::DirectionalBlur
                | Self::ZoomBlur
                | Self::DropShadow
                | Self::OuterGlow
                | Self::InnerGlow
                | Self::Glow
                | Self::Outline
                | Self::ChromaticAberration
                | Self::Pixelate
                | Self::WaveWarp
                | Self::TurbulentDisplace
                | Self::Bulge
                | Self::Twirl
        )
    }
}

pub(crate) fn prepare(effects: &mut [Effect], duration: f64) -> bool {
    let mut ids = HashSet::new();
    effects.iter_mut().all(|effect| {
        let kind = effect.kind;
        ids.insert(effect.id)
            && effect
                .frozen_wave_phase
                .is_none_or(|phase| kind == Kind::WaveWarp && (0.0..=1.0).contains(&phase))
            && effect.tracks.iter_mut().all(|(&parameter, track)| {
                track.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
                kind.controls()
                    .iter()
                    .any(|control| control.parameter == parameter)
                    && track.rest.iter().all(|v| v.is_finite())
                    && track
                        .keys
                        .windows(2)
                        .all(|keys| !crate::keyframes::same_time(keys[0].t, keys[1].t))
                    && track.keys.iter().all(|key| {
                        (0.0..=duration + 0.001).contains(&key.t)
                            && key.value.iter().all(|v| v.is_finite())
                            && key.ease.as_deref().is_none_or(|ease| {
                                matches!(
                                    ease,
                                    "smooth"
                                        | "linear"
                                        | "ease-in"
                                        | "ease-out"
                                        | "hold"
                                        | "custom"
                                )
                            })
                            && key.curve.is_none_or(|[x1, y1, x2, y2]| {
                                (0.0..=1.0).contains(&x1)
                                    && (0.0..=1.0).contains(&x2)
                                    && y1.abs() <= 5.0
                                    && y2.abs() <= 5.0
                            })
                    })
            })
    })
}
