use crate::paths::{Contour, Vertex};
use serde_json::{Value, json};

mod export;
mod import;

pub use export::{export, unsupported};
pub use import::import;

const FPS: f64 = 30.0;
const LINEAR: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

fn constant(value: impl Into<Value>) -> Value {
    json!({ "a": 0, "k": value.into() })
}

fn animated_key(time: f64, value: Value, ease: &str, curve: Option<[f32; 4]>) -> Value {
    let mut item = json!({"t": time * FPS, "s": value});
    if ease == "hold" {
        item["h"] = 1.into();
    } else {
        let [x1, y1, x2, y2] = match ease {
            "linear" => LINEAR,
            "ease-in" => [0.42, 0.0, 1.0, 1.0],
            "ease-out" => [0.0, 0.0, 0.58, 1.0],
            "custom" => curve.unwrap_or([1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0]),
            _ => [1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0],
        };
        item["o"] = json!({"x": x1, "y": y1});
        item["i"] = json!({"x": x2, "y": y2});
    }
    item
}

fn curve(contour: &Contour, size: [f32; 2], center: [f32; 2]) -> Value {
    let mut vertices = Vec::new();
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();
    let scale = |[x, y]: [f32; 2]| [x * size[0], y * size[1]];
    for vertex in &contour.points {
        let [x, y] = scale(vertex.point);
        vertices.push([x + center[0], y + center[1]]);
        incoming.push(scale(vertex.incoming.unwrap_or_default()));
        outgoing.push(scale(vertex.outgoing.unwrap_or_default()));
    }
    json!({"c": contour.closed, "v": vertices, "i": incoming, "o": outgoing})
}

fn read_curve(value: &Value, size: [f32; 2], center: [f32; 2]) -> Result<Contour, String> {
    let array = |key: &str| value[key].as_array().ok_or("Invalid Lottie Bezier path.");
    let (vertices, incoming, outgoing) = (array("v")?, array("i")?, array("o")?);
    if !(2..=crate::paths::MAX_POINTS).contains(&vertices.len())
        || vertices.len() != incoming.len()
        || vertices.len() != outgoing.len()
    {
        return Err("Invalid Lottie Bezier vertices or handles.".into());
    }
    let point = |v: &Value, center: [f32; 2]| -> Result<[f32; 2], String> {
        Ok([
            (number(&v[0])? - center[0]) / size[0],
            (number(&v[1])? - center[1]) / size[1],
        ])
    };
    let points = vertices
        .iter()
        .zip(incoming)
        .zip(outgoing)
        .map(|((v, i), o)| {
            let offset = |v| point(v, [0.0; 2]).map(|p| (p != [0.0; 2]).then_some(p));
            Ok(Vertex {
                point: point(v, center)?,
                incoming: offset(i)?,
                outgoing: offset(o)?,
                smooth: false,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Contour {
        points,
        closed: value["c"].as_bool().unwrap_or(false),
    })
}

fn number(value: &Value) -> Result<f32, String> {
    value
        .as_f64()
        .map(|n| n as f32)
        .filter(|n| n.is_finite())
        .ok_or_else(|| "A Lottie number is missing or out of range.".into())
}
