//! Animated SVG export, as Electron Forma writes it (`src/export/svg.js`, `src/model/sampling.js`): one group per layer
//! whose translate, rotate, scale and opacity (and fill, with color keys) animate with SMIL over the scene's length,
//! looping. Each layer is sampled exactly at its keyframes and other events, and between them only where it moves
//! along a curve. Pictures are embedded as data URLs, so the file stands alone; video layers show their poster.

use crate::compositing::Blend;
use crate::model::{Pose, Scene, Shape};
use crate::text_animator::{TextLane, TextProperty};
use crate::text_layout::TextCache;
use crate::text_render::TextInk;
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
    let mut text_cache = TextCache::default();
    for (index, shape) in scene.shapes.iter().enumerate() {
        crate::effect_svg::validate(scene, shape)?;
        let shaped_text = if TextInk::used_by(shape) {
            Some(shaped_geometry(shape, &mut text_cache)?)
        } else {
            None
        };
        let has_shaped_text = shaped_text.is_some();
        let p = scene.local_pose(shape, 0.0);
        let mut styling = format!(r#"<g fill="{}""#, attr(&shape.color_at(0.0)));
        styling.push_str(&compositing_style(shape, "Animated SVG")?);
        let mut chain = vec![shape];
        while let Some(parent) = chain.last().and_then(|s| {
            s.parent
                .as_deref()
                .and_then(|id| scene.shapes.iter().find(|p| p.id == id))
        }) {
            if chain.iter().any(|visited| visited.id == parent.id) {
                return Err(format!("{}: the Follows layers form a loop.", shape.name));
            }
            chain.push(parent);
        }
        for layer in &chain {
            if let Some(target) = link(scene, layer).filter(|target| {
                target.parent.is_some() || target.parent_space.is_some() || target.anchor.is_some()
            }) {
                return Err(format!(
                    "{}: animated SVG can't export text linked to a transformed layer ({}).",
                    layer.name, target.name
                ));
            }
        }
        let vector = crate::vector_svg::geometry(scene, shape, index, &mut defs, None)?;
        let local_pixels = vector.is_some();
        let geometry = match (shaped_text, vector) {
            (Some(geometry), _) | (None, Some(geometry)) => geometry,
            (None, None) => geometry(shape, place)?,
        };
        let filter = crate::effect_svg::filter(scene, shape, index, &mut defs);
        let nested = chain.len() > 1 || shape.parent_space.is_some();
        if !filter.is_empty() {
            let mut content = animate_group(
                scene,
                shape,
                "<g".into(),
                &geometry,
                true,
                local_pixels,
                &filter,
            );
            for (i, layer) in chain.iter().enumerate() {
                if i != 0 {
                    content = animate_group(scene, layer, "<g".into(), &content, false, false, "");
                }
                if let Some(basis) = layer.parent_space {
                    content = format!(r#"<g transform="{}">{content}</g>"#, svg_matrix(basis));
                }
            }
            let _ = write!(layers, "{styling}>{content}</g>");
            continue;
        }
        if !nested {
            if (local_pixels || has_shaped_text) && crate::render::blur(shape) > 0.0 {
                styling.push_str(&canvas_blur_filter(scene, shape, 0.0, &chain, &mut defs));
                let content =
                    animate_group(scene, shape, "<g".into(), &geometry, true, local_pixels, "");
                write!(layers, "{styling}>{content}</g>").unwrap();
            } else {
                styling.push_str(&blur_filter(shape, p, &mut defs));
                layers.push_str(&animate_group(
                    scene,
                    shape,
                    styling,
                    &geometry,
                    true,
                    local_pixels,
                    "",
                ));
            }
            continue;
        }
        styling.push_str(&canvas_blur_filter(scene, shape, 0.0, &chain, &mut defs));
        let mut content =
            animate_group(scene, shape, "<g".into(), &geometry, true, local_pixels, "");
        for (i, layer) in chain.iter().enumerate() {
            if i != 0 {
                content = animate_group(scene, layer, "<g".into(), &content, false, false, "");
            }
            if let Some(basis) = layer.parent_space {
                content = format!(r#"<g transform="{}">{content}</g>"#, svg_matrix(basis));
            }
        }
        write!(layers, "{styling}>{content}</g>").unwrap();
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

fn animate_group(
    scene: &Scene,
    shape: &Shape,
    mut group: String,
    content: &str,
    artwork: bool,
    local_pixels: bool,
    filter: &str,
) -> String {
    let p = scene.local_pose(shape, 0.0);
    let samples = sample_layer(scene, shape);
    let times: Vec<_> = samples
        .iter()
        .map(|s| fixed(s.t / scene.duration, 6))
        .collect();
    let timing = format!(
        r#"dur="{}s" repeatCount="indefinite" calcMode="linear" keyTimes="{}""#,
        scene.duration,
        times.join(";")
    );
    let values =
        |value: &dyn Fn(&Sample) -> String| samples.iter().map(value).collect::<Vec<_>>().join(";");
    let number = |v: f32| fixed(v as f64, 3);
    write!(
        group,
        r#" transform="translate({} {})"><g transform="rotate({})">"#,
        p.x, p.y, p.r
    )
    .unwrap();
    let filtered_anchor = shape.anchor.filter(|_| !filter.is_empty());
    if let Some(anchor) = filtered_anchor {
        let (x, y) = (0.5 - anchor.x, 0.5 - anchor.y);
        let _ = write!(
            group,
            r#"<g transform="translate({} {})"><animateTransform attributeName="transform" type="translate" {timing} values="{}"/>"#,
            x * p.w,
            y * p.h,
            values(&|s| format!("{} {}", number(s.pose.w * x), number(s.pose.h * y))),
        );
    }
    if !filter.is_empty() {
        let _ = write!(group, "<g{filter}>");
    }
    group.push_str("<g");
    if !local_pixels {
        write!(group, r#" transform="scale({} {})""#, p.w, p.h).unwrap();
    }
    if artwork {
        write!(group, r#" opacity="{}""#, p.o / 100.0).unwrap();
    }
    group.push('>');
    if let Some(anchor) = shape.anchor.filter(|_| filter.is_empty()) {
        let (x, y) = (0.5 - anchor.x, 0.5 - anchor.y);
        let (sx, sy) = if local_pixels { (p.w, p.h) } else { (1.0, 1.0) };
        write!(
            group,
            r#"<g transform="translate({} {})">{content}"#,
            x * sx, y * sy
        )
        .unwrap();
        if local_pixels {
            write!(group, r#"<animateTransform attributeName="transform" type="translate" {timing} values="{}"/>"#,
                values(&|s| format!("{} {}", number(s.pose.w * x), number(s.pose.h * y)))).unwrap();
        }
        group.push_str("</g>");
    } else {
        group.push_str(content);
    }
    if !local_pixels {
        write!(
        group,
        r#"<animateTransform attributeName="transform" type="scale" {timing} values="{}"/>"#,
        values(&|s| format!("{} {}", number(s.pose.w), number(s.pose.h)))
    )
        .unwrap();
    }
    if artwork {
        write!(
            group,
            r#"<animate attributeName="opacity" {timing} values="{}"/>"#,
            values(&|s| number(s.pose.o / 100.0))
        )
        .unwrap();
    }
    group.push_str("</g>");
    if !filter.is_empty() {
        group.push_str("</g>");
    }
    if filtered_anchor.is_some() {
        group.push_str("</g>");
    }
    write!(
        group,
        r#"<animateTransform attributeName="transform" type="rotate" {timing} values="{}"/></g>"#,
        values(&|s| number(s.pose.r))
    )
    .unwrap();
    write!(
        group,
        r#"<animateTransform attributeName="transform" type="translate" {timing} values="{}"/>"#,
        values(&|s| format!("{} {}", number(s.pose.x), number(s.pose.y)))
    )
    .unwrap();
    if artwork && !shape.color_keys.is_empty() {
        write!(
            group,
            r#"<animate attributeName="fill" {timing} values="{}"/>"#,
            attr(&values(&|s| s.color.clone()))
        )
        .unwrap();
    }
    group.push_str("</g>");
    group
}

fn svg_matrix([a, b, c, d, e, f]: [f32; 6]) -> String {
    format!("matrix({a} {b} {c} {d} {e} {f})")
}

/// The scene at `t` as a still SVG (Electron's `svgText(false)`): each layer shown then, posed as it is then.
pub fn still(scene: &Scene, place: &Place, t: f64) -> Result<String, String> {
    if scene.shapes.iter().any(|shape| {
        shape.kind == "adjustment" || shape.effects.iter().any(|effect| effect.enabled)
    }) || !crate::motion_blur::exposure(scene, t, crate::motion_blur::DEFAULT_FPS).is_empty()
    {
        let picture = crate::render::frame(scene, place, t)?;
        let png = picture
            .encode_png()
            .map_err(|error| format!("Could not encode the SVG picture: {error}"))?;
        let png = base64::engine::general_purpose::STANDARD.encode(png);
        let (w, h) = (scene.width, scene.height);
        return Ok(format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><image width="{w}" height="{h}" href="data:image/png;base64,{png}"/></svg>"#
        ));
    }
    // A matte may hide a layer even while its consumer is outside its clip.
    for shape in &scene.shapes {
        compositing_style(shape, "Still SVG")?;
    }
    let mut defs = String::new();
    let mut layers = String::new();
    let mut renderer = None;
    for (index, shape) in scene.shapes.iter().enumerate().filter(|(_, s)| s.visible(t, scene.duration)) {
        let frame = scene.layer_frame(shape, t);
        let p = frame.pose;
        let mut group = format!(r#"<g fill="{}""#, attr(&shape.color_at(t)));
        group.push_str(&compositing_style(shape, "Still SVG")?);
        if TextInk::used_by(shape) {
            let renderer =
                renderer.get_or_insert_with(|| crate::render::Renderer::new(scene, place));
            let Some(pixels) = renderer.still_text(scene, shape, t)? else {
                continue;
            };
            let rgba: Vec<_> = pixels
                .pixmap
                .pixels()
                .iter()
                .flat_map(|pixel| {
                    let pixel = pixel.demultiply();
                    [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
                })
                .collect();
            let mut png = Vec::new();
            use image::ImageEncoder;
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(
                    &rgba,
                    pixels.pixmap.width(),
                    pixels.pixmap.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|error| format!("{}: {error}", shape.name))?;
            let png = base64::engine::general_purpose::STANDARD.encode(png);
            let area = pixels.area;
            write!(layers, r#"{group}><image x="{}" y="{}" width="{}" height="{}" href="data:image/png;base64,{png}"/></g>"#,
                area.x(), area.y(), area.width(), area.height()).unwrap();
            continue;
        }
        if let Some(geometry) = crate::vector_svg::geometry(scene, shape, index, &mut defs, Some(t))? {
            group.push_str(&canvas_blur_filter(scene, shape, t, &[], &mut defs));
            let ts = frame.transform;
            let matrix = svg_matrix([ts.sx, ts.ky, ts.kx, ts.sy, ts.tx, ts.ty]);
            write!(group, r#"><g transform="{matrix}" opacity="{}">{geometry}</g></g>"#, p.o / 100.0).unwrap();
        } else if frame.space.is_identity() && shape.anchor.is_none() {
            group.push_str(&blur_filter(shape, p, &mut defs));
            write!(group, r#" transform="translate({} {})"><g transform="rotate({})"><g transform="scale({} {})" opacity="{}">"#,
                p.x, p.y, p.r, p.w, p.h, p.o / 100.0).unwrap();
            group.push_str(&geometry(shape, place)?);
            group.push_str("</g></g></g>");
        } else {
            group.push_str(&canvas_blur_filter(scene, shape, t, &[], &mut defs));
            let ts = frame.box_transform();
            let matrix = svg_matrix([ts.sx, ts.ky, ts.kx, ts.sy, ts.tx, ts.ty]);
            write!(
                group,
                r#"><g transform="{matrix}" opacity="{}">{}</g></g>"#,
                p.o / 100.0,
                geometry(shape, place)?
            )
            .unwrap();
        }
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

fn compositing_style(shape: &Shape, export: &str) -> Result<String, String> {
    if !shape.masks.is_empty() {
        return Err(format!(
            "{}: {export} can't export layer masks.",
            shape.name
        ));
    }
    if shape.track_matte.is_some() {
        return Err(format!(
            "{}: {export} can't export track mattes.",
            shape.name
        ));
    }
    let blend = shape.blend();
    if blend == Blend::Normal {
        return Ok(String::new());
    }
    let css = blend.svg().ok_or_else(|| {
        format!(
            "{}: {export} can't export the {} blend mode.",
            shape.name,
            blend.label()
        )
    })?;
    Ok(format!(r#" style="mix-blend-mode:{css}""#))
}

/// A blurred layer's filter, added to `defs`; the attribute that puts it on the layer's group (empty for no blur).
fn blur_filter(shape: &Shape, p: Pose, defs: &mut String) -> String {
    let blur = f64::from(crate::render::blur(shape));
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

/// Nested layers are filtered in canvas coordinates, with the same radius as the raster renderer.
fn canvas_blur_filter(
    scene: &Scene,
    shape: &Shape,
    at: f64,
    chain: &[&Shape],
    defs: &mut String,
) -> String {
    let blur = crate::render::blur(shape);
    if blur <= 0.0 {
        return String::new();
    }
    let radius = |t| blur * crate::transform::max_scale(scene.layer_frame(shape, t).space);
    let first = radius(at);
    let mut samples = vec![(at, first)];
    if chain.iter().skip(1).any(|layer| {
        layer.keys.len() > 1 || !layer.rotation_clips.is_empty() || link(scene, layer).is_some()
    }) {
        let duration = scene.duration;
        let count = (duration * 60.0).ceil().clamp(1.0, 12000.0) as u32;
        let mut times: Vec<_> = (1..=count)
            .map(|i| duration * f64::from(i) / f64::from(count))
            .collect();
        for layer in chain.iter().skip(1) {
            times.extend(
                event_times(scene, layer, &mut vec![layer.id.clone()])
                    .into_iter()
                    .filter(|time| *time > 0.0 && *time <= duration),
            );
        }
        times.sort_by(f64::total_cmp);
        times.dedup();
        for t in times {
            let (before, current) = (radius(t - EPSILON), radius(t));
            if (before - current).abs() > JUMP {
                samples.push((t, before));
            }
            samples.push((t, current));
        }
    }
    let largest = samples
        .iter()
        .map(|(_, sigma)| *sigma)
        .fold(first, f32::max);
    let margin = largest * 3.0 + 4.0;
    let id = attr(&format!("blur-{}", shape.id));
    write!(defs, r#"<filter id="{id}" filterUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}"><feGaussianBlur stdDeviation="{first}""#,
        -margin, -margin, scene.width + 2.0 * margin, scene.height + 2.0 * margin).unwrap();
    if samples
        .iter()
        .any(|(_, sigma)| (*sigma - first).abs() > 0.001)
    {
        let times = samples
            .iter()
            .map(|(t, _)| fixed(t / scene.duration, 6))
            .collect::<Vec<_>>();
        let radii = samples
            .iter()
            .map(|(_, sigma)| fixed(f64::from(*sigma), 3))
            .collect::<Vec<_>>();
        write!(defs, r#"><animate attributeName="stdDeviation" dur="{}s" repeatCount="indefinite" calcMode="linear" keyTimes="{}" values="{}"/></feGaussianBlur>"#,
            scene.duration, times.join(";"), radii.join(";")).unwrap();
    } else {
        defs.push_str("/>");
    }
    defs.push_str("</filter>");
    format!(r#" filter="url(#{id})""#)
}

fn shaped_geometry(shape: &Shape, cache: &mut TextCache) -> Result<String, String> {
    let unsupported = |feature| format!("{}: animated SVG can't export {feature}.", shape.name);
    let Some(style) = shape.text_layout.as_ref() else {
        return Err(unsupported("text fill and stroke styling"));
    };
    if !style.animators.is_empty() {
        return Err(unsupported("per-glyph text animators"));
    }
    if style.path.is_some() {
        return Err(unsupported("text along a path"));
    }
    if shape.vector.is_some() {
        return Err(unsupported("text fill and stroke styling"));
    }
    if style
        .style
        .get(&TextProperty::Tracking)
        .is_some_and(|track| track.rest[0] != 0.0 || !track.keys.is_empty())
    {
        return Err(unsupported("text tracking"));
    }
    if style
        .style
        .get(&TextProperty::LineHeight)
        .is_some_and(|track| !track.keys.is_empty())
    {
        return Err(unsupported("animated text line height"));
    }
    let layout = cache.layout(&shape.id, text::content(shape), style);
    let mut geometry = format!("<g><title>{}</title>", text_content(text::content(shape)));
    let stretch = 1.0 / layout.width.max(0.001);
    let leading = TextLane::Style(TextProperty::LineHeight).values(shape, 0.0)[0] / 100.0;
    for (i, line) in layout.lines.iter().enumerate() {
        let x = -0.5 + style.alignment.inset(1.0 - line.width * stretch);
        let y = layout.baseline
            + (i as f32 - layout.lines.len().saturating_sub(1) as f32 / 2.0) * leading;
        for glyph in &line.glyphs {
            if let Some(path) = &glyph.path {
                let outline = crate::paths::PathData::from_outline(path, [1.0, 1.0]);
                let d = outline.svg([1.0, 1.0], false);
                write!(
                    geometry,
                    r#"<path transform="translate({} {}) scale({stretch} 1)" d="{d}"/>"#,
                    x + glyph.x * stretch,
                    y + glyph.y
                )
                .unwrap();
            }
        }
    }
    geometry.push_str("</g>");
    Ok(geometry)
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
        pose: scene.local_pose(shape, t),
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
    use crate::keyframes::Property;

    let mid = (a + b) / 2.0;
    let mut opacity_changes = false;
    for property in Property::ALL {
        let Some(ka) = shape
            .keys
            .iter()
            .rev()
            .find(|k| k.sets(property) && k.t <= mid)
        else {
            continue;
        };
        let Some(kb) = shape.keys.iter().find(|k| k.sets(property) && k.t >= mid) else {
            continue;
        };
        let changing = property.values(Pose::from(ka)) != property.values(Pose::from(kb));
        let kind = ka.ease.as_deref().unwrap_or(&shape.ease);
        if changing && kind != "linear" && kind != "hold" {
            return true;
        }
        if property == Property::Opacity {
            opacity_changes = changing;
        }
    }
    if shape.rotation_clips.iter().any(|c| {
        c.degrees != 0.0 && c.ease != "linear" && c.start_time < b && c.start_time + c.duration > a
    }) {
        return true;
    }
    if let Some(d) = &shape.disappear {
        if d.duration > 0.0 && d.start_time < b && d.start_time + d.duration > a && opacity_changes
        {
            return true;
        }
    }
    let color_ease = shape
        .color_keys
        .iter()
        .rev()
        .find(|key| key.t <= mid)
        .and_then(|key| key.ease.as_deref())
        .unwrap_or(&shape.ease);
    shape.color_at(a + EPSILON) != shape.color_at(b - EPSILON)
        && color_ease != "linear"
        && color_ease != "hold"
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
    fn svg_blur_follows_animated_parent_scale() {
        let mut scene = Scene::empty("svg-blur".into());
        scene.add_shape("parent".into(), "rect");
        scene.add_shape("child".into(), "circle");
        scene.shapes[1].extra.insert("blur".into(), 60.into());
        scene.set_parent(1, Some(0), 0.0).unwrap();
        let parent = &mut scene.shapes[0];
        let pose = parent.pose(0.0);
        parent.edit_pose(
            1.0,
            Pose {
                w: 2.0 * pose.w,
                h: 2.0 * pose.h,
                ..pose
            },
        );

        let svg = animated(&scene, &place()).unwrap();
        let doc = roxmltree::Document::parse(&svg).unwrap();
        let blur = doc
            .descendants()
            .find(|n| n.has_tag_name("feGaussianBlur"))
            .unwrap();
        assert_eq!(blur.attribute("stdDeviation"), Some("60"));
        let motion = blur.first_element_child().unwrap();
        assert_eq!(motion.attribute("attributeName"), Some("stdDeviation"));
        assert_eq!(
            motion.attribute("values").unwrap().rsplit(';').next(),
            Some("120")
        );
        let svg = still(&scene, &place(), 1.0).unwrap();
        assert!(svg.contains(r#"<feGaussianBlur stdDeviation="120"/>"#));
        assert!(crate::svg::parse(&svg).is_ok());
    }

    #[test]
    fn exported_hierarchy_keeps_the_anchor_and_uses_the_property_curves() {
        use crate::keyframes::{Lane, Property};
        use crate::transform::Anchor;

        let mut scene = Scene::empty("svg-hierarchy".into());
        scene.add_shape("parent".into(), "rect");
        scene.add_shape("child".into(), "circle");
        scene.shapes[0].keys[0].o = 0.0;
        scene.shapes[1].keys[0].x += 70.0;
        scene.set_parent(1, Some(0), 0.0).unwrap();
        scene.set_anchor(1, Anchor { x: 0.0, y: 0.0 }, 0.0);
        scene.shapes[0].keys[0].r = 90.0;

        let child = &mut scene.shapes[1];
        let lane = Lane::Pose(Property::Rotation);
        let mut key = child.lane_key(lane, 0.0).unwrap();
        key.set_easing("hold", None);
        child.put_lane_key(0.0, key);
        child.key_property(
            2.0,
            Property::Rotation,
            Pose {
                r: 90.0,
                ..child.pose(0.0)
            },
        );
        let jump: Vec<_> = sample_layer(&scene, &scene.shapes[1])
            .into_iter()
            .filter_map(|s| (s.t == 2.0).then_some(s.pose.r))
            .collect();
        assert_eq!(jump, [0.0, 90.0]);

        let svg = animated(&scene, &place()).unwrap();
        assert!(svg.contains(r#"transform="translate(0.5 0.5)""#));
        assert!(svg.contains(r#"transform="matrix("#));
        let svg = roxmltree::Document::parse(&svg).unwrap();
        assert_eq!(
            svg.descendants()
                .filter(|n| n.attribute("type") == Some("rotate"))
                .count(),
            3
        );
        assert_eq!(
            svg.descendants()
                .filter(|n| n.attribute("attributeName") == Some("opacity"))
                .count(),
            2
        );
    }

    #[test]
    fn animated_svg_names_a_linked_text_layer_it_cannot_export() {
        let mut scene = Scene::example("linked-svg".into());
        scene.add_linked_text("title".into(), 0);
        scene.set_parent(0, Some(1), 0.0).unwrap();
        let error = animated(&scene, &place()).unwrap_err();
        assert!(error.contains(&scene.shapes[3].name));
        assert!(error.contains("text linked to a transformed layer"));
        assert!(crate::svg::parse(&still(&scene, &place(), 2.0).unwrap()).is_ok());
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
