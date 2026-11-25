use crate::model::{Scene, Shape};
use crate::paths::{MeasuredPath, PathData};
use crate::shape_style::{
    Cap, Gradient, Join, PaintMode, PaintTarget, VectorLayer, VectorProperty,
};
use std::fmt::Write;

struct Sample {
    t: f64,
    key_time: f64,
    size: [f32; 2],
    path: PathData,
}

struct Timeline {
    duration: f64,
    samples: Vec<Sample>,
    timing: String,
}

impl Timeline {
    fn new(
        scene: &Scene,
        shape: &Shape,
        vector: &VectorLayer,
        at: Option<f64>,
    ) -> Result<Self, String> {
        let duration = scene.duration;
        let animated = at.is_none();
        let mut times = vec![at.unwrap_or(0.0)];
        if animated {
            times.push(duration);
            let size_keys: Vec<_> = shape
                .keys
                .iter()
                .filter(|key| key.sets(crate::keyframes::Property::Size))
                .collect();
            let changing_size = size_keys
                .windows(2)
                .any(|pair| (pair[0].w, pair[0].h) != (pair[1].w, pair[1].h));
            if !vector.keys.is_empty() || changing_size {
                let count = (duration * 60.0).ceil().clamp(1.0, 3600.0) as u32;
                times.extend((1..count).map(|i| duration * f64::from(i) / f64::from(count)));
                times.extend(vector.keys.iter().map(|key| key.t));
                times.extend(size_keys.iter().map(|key| key.t));
            }
        }
        times.retain(|t| (0.0..=duration).contains(t));
        times.sort_by(f64::total_cmp);
        times.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
        let mut samples = Vec::with_capacity(times.len());
        let at_time = |t, key_time| {
            let pose = scene.local_pose(shape, t);
            let size = [pose.w, pose.h];
            let path = if shape.kind == "path" {
                shape.path_at(t, size)
            } else {
                crate::render::shape_outline(&shape.kind, size[0] / 2.0, size[1] / 2.0)
                    .map(|outline| PathData::from_outline(&outline, size))
            }
            .ok_or_else(|| format!("{}: SVG could not read the path geometry.", shape.name))?;
            Ok::<_, String>(Sample {
                t,
                key_time,
                path,
                size,
            })
        };
        for time in times {
            if animated && time > 0.0 && has_hold_at(shape, vector, time) {
                samples.push(at_time((time - 1e-7).max(0.0), time)?);
            }
            samples.push(at_time(time, time)?);
        }
        if samples.is_empty() {
            return Err(format!(
                "{}: SVG could not find a frame for this layer.",
                shape.name
            ));
        }

        let first = &samples[0].path;
        if animated
            && samples
                .iter()
                .any(|sample| different_contours(first, &sample.path))
        {
            return Err(format!(
                "{}: Animated SVG can't export a morph that opens, closes, adds, or removes a subpath.",
                shape.name
            ));
        }
        let max: Vec<_> = (0..first.contours.len())
            .map(|i| {
                samples
                    .iter()
                    .map(|sample| sample.path.contours[i].points.len())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        if animated {
            for sample in &mut samples {
                for (contour, count) in sample.path.contours.iter_mut().zip(&max) {
                    contour.split_to(*count);
                }
            }
        }
        let timing = samples
            .iter()
            .map(|s| format!("{:.8}", s.key_time / duration))
            .collect::<Vec<_>>()
            .join(";");
        Ok(Self {
            duration,
            samples,
            timing,
        })
    }

    fn strings(&self, value: impl Fn(&Sample) -> String) -> Vec<String> {
        self.samples.iter().map(value).collect()
    }

    fn attr(&self, key: &str, values: Vec<String>, attr: &mut String, children: &mut String) {
        let Some(first) = values.first() else {
            return;
        };
        let _ = write!(attr, r#" {key}="{first}""#);
        if values.iter().any(|value| value != first) {
            let _ = write!(
                children,
                r#"<animate attributeName="{key}" dur="{}s" repeatCount="indefinite" calcMode="linear" keyTimes="{}" values="{}"/>"#,
                self.duration,
                self.timing,
                values.join(";")
            );
        }
    }
}

fn has_hold_at(shape: &Shape, vector: &VectorLayer, t: f64) -> bool {
    use crate::keyframes::{Property, same_time};
    let held_size = shape
        .keys
        .iter()
        .any(|key| key.sets(Property::Size) && same_time(key.t, t))
        && shape
            .keys
            .iter()
            .rev()
            .find(|key| key.sets(Property::Size) && key.t < t - 1e-8)
            .is_some_and(|key| key.ease.as_deref().unwrap_or(&shape.ease) == "hold");
    held_size
        || vector.keys.iter().filter(|k| same_time(k.t, t)).any(|k| {
            vector
                .keys
                .iter()
                .rev()
                .find(|old| old.property == k.property && old.t < t - 1e-8)
                .is_some_and(|key| key.ease.as_deref().unwrap_or(&shape.ease) == "hold")
        })
}

fn different_contours(a: &PathData, b: &PathData) -> bool {
    a.contours.len() != b.contours.len()
        || a.contours
            .iter()
            .zip(&b.contours)
            .any(|(a, b)| a.closed != b.closed)
}

pub(crate) fn geometry(
    scene: &Scene,
    shape: &Shape,
    index: usize,
    defs: &mut String,
    at: Option<f64>,
) -> Result<Option<String>, String> {
    let Some(vector) = shape.vector.as_ref().filter(|_| shape.is_vector()) else {
        return Ok(None);
    };
    let timeline = Timeline::new(scene, shape, vector, at)?;
    let export = if at.is_some() {
        "Still SVG"
    } else {
        "Animated SVG"
    };
    let unsupported = |feature| Err(format!("{}: {export} can't export {feature}.", shape.name));
    let mut result = String::new();
    let d = timeline.strings(|s| s.path.svg(s.size, at.is_none()));
    let trim: Vec<_> = timeline
        .samples
        .iter()
        .map(|s| vector.trim_at(s.t, &shape.ease))
        .collect();
    let trimmed = trim.iter().flatten().any(|trim| !trim.full());
    let fill_mode = vector.fill.as_ref().map_or(PaintMode::Solid, |p| p.mode);
    if fill_mode != PaintMode::None {
        if trimmed && vector.trim.is_some_and(|trim| trim.fill) {
            return unsupported("trim applied to the fill");
        }
        let mut attrs = String::new();
        let mut children = String::new();
        timeline.attr("d", d.clone(), &mut attrs, &mut children);
        if fill_mode.gradient() {
            let id = format!("forma-fill-{index}");
            gradient(
                defs,
                &id,
                &timeline,
                shape,
                vector,
                PaintTarget::Fill,
                fill_mode,
            );
            let _ = write!(attrs, r#" fill="url(#{id})""#);
        }
        let _ = write!(result, "<path{attrs}>{children}</path>");
    }
    if let Some(stroke) = vector
        .stroke
        .as_ref()
        .filter(|stroke| stroke.paint.mode != PaintMode::None)
    {
        if trimmed && stroke.dash.is_some() {
            return unsupported("trim combined with stroke dashes");
        }
        if trimmed
            && timeline
                .samples
                .iter()
                .any(|sample| sample.path.contours.len() > 1)
        {
            return unsupported("trim across multiple subpaths");
        }
        let mut attrs = String::from(r#" fill="none""#);
        let mut children = String::new();
        timeline.attr("d", d, &mut attrs, &mut children);
        let cap = match stroke.cap {
            Cap::Butt => "butt",
            Cap::Round => "round",
            Cap::Square => "square",
        };
        let join = match stroke.join {
            Join::Miter => "miter",
            Join::Round => "round",
            Join::Bevel => "bevel",
        };
        let _ = write!(attrs, r#" stroke-linecap="{cap}" stroke-linejoin="{join}""#);
        let values = |property, channel| {
            timeline.strings(|s| {
                vector
                    .numbers_at(property, s.t, &shape.ease)
                    .map_or_else(|| "0".into(), |v| num(v[channel]))
            })
        };
        timeline.attr(
            "stroke-width",
            values(VectorProperty::StrokeWidth, 0),
            &mut attrs,
            &mut children,
        );
        if stroke.paint.mode.gradient() {
            let id = format!("forma-stroke-{index}");
            gradient(
                defs,
                &id,
                &timeline,
                shape,
                vector,
                PaintTarget::Stroke,
                stroke.paint.mode,
            );
            let _ = write!(attrs, r#" stroke="url(#{id})""#);
        } else {
            let colors = timeline.strings(|s| {
                let [r, g, b, _, _] = vector
                    .numbers_at(VectorProperty::StrokeColor, s.t, &shape.ease)
                    .unwrap_or_default();
                rgb([r, g, b, 100.0])
            });
            timeline.attr("stroke", colors, &mut attrs, &mut children);
            let opacity = timeline.strings(|s| {
                let opacity = vector
                    .numbers_at(VectorProperty::StrokeColor, s.t, &shape.ease)
                    .map_or(100.0, |v| v[3]);
                num(opacity / 100.0)
            });
            timeline.attr("stroke-opacity", opacity, &mut attrs, &mut children);
        }
        if let Some(dash) = stroke.dash {
            let _ = write!(attrs, r#" stroke-dasharray="{} {}""#, dash.length, dash.gap);
            timeline.attr(
                "stroke-dashoffset",
                values(VectorProperty::DashOffset, 0),
                &mut attrs,
                &mut children,
            );
        } else if trimmed {
            trim_stroke(&timeline, &trim, &mut attrs, &mut children);
        }
        let _ = write!(result, "<path{attrs}>{children}</path>");
    }
    Ok(Some(result))
}

fn trim_stroke(
    timeline: &Timeline,
    trim: &[Option<crate::shape_style::Trim>],
    attrs: &mut String,
    children: &mut String,
) {
    let mut dashes = Vec::with_capacity(trim.len());
    let mut offsets = Vec::with_capacity(trim.len());
    let mut visibility = Vec::with_capacity(trim.len());
    for (sample, trim) in timeline.samples.iter().zip(trim) {
        let trim = trim.unwrap_or_default();
        let length = sample
            .path
            .outline(sample.size)
            .map_or(0.0, |path| MeasuredPath::new(&path).length()) as f32;
        let visible = (trim.end - trim.start).clamp(0.0, 100.0) * length / 100.0;
        let offset = (trim.start / 100.0 + trim.offset / 360.0) * length;
        let dash = if trim.full() {
            length * 1.0001
        } else {
            visible
        };
        dashes.push(format!(
            "{} {}",
            num(dash),
            num((length - visible).max(0.0))
        ));
        offsets.push(num(-offset));
        visibility.push(if visible > 0.0 {
            "1".into()
        } else {
            "0".into()
        });
    }
    timeline.attr("stroke-dasharray", dashes, attrs, children);
    timeline.attr("stroke-dashoffset", offsets, attrs, children);
    timeline.attr("opacity", visibility, attrs, children);
}

fn gradient(
    defs: &mut String,
    id: &str,
    timeline: &Timeline,
    shape: &Shape,
    vector: &VectorLayer,
    target: PaintTarget,
    mode: PaintMode,
) {
    let frames: Vec<_> = timeline
        .samples
        .iter()
        .map(|s| {
            let gradient = vector
                .gradient_at(target, s.t, &shape.ease)
                .unwrap_or_else(Gradient::default);
            let [x, y, a, b] = gradient.ends;
            let [w, h] = s.size;
            (gradient, [x * w, y * h, a * w, b * h])
        })
        .collect();
    let radial = mode == PaintMode::Radial;
    let tag = if radial {
        "radialGradient"
    } else {
        "linearGradient"
    };
    let mut attrs = format!(r#" id="{id}" gradientUnits="userSpaceOnUse""#);
    let mut children = String::new();
    let coords = |i: usize| frames.iter().map(|(_, points)| num(points[i])).collect();
    if radial {
        timeline.attr("cx", coords(0), &mut attrs, &mut children);
        timeline.attr("cy", coords(1), &mut attrs, &mut children);
        let radius = frames
            .iter()
            .map(|(_, [x, y, a, b])| num((a - x).hypot(b - y)))
            .collect();
        timeline.attr("r", radius, &mut attrs, &mut children);
    } else {
        for (i, key) in ["x1", "y1", "x2", "y2"].into_iter().enumerate() {
            timeline.attr(key, coords(i), &mut attrs, &mut children);
        }
    }
    if let Some((first, _)) = frames.first() {
        for i in 0..first.stops.len() {
            let mut stop_attrs = String::new();
            let mut stop_children = String::new();
            timeline.attr(
                "offset",
                frames
                    .iter()
                    .map(|(g, _)| num(g.stops[i].offset / 100.0))
                    .collect(),
                &mut stop_attrs,
                &mut stop_children,
            );
            timeline.attr(
                "stop-color",
                frames.iter().map(|(g, _)| rgb(g.stops[i].rgba)).collect(),
                &mut stop_attrs,
                &mut stop_children,
            );
            timeline.attr(
                "stop-opacity",
                frames
                    .iter()
                    .map(|(g, _)| num(g.stops[i].rgba[3] / 100.0))
                    .collect(),
                &mut stop_attrs,
                &mut stop_children,
            );
            let _ = write!(children, "<stop{stop_attrs}>{stop_children}</stop>");
        }
    }
    let _ = write!(defs, "<{tag}{attrs}>{children}</{tag}>");
}

fn rgb([r, g, b, _]: [f32; 4]) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        r.round() as u8,
        g.round() as u8,
        b.round() as u8
    )
}

fn num(value: f32) -> String {
    format!("{value:.5}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}
