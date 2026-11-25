use crate::model::Shape;
use crate::paths::{MeasuredPath, PathData};
use crate::shape_style::{
    Cap, Gradient, Join, PaintMode, PaintTarget, VectorLayer, VectorProperty, VectorValue,
};
use crate::transform::{LayerFrame, max_scale};
use std::collections::HashMap;
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder,
    Pixmap, Point, RadialGradient, Rect, SpreadMode, Stroke, StrokeDash, Transform,
};

#[derive(Default)]
pub(crate) struct GeometryCache {
    measured: HashMap<String, (Path, MeasuredPath)>,
    morphs: HashMap<String, Morph>,
}

struct Morph {
    before: PathData,
    after: PathData,
    matched: (PathData, PathData),
}

impl GeometryCache {
    fn measured(&mut self, layer: &str, path: &Path) -> &MeasuredPath {
        if self.measured.len() >= 128 && !self.measured.contains_key(layer) {
            self.measured.clear();
        }
        let entry = self
            .measured
            .entry(layer.to_owned())
            .or_insert_with(|| (path.clone(), MeasuredPath::new(path)));
        if entry.0 != *path {
            *entry = (path.clone(), MeasuredPath::new(path));
        }
        &entry.1
    }

    fn morph(&mut self, layer: &str, a: &PathData, b: &PathData, fraction: f32) -> PathData {
        if self.morphs.len() >= 32 && !self.morphs.contains_key(layer) {
            self.morphs.clear();
        }
        let prepare = || {
            let (mut x, mut y) = (a.clone(), b.clone());
            x.match_topology(&mut y);
            Morph {
                before: a.clone(),
                after: b.clone(),
                matched: (x, y),
            }
        };
        let entry = self.morphs.entry(layer.to_owned()).or_insert_with(prepare);
        if entry.before != *a || entry.after != *b {
            *entry = prepare();
        }
        entry.matched.0.interpolate(&entry.matched.1, fraction)
    }

    fn outline(
        &mut self,
        shape: &Shape,
        vector: &VectorLayer,
        size: [f32; 2],
        t: f64,
    ) -> Option<Path> {
        if shape.kind != "path" || vector.source_shape.is_some() {
            let kind = vector.source_shape.as_deref().unwrap_or(&shape.kind);
            return crate::render::shape_outline(kind, size[0] / 2.0, size[1] / 2.0);
        }
        if let Some((a, b)) = vector.bracket(VectorProperty::Path, t)
            && let (VectorValue::Path(before), VectorValue::Path(after)) = (&a.value, &b.value)
        {
            let fraction = VectorLayer::fraction(a, b, t, &shape.ease);
            return if fraction == 0.0 {
                before.outline(size)
            } else if fraction == 1.0 {
                after.outline(size)
            } else {
                self.morph(&shape.id, before, after, fraction).outline(size)
            };
        }
        vector.path.as_ref()?.outline(size)
    }
}

pub(crate) struct VectorInk {
    parts: Vec<(Path, Paint<'static>)>,
    place: Transform,
    pub opacity: f32,
}

impl VectorInk {
    pub fn new(
        shape: &Shape,
        vector: &VectorLayer,
        frame: LayerFrame,
        place: Transform,
        t: f64,
        color: &str,
        cache: &mut GeometryCache,
    ) -> Option<Self> {
        let size = [frame.pose.w, frame.pose.h];
        let outline = cache.outline(shape, vector, size, t)?;
        let trim = vector.trim_at(t, &shape.ease).filter(|trim| !trim.full());
        let pieces = trim.map(|trim| {
            cache
                .measured(&shape.id, &outline)
                .trim(trim.start, trim.end, trim.offset)
        });
        let mut parts = Vec::with_capacity(2);
        if let Some(paint) = paint(
            vector,
            PaintTarget::Fill,
            t,
            &shape.ease,
            size,
            crate::render::fill(color, 100.0),
        ) {
            let fill = if trim.is_some_and(|trim| trim.fill) {
                let mut builder = PathBuilder::new();
                for (piece, _) in pieces.iter().flatten() {
                    builder.push_path(piece);
                }
                builder.finish()
            } else {
                Some(outline.clone())
            };
            if let Some(fill) = fill {
                parts.push((fill, paint));
            }
        }
        if let Some(style) = vector.stroke_at(t, &shape.ease).filter(|s| s.width > 0.0)
            && let Some(paint) = paint(
                vector,
                PaintTarget::Stroke,
                t,
                &shape.ease,
                size,
                solid(style.rgba),
            )
        {
            let stroke = Stroke {
                width: style.width,
                line_cap: match style.cap {
                    Cap::Butt => LineCap::Butt,
                    Cap::Round => LineCap::Round,
                    Cap::Square => LineCap::Square,
                },
                line_join: match style.join {
                    Join::Miter => LineJoin::Miter,
                    Join::Round => LineJoin::Round,
                    Join::Bevel => LineJoin::Bevel,
                },
                ..Stroke::default()
            };
            let scale = max_scale(place).clamp(0.15, 32.0);
            let mut builder = PathBuilder::new();
            let mut add = |outline: &Path, from: f32| {
                let result = if let Some(dash) = style.dash {
                    StrokeDash::new(vec![dash.length, dash.gap], dash.offset + from)
                        .and_then(|dash| outline.dash(&dash, scale))
                        .and_then(|path| path.stroke(&stroke, scale))
                } else {
                    outline.stroke(&stroke, scale)
                };
                if let Some(result) = result {
                    builder.push_path(&result);
                }
            };
            if let Some(pieces) = &pieces {
                for (piece, from) in pieces {
                    add(piece, *from);
                }
            } else {
                add(&outline, 0.0);
            }
            if let Some(stroked) = builder.finish() {
                parts.push((stroked, paint));
            }
        }
        (!parts.is_empty()).then_some(Self {
            parts,
            place,
            opacity: (frame.pose.o / 100.0).clamp(0.0, 1.0),
        })
    }

    pub fn bounds(&self) -> Option<Rect> {
        let bounds = self
            .parts
            .iter()
            .map(|(path, _)| path.bounds())
            .reduce(|a, b| {
                Rect::from_ltrb(
                    a.left().min(b.left()),
                    a.top().min(b.top()),
                    a.right().max(b.right()),
                    a.bottom().max(b.bottom()),
                )
                .unwrap_or(a)
            })?;
        let mut corners = [
            Point::from_xy(bounds.left(), bounds.top()),
            Point::from_xy(bounds.right(), bounds.top()),
            Point::from_xy(bounds.right(), bounds.bottom()),
            Point::from_xy(bounds.left(), bounds.bottom()),
        ];
        self.place.map_points(&mut corners);
        Rect::from_points(&corners)
    }

    pub fn paint(&self, target: &mut Pixmap, offset: Transform) {
        let transform = self.place.post_concat(offset);
        for (path, paint) in &self.parts {
            target.fill_path(path, paint, FillRule::Winding, transform, None);
        }
    }
}

fn paint(
    vector: &VectorLayer,
    target: PaintTarget,
    t: f64,
    ease: &str,
    size: [f32; 2],
    fallback: Paint<'static>,
) -> Option<Paint<'static>> {
    let mode = vector
        .paint(target)
        .map_or(PaintMode::Solid, |paint| paint.mode);
    match mode {
        PaintMode::None => None,
        PaintMode::Solid => Some(fallback),
        PaintMode::Linear | PaintMode::Radial => {
            let gradient = vector.gradient_at(target, t, ease)?;
            Some(gradient_paint(mode, gradient, size))
        }
    }
}

fn solid(rgba: [f32; 4]) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(skia_color(rgba));
    paint
}

fn skia_color([r, g, b, a]: [f32; 4]) -> Color {
    Color::from_rgba8(
        r.round() as u8,
        g.round() as u8,
        b.round() as u8,
        (a * 2.55).round() as u8,
    )
}

fn gradient_paint(mode: PaintMode, gradient: Gradient, [w, h]: [f32; 2]) -> Paint<'static> {
    let [x, y, a, b] = gradient.ends;
    let (start, end) = (Point::from_xy(x * w, y * h), Point::from_xy(a * w, b * h));
    let radius = (end.x - start.x).hypot(end.y - start.y);
    let last = gradient.stops.last().map_or([0.0; 4], |stop| stop.rgba);
    let stops = gradient
        .stops
        .into_iter()
        .map(|stop| GradientStop::new(stop.offset / 100.0, skia_color(stop.rgba)))
        .collect();
    let shader = if mode == PaintMode::Linear {
        LinearGradient::new(start, end, stops, SpreadMode::Pad, Transform::identity())
    } else {
        RadialGradient::new(
            start,
            start,
            radius,
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        )
    };
    shader.map_or_else(
        || solid(last),
        |shader| Paint {
            shader,
            anti_alias: true,
            ..Paint::default()
        },
    )
}
