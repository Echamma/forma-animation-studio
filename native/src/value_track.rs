use crate::keyframes::same_time;
use crate::model::ease_at;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueKey {
    pub t: f64,
    pub value: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<[f32; 4]>,
}

impl ValueKey {
    pub fn new(t: f64, value: [f32; 3]) -> Self {
        Self {
            t,
            value,
            ease: None,
            curve: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValueTrack {
    pub rest: [f32; 3],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<ValueKey>,
}

impl ValueTrack {
    pub(crate) fn new(rest: [f32; 3]) -> Self {
        Self {
            rest,
            keys: Vec::new(),
        }
    }

    pub(crate) fn sample(&self, t: f64, ease: &str) -> [f32; 3] {
        let Some(first) = self.keys.first() else {
            return self.rest;
        };
        let after = self.keys.partition_point(|key| key.t <= t);
        let a = self.keys.get(after.saturating_sub(1)).unwrap_or(first);
        let b = self.keys.get(after).unwrap_or(a);
        if b.t <= a.t || t <= a.t {
            return a.value;
        }
        let fraction = ease_at(
            a.ease.as_deref().unwrap_or(ease),
            ((t - a.t) / (b.t - a.t)) as f32,
            a.curve,
        );
        std::array::from_fn(|i| a.value[i] + (b.value[i] - a.value[i]) * fraction)
    }

    pub fn key(&self, t: f64) -> Option<&ValueKey> {
        self.keys.iter().find(|key| same_time(t, key.t))
    }

    pub(crate) fn insert(&mut self, t: f64, mut key: ValueKey) {
        if !t.is_finite() || t < 0.0 {
            return;
        }
        key.t = t;
        self.keys.retain(|old| !same_time(t, old.t));
        let slot = self.keys.partition_point(|old| old.t < t);
        self.keys.insert(slot, key);
    }

    pub(crate) fn erase(&mut self, t: f64, sampled: [f32; 3]) {
        self.keys.retain(|key| !same_time(t, key.t));
        if self.keys.is_empty() {
            self.rest = sampled;
        }
    }
}
