use crate::model::Scene;
use serde::{Deserialize, Serialize};

pub const DEFAULT_FPS: u32 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub shutter_angle: f32,
    pub samples: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shutter_angle: 180.0,
            samples: 8,
        }
    }
}

impl Settings {
    pub(crate) fn valid(self) -> bool {
        (0.0..=720.0).contains(&self.shutter_angle) && (2..=32).contains(&self.samples)
    }
}

pub(crate) fn exposure(scene: &Scene, time: f64, fps: u32) -> Vec<f64> {
    let settings = scene.motion_blur.unwrap_or_default();
    if !settings.valid()
        || settings.shutter_angle == 0.0
        || fps == 0
        || !scene.shapes.iter().any(|layer| layer.motion_blur)
    {
        return Vec::new();
    }
    let duration = f64::from(settings.shutter_angle) / (360.0 * f64::from(fps));
    (0..settings.samples)
        .map(|i| {
            let offset = (f64::from(i) + 0.5) / f64::from(settings.samples) - 0.5;
            (time + offset * duration).clamp(0.0, scene.duration.max(0.0))
        })
        .collect()
}
