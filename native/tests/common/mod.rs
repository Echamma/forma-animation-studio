use serde_json::Value;
use std::collections::HashSet;

// The pose and color key rules in src/model/validate.js, applied to the saved JSON.
pub fn assert_electron_keys(project: &Value) {
    let duration = project["duration"].as_f64().unwrap();
    for layer in project["shapes"].as_array().unwrap() {
        let keys = layer["keys"].as_array().unwrap();
        let name = &layer["name"];
        assert!(
            (1..=1000).contains(&keys.len()),
            "{name}: invalid key count"
        );
        assert!(
            valid_times(keys, duration),
            "{name}: invalid or duplicate key times"
        );
        for key in keys {
            assert!(valid_pose_key(key), "{name}: invalid key {key}");
        }
        if let Some(keys) = layer.get("colorKeys") {
            let keys = keys.as_array().unwrap();
            assert!(keys.len() <= 1000 && valid_times(keys, duration + 0.001));
            assert!(
                keys.iter()
                    .all(|key| key["color"].as_str().is_some_and(is_hex_color))
            );
        }
    }
}

fn valid_times(keys: &[Value], duration: f64) -> bool {
    let mut seen = HashSet::new();
    keys.iter().all(|key| {
        key["t"].as_f64().is_some_and(|t| {
            t.is_finite() && (0.0..=duration).contains(&t) && seen.insert((t + 0.0).to_bits())
        })
    })
}

fn valid_pose_key(key: &Value) -> bool {
    ["x", "y", "w", "h", "r", "o"].iter().all(|&p| {
        key[p]
            .as_f64()
            .is_some_and(|v| v.is_finite() && v.abs() <= 10_000.0)
    }) && key["w"].as_f64().is_some_and(|w| w > 0.0)
        && key["h"].as_f64().is_some_and(|h| h > 0.0)
        && in_range(&key["o"], 0.0, 100.0)
        && key.get("ease").is_none_or(|v| {
            matches!(
                v.as_str(),
                Some("smooth" | "linear" | "ease-in" | "ease-out" | "hold" | "custom")
            )
        })
        && key.get("curve").is_none_or(valid_curve)
}

fn valid_curve(curve: &Value) -> bool {
    let Some([x1, y1, x2, y2]) = curve.as_array().map(Vec::as_slice) else {
        return false;
    };
    in_range(x1, 0.0, 1.0)
        && in_range(x2, 0.0, 1.0)
        && in_range(y1, -5.0, 5.0)
        && in_range(y2, -5.0, 5.0)
}

fn in_range(value: &Value, min: f64, max: f64) -> bool {
    value.as_f64().is_some_and(|v| (min..=max).contains(&v))
}

fn is_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color.bytes().skip(1).all(|c| c.is_ascii_hexdigit())
}
