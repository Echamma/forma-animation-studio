use crate::effect_render::{color_matrix, turbulence_offset};
use crate::effects::{Kind, Parameter::*, Sample};
use crate::model::{Scene, Shape};
use std::fmt::Write;

pub(crate) fn validate(scene: &Scene, shape: &Shape) -> Result<(), String> {
    if shape.kind == "adjustment" {
        return Err(format!(
            "{}: animated SVG can't export adjustment layers.",
            shape.name
        ));
    }
    if shape.motion_blur && scene.motion_blur.unwrap_or_default().shutter_angle > 0.0 {
        return Err(format!(
            "{}: animated SVG can't export motion blur.",
            shape.name
        ));
    }
    for effect in shape.effects.iter().filter(|effect| effect.enabled) {
        let sample = effect.sample(0.0, &shape.ease);
        if !matches!(
            effect.kind,
            Kind::GaussianBlur | Kind::DropShadow | Kind::TurbulentDisplace | Kind::Levels
        ) && color_matrix(&sample).is_none()
        {
            return Err(format!(
                "{}: animated SVG can't export {}.",
                shape.name,
                effect.kind.label()
            ));
        }
    }
    Ok(())
}

pub(crate) fn filter(scene: &Scene, shape: &Shape, index: usize, defs: &mut String) -> String {
    let effects: Vec<_> = shape
        .effects
        .iter()
        .filter(|effect| effect.enabled)
        .collect();
    if effects.is_empty() {
        return String::new();
    }
    let id = format!("forma-effects-{index}");
    let timing = Timing::new(scene, shape);
    let span = timing
        .times
        .iter()
        .map(|&t| scene.local_pose(shape, t))
        .fold(1.0_f32, |span, p| span.max(p.w.abs()).max(p.h.abs()));
    let legacy = crate::render::blur(shape);
    let mut margin = legacy * 3.0 + 4.0;
    for effect in &effects {
        let padding = |t| {
            let s = effect.sample(t, &shape.ease);
            match s.kind {
                Kind::GaussianBlur => 3.0 * s.get(Radius),
                Kind::DropShadow => 3.0 * s.get(Softness) + s.get(Distance),
                Kind::TurbulentDisplace => s.get(Displacement) + turbulence_offset(&s).abs(),
                _ => 0.0,
            }
        };
        margin += timing
            .times
            .iter()
            .copied()
            .map(padding)
            .fold(0.0, f32::max);
    }
    let extent = span + margin;
    let _ = write!(
        defs,
        r#"<filter id="{id}" filterUnits="userSpaceOnUse" primitiveUnits="userSpaceOnUse" color-interpolation-filters="sRGB" x="-{extent}" y="-{extent}" width="{}" height="{}">"#,
        extent * 2.0,
        extent * 2.0
    );
    let mut input = "SourceGraphic".to_owned();
    if legacy > 0.0 {
        let _ = write!(
            defs,
            r#"<feGaussianBlur in="SourceGraphic" stdDeviation="{legacy}" result="legacy-blur"/>"#
        );
        input = "legacy-blur".into();
    }
    for (i, effect) in effects.into_iter().enumerate() {
        let output = format!("effect-{i}");
        let samples: Vec<_> = timing
            .times
            .iter()
            .map(|&t| effect.sample(t, &shape.ease))
            .collect();
        let value = |param| timing.attribute(&samples, |s| number(s.get(param)));
        match effect.kind {
            Kind::GaussianBlur => {
                primitive(
                    defs,
                    "feGaussianBlur",
                    &input,
                    &output,
                    &[("stdDeviation", value(Radius))],
                );
            }
            Kind::DropShadow => {
                shadow(defs, &timing, &samples, &input, &output);
            }
            Kind::TurbulentDisplace => {
                let noise = format!("{output}-noise");
                let shifted = format!("{output}-shifted");
                let frequency = timing.attribute(&samples, |s| number(1.0 / s.get(TurbulenceSize)));
                let offset = timing.attribute(&samples, |s| number(turbulence_offset(s)));
                let _ = write!(
                    defs,
                    r#"<feTurbulence type="fractalNoise" numOctaves="2" seed="1" stitchTiles="noStitch" result="{noise}""#
                );
                attributes(defs, &[("baseFrequency", frequency)]);
                defs.push_str("</feTurbulence>");
                primitive(
                    defs,
                    "feOffset",
                    &noise,
                    &shifted,
                    &[("dx", offset.clone()), ("dy", offset)],
                );
                let _ = write!(
                    defs,
                    r#"<feDisplacementMap in="{input}" in2="{shifted}" xChannelSelector="R" yChannelSelector="G" result="{output}""#
                );
                attributes(defs, &[("scale", value(Displacement))]);
                defs.push_str("</feDisplacementMap>");
            }
            Kind::Levels => levels(defs, &timing, &samples, &input, &output),
            _ => {
                let matrix = timing.attribute(&samples, |s| {
                    color_matrix(s).map_or_else(String::new, |matrix| {
                        let mut values = Vec::with_capacity(20);
                        for row in matrix {
                            values.extend([row[0], row[1], row[2], 0.0, row[3]].map(number));
                        }
                        values.extend(["0", "0", "0", "1", "0"].map(String::from));
                        values.join(" ")
                    })
                });
                primitive(
                    defs,
                    "feColorMatrix",
                    &input,
                    &output,
                    &[("values", matrix)],
                );
            }
        }
        input = output;
    }
    defs.push_str("</filter>");
    format!(r#" filter="url(#{id})""#)
}

fn shadow(defs: &mut String, timing: &Timing, samples: &[Sample], input: &str, output: &str) {
    let blurred = format!("{output}-blurred");
    let shifted = format!("{output}-shifted");
    let ink = format!("{output}-ink");
    let tint = format!("{output}-tint");
    let value = |param| timing.attribute(samples, |s| number(s.get(param)));
    let dx = timing.attribute(samples, |s| {
        number(s.get(Angle).to_radians().cos() * s.get(Distance))
    });
    let dy = timing.attribute(samples, |s| {
        number(s.get(Angle).to_radians().sin() * s.get(Distance))
    });
    let color = timing.attribute(samples, |s| {
        let [r, g, b] = s.rgb(Color).map(|v| (255.0 * v).round() as u8);
        format!("#{r:02x}{g:02x}{b:02x}")
    });
    let opacity = timing.attribute(samples, |s| {
        let opacity = if s.get(Softness) == 0.0 && s.get(Distance) == 0.0 {
            0.0
        } else {
            s.get(Opacity) / 100.0
        };
        number(opacity)
    });
    primitive(
        defs,
        "feGaussianBlur",
        input,
        &blurred,
        &[("stdDeviation", value(Softness))],
    );
    primitive(
        defs,
        "feOffset",
        &blurred,
        &shifted,
        &[("dx", dx), ("dy", dy)],
    );
    let _ = write!(defs, r#"<feFlood result="{ink}""#);
    attributes(defs, &[("flood-color", color), ("flood-opacity", opacity)]);
    let _ = write!(
        defs,
        r#"</feFlood><feComposite in="{ink}" in2="{shifted}" operator="in" result="{tint}"/><feMerge result="{output}"><feMergeNode in="{tint}"/><feMergeNode in="{input}"/></feMerge>"#
    );
}

fn levels(defs: &mut String, timing: &Timing, samples: &[Sample], input: &str, output: &str) {
    let normalized = format!("{output}-normalized");
    let slope = |s: &Sample| 255.0 / (s.get(InputWhite) - s.get(InputBlack)).max(1.0);
    let attrs = [
        ("slope", timing.attribute(samples, |s| number(slope(s)))),
        (
            "intercept",
            timing.attribute(samples, |s| number(-slope(s) * s.get(InputBlack) / 255.0)),
        ),
    ];
    transfer(defs, input, &normalized, "linear", &attrs);
    let attrs = [
        (
            "amplitude",
            timing.attribute(samples, |s| {
                number((s.get(OutputWhite) - s.get(OutputBlack)) / 255.0)
            }),
        ),
        (
            "exponent",
            timing.attribute(samples, |s| number(1.0 / s.get(Gamma))),
        ),
        (
            "offset",
            timing.attribute(samples, |s| number(s.get(OutputBlack) / 255.0)),
        ),
    ];
    transfer(defs, &normalized, output, "gamma", &attrs);
}

fn transfer(defs: &mut String, input: &str, output: &str, kind: &str, attrs: &[(&str, Attribute)]) {
    let _ = write!(
        defs,
        r#"<feComponentTransfer in="{input}" result="{output}">"#
    );
    for channel in ["R", "G", "B"] {
        let _ = write!(defs, r#"<feFunc{channel} type="{kind}""#);
        attributes(defs, attrs);
        let _ = write!(defs, "</feFunc{channel}>");
    }
    defs.push_str("</feComponentTransfer>");
}

fn primitive(defs: &mut String, tag: &str, input: &str, output: &str, attrs: &[(&str, Attribute)]) {
    let _ = write!(defs, r#"<{tag} in="{input}" result="{output}""#);
    attributes(defs, attrs);
    let _ = write!(defs, "</{tag}>");
}

#[derive(Clone)]
struct Attribute {
    initial: String,
    animation: Option<String>,
}

fn attributes(defs: &mut String, attrs: &[(&str, Attribute)]) {
    for (name, attr) in attrs {
        let _ = write!(defs, r#" {name}="{}""#, attr.initial);
    }
    defs.push('>');
    for (name, attr) in attrs {
        if let Some(animation) = &attr.animation {
            let _ = write!(defs, r#"<animate attributeName="{name}" {animation}/>"#);
        }
    }
}

struct Timing {
    times: Vec<f64>,
    duration: f64,
    key_times: String,
}

impl Timing {
    fn new(scene: &Scene, shape: &Shape) -> Self {
        let count = ((scene.duration * 60.0).ceil() as usize).clamp(1, 7200);
        let mut times: Vec<_> = (0..=count)
            .map(|i| scene.duration * i as f64 / count as f64)
            .collect();
        for time in shape.effect_times() {
            let time = time.clamp(0.0, scene.duration);
            if time > 0.000_01 {
                times.push(time - 0.000_01);
            }
            times.push(time);
        }
        times.sort_by(f64::total_cmp);
        times.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
        let key_times = times
            .iter()
            .map(|t| format!("{:.12}", t / scene.duration))
            .collect::<Vec<_>>()
            .join(";");
        Self {
            times,
            duration: scene.duration,
            key_times,
        }
    }

    fn attribute(&self, samples: &[Sample], value: impl Fn(&Sample) -> String) -> Attribute {
        let values: Vec<_> = samples.iter().map(value).collect();
        let initial = values.first().cloned().unwrap_or_default();
        let animation = values.iter().any(|value| *value != initial).then(|| {
            format!(
                r#"dur="{}s" repeatCount="indefinite" calcMode="linear" keyTimes="{}" values="{}""#,
                self.duration,
                self.key_times,
                values.join(";"),
            )
        });
        Attribute { initial, animation }
    }
}

fn number(value: f32) -> String {
    format!("{value:.6}")
}
