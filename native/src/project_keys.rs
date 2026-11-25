use crate::model::{Key, Pose, Shape};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const NATIVE_KEYFRAMES: &str = "nativeKeyframes";
const VECTOR_POSE_KEYS: &str = "vectorPoseKeys";
const MAX_KEYS: usize = 1000;
const MAX_VALUE: f32 = 10_000.0;

pub(crate) fn electron_keys_valid(keys: &[Key], duration: f64) -> bool {
    if keys.is_empty() || keys.len() > MAX_KEYS {
        return false;
    }
    let mut times = HashSet::with_capacity(keys.len());
    keys.iter().all(|k| {
        (0.0..=duration).contains(&k.t)
            && times.insert((k.t + 0.0).to_bits())
            && [k.x, k.y, k.r].into_iter().all(|v| v.abs() <= MAX_VALUE)
            && [k.w, k.h].into_iter().all(|v| v > 0.0 && v <= MAX_VALUE)
            && (0.0..=100.0).contains(&k.o)
            && k.ease.as_deref().is_none_or(|ease| {
                matches!(
                    ease,
                    "smooth" | "linear" | "ease-in" | "ease-out" | "hold" | "custom"
                )
            })
            && k.curve.is_none_or(|[x1, y1, x2, y2]| {
                (0.0..=1.0).contains(&x1)
                    && (0.0..=1.0).contains(&x2)
                    && y1.abs() <= 5.0
                    && y2.abs() <= 5.0
            })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeKeyframes<'a> {
    keys: &'a [Key],
    baked_fingerprint: String,
}

#[derive(Serialize)]
struct BakedKey {
    t: f64,
    #[serde(flatten)]
    pose: Pose,
}

pub(crate) fn bake(shape: &Shape, duration: f64, saved: &mut Value) -> serde_json::Result<()> {
    let baked: Vec<_> = bake_times(&shape.keys, duration)
        .into_iter()
        .map(|t| BakedKey {
            t,
            pose: bounded_pose(shape.pose(t)),
        })
        .collect();
    let keys = serde_json::to_value(baked)?;
    let baked_fingerprint = fingerprint(&keys)
        .ok_or_else(|| serde::ser::Error::custom("Could not fingerprint the baked keyframes."))?;
    saved[NATIVE_KEYFRAMES] = serde_json::to_value(NativeKeyframes {
        keys: &shape.keys,
        baked_fingerprint,
    })?;
    saved["keys"] = keys;
    Ok(())
}

fn bake_times(keys: &[Key], duration: f64) -> Vec<f64> {
    let duration = if duration.is_finite() {
        duration.max(0.0)
    } else {
        0.0
    };
    let mut times: Vec<_> = keys
        .iter()
        .map(|k| {
            (if k.t.is_nan() {
                0.0
            } else {
                k.t.clamp(0.0, duration)
            }) + 0.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    if times.is_empty() {
        times.push(0.0);
    }
    if times.len() > MAX_KEYS {
        // Sample across the whole animation and keep both endpoints.
        times = (0..MAX_KEYS)
            .map(|i| times[i * (times.len() - 1) / (MAX_KEYS - 1)])
            .collect();
    }

    times
}

fn bounded_pose(mut p: Pose) -> Pose {
    let bounded = |v: f32, lo, hi| (if v.is_nan() { 0.0 } else { v }).clamp(lo, hi);
    let size = |v: f32| if v > 0.0 { v.min(MAX_VALUE) } else { 1.0 };
    [p.x, p.y, p.r] = [p.x, p.y, p.r].map(|v| bounded(v, -MAX_VALUE, MAX_VALUE));
    [p.w, p.h] = [p.w, p.h].map(size);
    p.o = bounded(p.o, 0.0, 100.0);
    p
}

pub(crate) fn take_native_keys(layer: &mut Map<String, Value>) -> Option<Vec<Key>> {
    let legacy = layer
        .get("vector")
        .is_some_and(Value::is_object)
        .then(|| layer.remove(VECTOR_POSE_KEYS))
        .flatten();
    let saved = if let Some(mut native) = layer.remove(NATIVE_KEYFRAMES) {
        let current = fingerprint(layer.get("keys")?)?;
        if native.get("bakedFingerprint")?.as_str()? != current {
            return None;
        }
        native.get_mut("keys")?.take()
    } else {
        // Phase 3's vector fallback preceded the fingerprint.
        legacy?
    };
    let keys: Vec<Key> = serde_json::from_value(saved).ok()?;
    keys.iter()
        .all(|k| {
            [k.x, k.y, k.w, k.h, k.r, k.o]
                .into_iter()
                .all(f32::is_finite)
        })
        .then_some(keys)
}

fn fingerprint(keys: &Value) -> Option<String> {
    const FIELDS: [&str; 7] = ["t", "x", "y", "w", "h", "r", "o"];
    let mut hash = Sha256::new();
    for key in keys.as_array()? {
        let key = key.as_object()?;
        if key.len() != FIELDS.len() {
            return None;
        }
        // Hash JSON numbers before they narrow to native f32 poses. Electron can reformat them.
        for field in FIELDS {
            let value = key.get(field)?.as_f64()?;
            hash.update((value + 0.0).to_be_bytes());
        }
    }
    let digest = format!("{:x}", hash.finalize());
    Some(digest[..32].to_owned())
}
