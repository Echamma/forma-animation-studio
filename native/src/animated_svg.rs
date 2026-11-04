//! Animated SVG export, as Electron Forma writes it (`src/export/svg.js`, `src/model/sampling.js`): one group per layer
//! whose translate, rotate, scale and opacity (and fill, with color keys) animate with SMIL over the scene's length,
//! looping. Each layer is sampled exactly at its keyframes and other events, and between them only where it moves
//! along a curve. Pictures are embedded as data URLs, so the file stands alone; video layers show their poster.

use crate::model::{Pose, Scene, Shape};
use crate::{assets, text};
use base64::Engine;
use canopy_ui::Place;
use std::fmt::Write;

/// The value just before an event is read this much earlier; differences smaller than `JUMP` over that instant are
/// continuous change, not a step.
const EPSILON: f64 = 1e-5;
const JUMP: f32 = 0.01;

struct Sample {
    t: f64,
    pose: Pose,
    color: String,
}

pub fn animated(scene: &Scene, place: &Place) -> Result<String, String> {
    let mut defs = String::new();
    let mut layers = String::new();
    let timing = |samples: &[Sample]| {
        let times: Vec<String> = samples
            .iter()
            .map(|s| fixed(s.t / scene.duration, 6))
            .collect();
        format!(
            r#"dur="{}s" repeatCount="indefinite" calcMode="linear" keyTimes="{}""#,
            scene.duration,
            times.join(";")
        )
    };
    for shape in &scene.shapes {
        let p = scene.render_pose(shape, 0.0);
        let mut group = format!(r#"<g fill="{}""#, attr(&shape.color_at(0.0)));
        group.push_str(&blur_filter(shape, p, &mut defs));
        let samples = sample_layer(scene, shape);
        let timing = timing(&samples);
        let values = |value: &dyn Fn(&Sample) -> String| {
            samples.iter().map(value).collect::<Vec<_>>().join(";")
        };
        let number = |v: f32| fixed(v as f64, 3);
        write!(group, r#" transform="translate({} {})">"#, p.x, p.y).unwrap();
        write!(
            group,
            r#"<g transform="rotate({})"><g transform="scale({} {})" opacity="{}">"#,
            p.r,
            p.w,
            p.h,
            p.o / 100.0
        )
        .unwrap();
        group.push_str(&geometry(shape, place)?);
        write!(
            group,
            r#"<animateTransform attributeName="transform" type="scale" {timing} values="{}"/>"#,
            values(&|s| format!("{} {}", number(s.pose.w), number(s.pose.h)))
        )
        .unwrap();
        write!(
            group,
            r#"<animate attributeName="opacity" {timing} values="{}"/></g>"#,
            values(&|s| number(s.pose.o / 100.0))
        )
        .unwrap();
        write!(group, r#"<animateTransform attributeName="transform" type="rotate" {timing} values="{}"/></g>"#, values(&|s| number(s.pose.r))).unwrap();
        write!(group, r#"<animateTransform attributeName="transform" type="translate" {timing} values="{}"/>"#, values(&|s| format!("{} {}", number(s.pose.x), number(s.pose.y)))).unwrap();
        if !shape.color_keys.is_empty() {
            write!(
                group,
                r#"<animate attributeName="fill" {timing} values="{}"/>"#,
                attr(&values(&|s| s.color.clone()))
            )
            .unwrap();
        }
        group.push_str("</g>");
        layers.push_str(&group);
    }
    let (w, h) = (scene.width, scene.height);
    let defs = if defs.is_empty() {
        "<defs/>".to_owned()
    } else {
        format!("<defs>{defs}</defs>")
    };
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" overflow="hidden">{defs}<rect width="{w}" height="{h}" fill="{}"/>{layers}</svg>"#,
        attr(&scene.bg)
    ))
}

/// The scene at `t` as a still SVG (Electron's `svgText(false)`): each layer shown then, posed as it is then.
pub fn still(scene: &Scene, place: &Place, t: f64) -> Result<String, String> {
    let mut defs = String::new();
    let mut layers = String::new();
    for shape in scene.shapes.iter().filter(|s| s.visible(t, scene.duration)) {
        let p = scene.render_pose(shape, t);
        let mut group = format!(r#"<g fill="{}""#, attr(&shape.color_at(t)));
        group.push_str(&blur_filter(shape, p, &mut defs));
        write!(group, r#" transform="translate({} {})"><g transform="rotate({})"><g transform="scale({} {})" opacity="{}">"#, p.x, p.y, p.r, p.w, p.h, p.o / 100.0).unwrap();
        group.push_str(&geometry(shape, place)?);
        group.push_str("</g></g></g>");
        layers.push_str(&group);
    }
    let (w, h) = (scene.width, scene.height);
    let defs = if defs.is_empty() {
        "<defs/>".to_owned()
    } else {
        format!("<defs>{defs}</defs>")
    };
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" overflow="hidden">{defs}<rect width="{w}" height="{h}" fill="{}"/>{layers}</svg>"#,
        attr(&scene.bg)
    ))
}

/// A blurred layer's filter, added to `defs`; the attribute that puts it on the layer's group (empty for no blur).
fn blur_filter(shape: &Shape, p: Pose, defs: &mut String) -> String {
    let blur = shape
        .extra
        .get("blur")
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite())
        .map_or(0.0, |v| v.clamp(0.0, 100.0));
    if blur <= 0.0 {
        return String::new();
    }
    let width = shape.keys.iter().fold(p.w, |w, k| w.max(k.w)) as f64;
    let height = shape.keys.iter().fold(p.h, |h, k| h.max(k.h)) as f64;
    let margin = blur * 3.0 + 4.0;
    let id = attr(&format!("blur-{}", shape.id));
    write!(defs, r#"<filter id="{id}" filterUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}"><feGaussianBlur stdDeviation="{blur}"/></filter>"#,
        -width / 2.0 - margin, -height / 2.0 - margin, width + margin * 2.0, height + margin * 2.0).unwrap();
    format!(r#" filter="url(#{id})""#)
}

/// A layer one unit across, centered on its origin (the groups around it scale and place it), as `geometry()` draws it.
fn geometry(shape: &Shape, place: &Place) -> Result<String, String> {
    if matches!(shape.kind.as_str(), "image" | "svg" | "video") {
        let href = match shape.kind.as_str() {
            "svg" => format!(
                "data:image/svg+xml;charset=utf-8,{}",
                uri_component(&assets::svg_source(shape, place)?.unwrap_or_default())
            ),
            kind => {
                let field = if kind == "video" {
                    "posterData"
                } else {
                    "assetData"
                };
                let value = assets::stored(shape, field, place)?
                    .ok_or_else(|| format!("{} has no picture", shape.name))?;
                if value.starts_with("data:") {
                    value
                } else {
                    data_url(&value, place)?
                }
            }
        };
        return Ok(format!(
            r#"<image x="-0.5" y="-0.5" width="1" height="1" href="{}" preserveAspectRatio="none"/>"#,
            attr(&href)
        ));
    }
    Ok(match shape.kind.as_str() {
        "text" => format!(
            r#"<text x="0" y="0" text-anchor="middle" dominant-baseline="middle" font-family="DM Sans, sans-serif" font-size="1" font-weight="600" textLength="1" lengthAdjust="spacingAndGlyphs">{}</text>"#,
            text_content(text::content(shape))
        ),
        "circle" => r#"<ellipse cx="0" cy="0" rx="0.5" ry="0.5"/>"#.into(),
        "triangle" => r#"<polygon points="0,-0.5 0.5,0.5 -0.5,0.5"/>"#.into(),
        "diamond" => r#"<polygon points="0,-0.5 0.5,0 0,0.5 -0.5,0"/>"#.into(),
        "arrow" => r#"<polygon points="-0.5,-0.18 0.12,-0.18 0.12,-0.5 0.5,0 0.12,0.5 0.12,0.18 -0.5,0.18"/>"#.into(),
        _ => r#"<rect x="-0.5" y="-0.5" width="1" height="1" rx="0.015" ry="0.015"/>"#.into(),
    })
}

/// A library picture as a data URL (Electron's `inlineMedia`).
fn data_url(value: &str, place: &Place) -> Result<String, String> {
    let (bytes, extension) = assets::media_bytes(value, place)?;
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    };
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// When an animated SVG needs a value for a layer (Electron's `sampleLayer`): exact at every keyframe, clip edge,
/// rotation and fade boundary; straight or still stretches need only their ends, curved ones are sampled at `fps`,
/// and instant jumps get two values at the same moment.
fn sample_layer(scene: &Scene, shape: &Shape) -> Vec<Sample> {
    let duration = scene.duration;
    let fps = (12000.0 / duration).min(60.0);
    let at = |t: f64| Sample {
        t,
        pose: scene.render_pose(shape, t),
        color: shape.color_at(t),
    };
    let mut events = vec![0.0, duration];
    events.extend(
        event_times(scene, shape, &mut vec![shape.id.clone()])
            .into_iter()
            .filter(|t| t.is_finite() && *t > 0.0 && *t < duration),
    );
    events.sort_by(f64::total_cmp);
    events.dedup();
    let mut samples = Vec::new();
    for (i, &a) in events.iter().enumerate() {
        let sample = at(a);
        if i > 0 {
            let before = Sample {
                t: a,
                ..at(a - EPSILON)
            };
            if !same(&before, &sample) {
                samples.push(before);
            }
        }
        samples.push(sample);
        if let Some(&b) = events.get(i + 1) {
            if curved(shape, a, b) {
                let mut frame = (a * fps + EPSILON).floor() + 1.0;
                while frame / fps < b - EPSILON {
                    samples.push(at(frame / fps));
                    frame += 1.0;
                }
            }
        }
    }
    samples
}

fn link<'a>(scene: &'a Scene, shape: &Shape) -> Option<&'a Shape> {
    if shape.kind != "text" {
        return None;
    }
    let id = shape
        .extra
        .get("linkId")
        .and_then(|v| v.as_str())
        .filter(|id| !id.is_empty())?;
    scene.shapes.iter().find(|s| s.id == id)
}

fn event_times(scene: &Scene, shape: &Shape, seen: &mut Vec<String>) -> Vec<f64> {
    let mut times: Vec<f64> = shape.in_time.into_iter().chain(shape.out_time).collect();
    times.extend(shape.keys.iter().map(|k| k.t));
    times.extend(shape.color_keys.iter().map(|k| k.t));
    for clip in &shape.rotation_clips {
        times.extend([clip.start_time, clip.start_time + clip.duration]);
    }
    if let Some(d) = &shape.disappear {
        times.extend([
            d.start_time,
            d.start_time + d.duration,
            scene.reappear_at(shape),
        ]);
    }
    if let Some(target) = link(scene, shape).filter(|target| !seen.contains(&target.id)) {
        seen.push(target.id.clone());
        times.extend(event_times(scene, target, seen));
        times.push(target.out_time.unwrap_or(scene.duration));
    }
    times
}

/// Whether anything about the layer changes along a curve between two neighbouring event times.
fn curved(shape: &Shape, a: f64, b: f64) -> bool {
    if shape.kind == "text"
        && shape
            .extra
            .get("linkId")
            .and_then(|v| v.as_str())
            .is_some_and(|id| !id.is_empty())
    {
        return true;
    }
    let mid = (a + b) / 2.0;
    let before = shape.keys.iter().rposition(|k| k.t <= mid).unwrap_or(0);
    let after = shape
        .keys
        .iter()
        .position(|k| k.t >= mid)
        .unwrap_or(shape.keys.len() - 1);
    let (ka, kb) = (&shape.keys[before], &shape.keys[after]);
    let moving = before != after
        && (ka.x != kb.x
            || ka.y != kb.y
            || ka.w != kb.w
            || ka.h != kb.h
            || ka.r != kb.r
            || ka.o != kb.o);
    let kind = ka.ease.as_deref().unwrap_or(&shape.ease);
    if moving && kind != "linear" && kind != "hold" {
        return true;
    }
    if shape.rotation_clips.iter().any(|c| {
        c.degrees != 0.0 && c.ease != "linear" && c.start_time < b && c.start_time + c.duration > a
    }) {
        return true;
    }
    if let Some(d) = &shape.disappear {
        if d.duration > 0.0
            && d.start_time < b
            && d.start_time + d.duration > a
            && moving
            && ka.o != kb.o
        {
            return true;
        }
    }
    shape.color_at(a + EPSILON) != shape.color_at(b - EPSILON)
        && shape.ease != "linear"
        && shape.ease != "hold"
}

fn same(p: &Sample, q: &Sample) -> bool {
    let (a, b) = (p.pose, q.pose);
    [
        (a.x, b.x),
        (a.y, b.y),
        (a.w, b.w),
        (a.h, b.h),
        (a.r, b.r),
        (a.o, b.o),
    ]
    .iter()
    .all(|(u, v)| (u - v).abs() < JUMP)
        && p.color == q.color
}

/// JavaScript's `+value.toFixed(digits)` as text: rounded, without trailing zeros, and never "-0".
fn fixed(value: f64, digits: usize) -> String {
    let rounded: f64 = format!("{value:.digits$}").parse().unwrap_or(0.0);
    format!("{}", rounded + 0.0)
}

fn attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn text_content(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// JavaScript's `encodeURIComponent`.
fn uri_component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            write!(out, "%{byte:02X}").unwrap();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use resvg::usvg::roxmltree;

    fn place() -> Place {
        let root = std::env::temp_dir();
        Place {
            space: None,
            projects_dir: root.clone(),
            data_dir: root.clone(),
            exports_dir: root,
        }
    }

    #[test]
    fn straight_moves_need_only_their_ends_and_curves_get_frames() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "circle");
        let shape = &mut scene.shapes[0];
        shape.ease = "linear".into();
        shape.edit_pose(
            2.0,
            Pose {
                x: 700.0,
                ..shape.pose(0.0)
            },
        );
        assert_eq!(
            sample_layer(&scene, &scene.shapes[0])
                .iter()
                .map(|s| s.t)
                .collect::<Vec<_>>(),
            vec![0.0, 2.0, 4.0]
        );
        scene.shapes[0].ease = "smooth".into();
        // Smooth: every 60th of a second between the keys.
        assert_eq!(sample_layer(&scene, &scene.shapes[0]).len(), 3 + 119);
    }

    #[test]
    fn a_clip_cut_is_two_values_at_one_moment() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "rect");
        scene.shapes[0].in_time = Some(1.0);
        let samples = sample_layer(&scene, &scene.shapes[0]);
        let at_one: Vec<f32> = samples
            .iter()
            .filter(|s| s.t == 1.0)
            .map(|s| s.pose.o)
            .collect();
        assert_eq!(at_one, vec![0.0, 100.0]);
    }

    #[test]
    fn the_file_is_an_svg_resvg_can_draw() {
        let mut scene = Scene::empty("test".into());
        scene.add_shape("a".into(), "circle");
        scene.add_text("t".into());
        scene.shapes[1]
            .extra
            .insert("text".into(), "Fish & <chips>".into());
        scene.shapes[0].extra.insert("blur".into(), 4.into());
        let svg = animated(&scene, &place()).unwrap();
        assert!(svg.contains("Fish &amp; &lt;chips&gt;"));
        assert!(svg.contains(r#"<feGaussianBlur stdDeviation="4"/>"#));
        assert!(crate::svg::parse(&svg).is_ok());
    }

    #[test]
    fn matches_what_electron_forma_writes() {
        // `animated.electron.svg` is Electron Forma's own `svgText(true)` for `animated.json`, run in Electron on
        // 2026-11-01: easing (custom curves too), rotation clips, disappearing until a clip ends, color keys, linked
        // text, clip cuts, hold keys and blur. Numbers may differ in the last place: Forma native keeps poses in f32.
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let scene = Scene::read(&std::fs::read(fixtures.join("animated.json")).unwrap()).unwrap();
        let ours = animated(&scene, &place()).unwrap();
        let theirs = std::fs::read_to_string(fixtures.join("animated.electron.svg")).unwrap();
        let (ours, theirs) = (
            roxmltree::Document::parse(&ours).unwrap(),
            roxmltree::Document::parse(&theirs).unwrap(),
        );
        let (ours, theirs): (Vec<_>, Vec<_>) =
            (ours.descendants().collect(), theirs.descendants().collect());
        assert_eq!(ours.len(), theirs.len());
        for (a, b) in ours.iter().zip(&theirs) {
            assert_eq!(a.tag_name().name(), b.tag_name().name());
            if a.is_text() {
                assert_eq!(a.text(), b.text());
            }
            let names = |n: &roxmltree::Node| {
                n.attributes()
                    .map(|a| a.name().to_owned())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                names(a),
                names(b),
                "attributes of <{}>",
                a.tag_name().name()
            );
            for (x, y) in a.attributes().zip(b.attributes()) {
                let split = |v: &str| {
                    v.split([' ', ';', ',', '(', ')'])
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                };
                let (xs, ys) = (split(x.value()), split(y.value()));
                assert_eq!(
                    xs.len(),
                    ys.len(),
                    "{} of <{}>",
                    x.name(),
                    a.tag_name().name()
                );
                for (u, v) in xs.iter().zip(&ys) {
                    match (u.parse::<f64>(), v.parse::<f64>()) {
                        (Ok(u), Ok(v)) => assert!(
                            (u - v).abs() <= 0.0015 + v.abs() * 1e-6,
                            "{} of <{}>: {u} vs {v}",
                            x.name(),
                            a.tag_name().name()
                        ),
                        _ => assert_eq!(u, v, "{} of <{}>", x.name(), a.tag_name().name()),
                    }
                }
            }
        }
    }

    #[test]
    fn numbers_are_written_like_javascript() {
        assert_eq!(fixed(-0.0001, 3), "0");
        assert_eq!(fixed(2.5, 3), "2.5");
        assert_eq!(fixed(1.0 / 3.0, 6), "0.333333");
        assert_eq!(uri_component("<svg a='1'>é"), "%3Csvg%20a%3D'1'%3E%C3%A9");
    }
}
