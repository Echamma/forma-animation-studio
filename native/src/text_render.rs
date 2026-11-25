use crate::model::{Scene, Shape};
use crate::paths::{MeasuredPath, PathSampler};
use crate::shape_style::{PaintTarget, StrokeStyle, VectorLayer};
use crate::text_animator::{Alignment, Animator, TextLane, TextLayout, TextProperty, Units};
use crate::text_layout::{Cluster, ShapedText, TextCache};
use crate::transform::{LayerFrame, axis_scales, max_scale, point, vector};
use crate::vector_render::{GeometryCache, solid, stroke_style, tinted_paint};
use std::collections::HashMap;
use std::sync::Arc;
use tiny_skia::{
    FillRule, IntRect, Paint, Path, Pixmap, PixmapPaint, Point, Rect, Shader, Stroke, StrokeDash,
    Transform,
};

#[derive(Default)]
pub(crate) struct TextPaths(HashMap<String, (Path, Option<PathSampler>)>);

impl TextPaths {
    fn get(&mut self, id: &str, path: Path) -> Option<&PathSampler> {
        if self.0.len() >= 64 && !self.0.contains_key(id) {
            self.0.clear();
        }
        let measure = |path: &Path| MeasuredPath::new(path).first_contour();
        let entry = self.0.entry(id.to_owned()).or_insert_with(|| {
            let sampler = measure(&path);
            (path.clone(), sampler)
        });
        if entry.0 != path {
            *entry = (path, None);
            entry.1 = measure(&entry.0);
        }
        entry.1.as_ref()
    }
}

struct FollowPath<'a> {
    sampler: &'a PathSampler,
    origin: (f32, f32),
    local: Transform,
    across: f32,
    offset: f32,
}

impl<'a> FollowPath<'a> {
    fn new(
        scene: &Scene,
        text: &Shape,
        frame: LayerFrame,
        time: f64,
        geometry: &mut GeometryCache,
        paths: &'a mut TextPaths,
    ) -> Option<Self> {
        let id = text.text_layout.as_ref()?.path.as_ref()?;
        let path = scene
            .shapes
            .iter()
            .find(|layer| &layer.id == id && layer.kind == "path")?;
        let target = scene.layer_frame(path, time);
        let mut axes = target.transform;
        let origin = (axes.tx, axes.ty);
        (axes.tx, axes.ty) = (0.0, 0.0);
        let outline = geometry.outline(
            path,
            path.vector.as_ref()?,
            [target.pose.w, target.pose.h],
            time,
        )?;
        let sampler = paths.get(id, outline.transform(axes)?)?;
        Some(Self {
            sampler,
            origin,
            local: frame.transform.invert()?,
            across: axis_scales(frame.transform).0,
            offset: TextLane::Style(TextProperty::PathOffset).values(text, time)[0],
        })
    }

    fn at(&self, x: f32, width: f32, alignment: Alignment) -> Option<Transform> {
        let free = self.sampler.length() - width * self.across;
        let start = alignment.inset(free);
        let (position, tangent) = self.sampler.at(start + self.offset + x * self.across)?;
        let (x, y) = point(
            self.local,
            position.x as f32 + self.origin.0,
            position.y as f32 + self.origin.1,
        );
        let (dx, dy) = vector(self.local, tangent.x as f32, tangent.y as f32);
        Some(Transform::from_rotate(dy.atan2(dx).to_degrees()).post_translate(x, y))
    }
}

#[derive(Clone, Copy, Default)]
struct Tint {
    weight: f32,
    color: [f32; 3],
}

impl Tint {
    fn add(&mut self, target: [f32; 3], amount: f32) {
        self.weight += (1.0 - self.weight) * amount;
        for (channel, target) in self.color.iter_mut().zip(target) {
            *channel += (target - *channel) * amount;
        }
    }

    fn apply(self, mut rgba: [f32; 4]) -> [f32; 4] {
        for (channel, tint) in rgba.iter_mut().zip(self.color) {
            *channel = *channel * (1.0 - self.weight) + tint;
        }
        rgba
    }
}

#[derive(Clone, Copy)]
pub(crate) struct GlyphEffect {
    pub position: [f32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
    pub opacity: f32,
    pub tracking: f32,
    pub blur: f32,
    pub skew: f32,
    fill: Tint,
    stroke: Tint,
}

impl Default for GlyphEffect {
    fn default() -> Self {
        Self {
            position: [0.0; 2],
            scale: [1.0; 2],
            rotation: 0.0,
            opacity: 1.0,
            tracking: 0.0,
            blur: 0.0,
            skew: 0.0,
            fill: Tint::default(),
            stroke: Tint::default(),
        }
    }
}

impl GlyphEffect {
    fn apply(&mut self, property: TextProperty, values: [f32; 3], amount: f32) {
        let [a, b, _] = values;
        match property {
            TextProperty::Position => {
                self.position[0] += a * amount;
                self.position[1] += b * amount;
            }
            TextProperty::Scale => {
                self.scale[0] *= 1.0 + (a / 100.0 - 1.0) * amount;
                self.scale[1] *= 1.0 + (b / 100.0 - 1.0) * amount;
            }
            TextProperty::Rotation => self.rotation += a * amount,
            TextProperty::Opacity => self.opacity *= 1.0 + (a / 100.0 - 1.0) * amount,
            TextProperty::Tracking => self.tracking += a * amount,
            TextProperty::Blur => self.blur += a * amount,
            TextProperty::Skew => self.skew += a * amount,
            TextProperty::Fill => self.fill.add(values, amount),
            TextProperty::Stroke => self.stroke.add(values, amount),
            _ => {}
        }
    }

    fn transform(self, pivot: (f32, f32)) -> Transform {
        let skew = self.skew.clamp(-85.0, 85.0).to_radians().tan();
        Transform::from_translate(-pivot.0, -pivot.1)
            .post_scale(self.scale[0], self.scale[1])
            .post_concat(Transform::from_row(1.0, 0.0, skew, 1.0, 0.0, 0.0))
            .post_rotate(self.rotation)
            .post_translate(pivot.0 + self.position[0], pivot.1 + self.position[1])
    }
}

pub(crate) struct SampledAnimator {
    units: Units,
    weights: Vec<f32>,
    effects: Vec<(TextProperty, [f32; 3])>,
}

impl SampledAnimator {
    pub fn new(animator: &Animator, shaped: &ShapedText, time: f64, ease: &str) -> Self {
        Self {
            units: animator.units,
            weights: animator.weights(shaped.count(animator.units), time, ease),
            effects: TextProperty::EFFECTS
                .into_iter()
                .filter_map(|property| {
                    animator
                        .tracks
                        .get(&property)
                        .map(|track| (property, track.at(property, time, ease)))
                })
                .collect(),
        }
    }
}

impl GlyphEffect {
    pub fn at(animators: &[SampledAnimator], cluster: &Cluster) -> Self {
        let mut result = Self::default();
        for animator in animators {
            if let Some(amount) = cluster
                .unit(animator.units)
                .and_then(|i| animator.weights.get(i))
                .copied()
                .filter(|&amount| amount > 0.0)
            {
                for &(property, values) in &animator.effects {
                    result.apply(property, values, amount);
                }
            }
        }
        result
    }
}

struct TextPaint<'a> {
    vector: &'a VectorLayer,
    time: f64,
    ease: &'a str,
    size: [f32; 2],
    fill: [f32; 4],
    stroke: Option<StrokeStyle>,
    stroke_geometry: Option<(Stroke, Option<StrokeDash>)>,
}

impl<'a> TextPaint<'a> {
    fn new(
        shape: &'a Shape,
        vector: &'a VectorLayer,
        frame: LayerFrame,
        color: &str,
        time: f64,
    ) -> Self {
        let [r, g, b] = crate::render::rgb(color).map(f32::from);
        let stroke = vector
            .stroke_at(time, &shape.ease)
            .filter(|style| style.width > 0.0);
        let stroke_geometry = stroke.as_ref().map(|style| {
            let dash = style
                .dash
                .and_then(|dash| StrokeDash::new(vec![dash.length, dash.gap], dash.offset));
            (stroke_style(style), dash)
        });
        Self {
            vector,
            time,
            ease: &shape.ease,
            size: [frame.pose.w, frame.pose.h],
            fill: [r, g, b, 100.0],
            stroke,
            stroke_geometry,
        }
    }

    fn paint(&self, target: PaintTarget, tint: Tint) -> Option<Paint<'static>> {
        let rgba = match target {
            PaintTarget::Fill => self.fill,
            PaintTarget::Stroke => self.stroke.as_ref()?.rgba,
        };
        tinted_paint(
            self.vector,
            target,
            self.time,
            self.ease,
            self.size,
            solid(tint.apply(rgba)),
            |rgba| tint.apply(rgba),
        )
    }

    fn stroke(&self, outline: &Path, scale: f32) -> Option<Path> {
        let (stroke, dash) = self.stroke_geometry.as_ref()?;
        if let Some(dash) = dash {
            outline.dash(dash, scale)?.stroke(stroke, scale)
        } else {
            outline.stroke(stroke, scale)
        }
    }
}

pub(crate) struct TextInk {
    glyphs: Vec<GlyphInk>,
    place: Transform,
    pub opacity: f32,
}

type PaintedPath = (Path, Paint<'static>);

enum GlyphPaths {
    Solid(Arc<Path>, Paint<'static>, Transform),
    Styled {
        fill: Option<PaintedPath>,
        stroke: Option<PaintedPath>,
    },
}

struct GlyphInk {
    paths: GlyphPaths,
    blur: f32,
    opacity: f32,
    bounds: Rect,
}

fn leading(shape: &Shape, size: f32, time: f64) -> f32 {
    size * TextLane::Style(TextProperty::LineHeight).values(shape, time)[0] / 100.0
}

pub(crate) fn selection_height_factor(shape: &Shape, time: f64) -> f32 {
    if !TextInk::used_by(shape) {
        return 1.0;
    }
    let extra_lines = crate::text::content(shape)
        .bytes()
        .filter(|&byte| byte == b'\n')
        .count();
    1.0 + extra_lines as f32 * leading(shape, 1.0, time)
}

impl TextInk {
    pub(crate) fn used_by(shape: &Shape) -> bool {
        shape.kind == "text" && (shape.text_layout.is_some() || shape.vector.is_some())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scene: &Scene,
        shape: &Shape,
        frame: LayerFrame,
        view: Transform,
        color: &str,
        time: f64,
        cache: &mut TextCache,
        geometry: &mut GeometryCache,
        paths: &mut TextPaths,
    ) -> Option<Self> {
        let default = TextLayout::default();
        let layout = shape.text_layout.as_ref().unwrap_or(&default);
        let shaped = cache.layout(&shape.id, crate::text::content(shape), layout);
        let follow = FollowPath::new(scene, shape, frame, time, geometry, paths);
        let animators: Vec<_> = layout
            .animators
            .iter()
            .filter(|animator| {
                TextProperty::EFFECTS
                    .iter()
                    .any(|property| animator.tracks.contains_key(property))
            })
            .map(|animator| SampledAnimator::new(animator, &shaped, time, &shape.ease))
            .collect();
        let default_paint = VectorLayer::default();
        let paints = TextPaint::new(
            shape,
            shape.vector.as_ref().unwrap_or(&default_paint),
            frame,
            color,
            time,
        );
        let place = frame.transform.post_concat(view);
        let style_value = |p| TextLane::Style(p).values(shape, time)[0];
        let size = frame.pose.h;
        let params = Typesetting {
            width: frame.pose.w,
            size,
            stretch: frame.pose.w / shaped.width.max(0.001),
            leading: leading(shape, size, time),
            tracking: style_value(TextProperty::Tracking),
            alignment: layout.alignment,
        };
        let mut glyphs = Vec::new();
        let common = Paints {
            fill: paints.paint(PaintTarget::Fill, Tint::default()),
            stroke: paints.paint(PaintTarget::Stroke, Tint::default()),
        };
        let coordinates = GlyphCoordinates {
            place,
            blur_scale: max_scale(place),
        };
        for (line_number, line) in shaped.lines.iter().enumerate() {
            let effects: Vec<_> = line
                .clusters
                .iter()
                .map(|cluster| GlyphEffect::at(&animators, cluster))
                .collect();
            let mut layout = params.line(line, &effects, line_number, &shaped);
            for (cluster, effect) in line.clusters.iter().zip(effects) {
                let Some((x, y, motion)) = layout.cluster(cluster, effect, follow.as_ref()) else {
                    continue;
                };
                let glyph_paints = common.for_effect(&paints, effect);
                for glyph in &line.glyphs[cluster.glyphs.clone()] {
                    let Some(path) = &glyph.path else { continue };
                    let transform = Transform::from_scale(params.stretch, size)
                        .post_translate(glyph.x * params.stretch + x, glyph.y * size + y);
                    if let Some(ink) = GlyphInk::new(
                        path,
                        transform,
                        effect,
                        motion,
                        coordinates,
                        &paints,
                        &glyph_paints,
                    ) {
                        glyphs.push(ink);
                    }
                }
            }
        }
        (!glyphs.is_empty()).then_some(Self {
            glyphs,
            place,
            opacity: (frame.pose.o / 100.0).clamp(0.0, 1.0),
        })
    }

    pub fn bounds(&self) -> Option<Rect> {
        self.glyphs.iter().map(|glyph| glyph.bounds).reduce(union)
    }

    pub(crate) fn outlined(&self, size: [f32; 2]) -> crate::paths::PathData {
        let contours = self.glyphs.iter().filter_map(|glyph| match &glyph.paths {
            GlyphPaths::Solid(path, _, transform) => (**path).clone().transform(*transform),
            GlyphPaths::Styled { .. } => None,
        }).flat_map(|path| crate::paths::PathData::from_outline(&path, size).contours).collect();
        crate::paths::PathData { contours }
    }

    pub(crate) fn hit_boxes(&self) -> Vec<Rect> {
        self.glyphs
            .iter()
            .filter_map(|glyph| outset(glyph.bounds, 2.0 - (glyph.blur * 3.0).ceil()))
            .collect()
    }

    pub fn paint(&self, target: &mut Pixmap, offset: Transform) {
        let place = self.place.post_concat(offset);
        for glyph in &self.glyphs {
            glyph.paint(target, place, offset);
        }
    }
}

struct Typesetting {
    width: f32,
    size: f32,
    stretch: f32,
    leading: f32,
    tracking: f32,
    alignment: Alignment,
}

impl Typesetting {
    fn line<'a>(
        &'a self,
        line: &crate::text_layout::Line,
        effects: &[GlyphEffect],
        index: usize,
        shaped: &ShapedText,
    ) -> LinePlacement<'a> {
        let width = line.width * self.stretch
            + effects
                .iter()
                .take(effects.len().saturating_sub(1))
                .map(|effect| self.tracking + effect.tracking)
                .sum::<f32>();
        let row = (index as f32 - shaped.lines.len().saturating_sub(1) as f32 * 0.5) * self.leading;
        LinePlacement {
            params: self,
            width,
            row,
            left: -self.width * 0.5 + self.alignment.inset(self.width - width),
            baseline: shaped.baseline * self.size + row,
            tracking: 0.0,
        }
    }
}

struct LinePlacement<'a> {
    params: &'a Typesetting,
    width: f32,
    left: f32,
    row: f32,
    baseline: f32,
    tracking: f32,
}

impl LinePlacement<'_> {
    fn cluster(
        &mut self,
        cluster: &Cluster,
        effect: GlyphEffect,
        follow: Option<&FollowPath<'_>>,
    ) -> Option<(f32, f32, Transform)> {
        let shift = self.tracking;
        self.tracking += self.params.tracking + effect.tracking;
        if effect.opacity <= 0.0 || effect.scale.iter().any(|size| *size <= 0.0001) {
            return None;
        }
        let center = (cluster.x + cluster.advance * 0.5) * self.params.stretch + shift;
        let pivot_height = self.params.size * 0.35;
        if let Some(follow) = follow {
            let on_path = follow.at(center, self.width, self.params.alignment)?;
            let motion = effect
                .transform((0.0, self.row - pivot_height))
                .post_concat(on_path);
            Some((-center + shift, self.row, motion))
        } else {
            let motion = effect.transform((self.left + center, self.baseline - pivot_height));
            Some((self.left + shift, self.baseline, motion))
        }
    }
}

#[derive(Clone, Copy)]
struct GlyphCoordinates {
    place: Transform,
    blur_scale: f32,
}

struct Paints {
    fill: Option<Paint<'static>>,
    stroke: Option<Paint<'static>>,
}

impl Paints {
    fn for_effect(&self, base: &TextPaint<'_>, effect: GlyphEffect) -> Self {
        let tinted = |target, tint: Tint, plain: &Option<Paint<'static>>| {
            if tint.weight > 0.0 {
                base.paint(target, tint)
            } else {
                plain.clone()
            }
        };
        Self {
            fill: tinted(PaintTarget::Fill, effect.fill, &self.fill),
            stroke: tinted(PaintTarget::Stroke, effect.stroke, &self.stroke),
        }
    }
}

impl GlyphInk {
    fn new(
        path: &Arc<Path>,
        glyph: Transform,
        effect: GlyphEffect,
        motion: Transform,
        coordinates: GlyphCoordinates,
        style: &TextPaint<'_>,
        paints: &Paints,
    ) -> Option<Self> {
        let place = coordinates.place;
        let (paths, bounds) = if paints.stroke.is_none()
            && let Some(paint) = paints
                .fill
                .as_ref()
                .filter(|paint| matches!(paint.shader, Shader::SolidColor(_)))
        {
            let transform = glyph.post_concat(motion);
            let bounds = map_bounds(path.bounds(), transform.post_concat(place))?;
            (
                GlyphPaths::Solid(path.clone(), paint.clone(), transform),
                bounds,
            )
        } else {
            let outline = path.as_ref().clone().transform(glyph)?;
            let stroke = paints.stroke.as_ref().and_then(|paint| {
                let scale = max_scale(motion.post_concat(place)).clamp(0.15, 32.0);
                Some((
                    style.stroke(&outline, scale)?.transform(motion)?,
                    paint.clone(),
                ))
            });
            let fill = paints
                .fill
                .as_ref()
                .and_then(|paint| Some((outline.transform(motion)?, paint.clone())));
            let bounds = fill
                .iter()
                .chain(stroke.iter())
                .filter_map(|(path, _)| map_bounds(path.bounds(), place))
                .reduce(union)?;
            (GlyphPaths::Styled { fill, stroke }, bounds)
        };
        let blur = (effect.blur.clamp(0.0, 100.0) * coordinates.blur_scale).clamp(0.0, 300.0);
        let bounds = outset(bounds, (blur * 3.0).ceil())?;
        Some(Self {
            paths,
            blur,
            opacity: effect.opacity,
            bounds,
        })
    }

    fn paint(&self, target: &mut Pixmap, place: Transform, offset: Transform) {
        let draw = |target: &mut Pixmap, place| match &self.paths {
            GlyphPaths::Solid(path, paint, transform) => {
                target.fill_path(
                    path,
                    paint,
                    FillRule::Winding,
                    transform.post_concat(place),
                    None,
                );
            }
            GlyphPaths::Styled { fill, stroke } => {
                for (path, paint) in fill.iter().chain(stroke.iter()) {
                    target.fill_path(path, paint, FillRule::Winding, place, None);
                }
            }
        };
        if self.opacity >= 1.0 && self.blur <= 0.0 {
            draw(target, place);
            return;
        }
        let Some(bounds) = map_bounds(self.bounds, offset) else {
            return;
        };
        let margin = (self.blur * 3.0).ceil() + 2.0;
        let Some(area) = IntRect::from_ltrb(
            (bounds.left().floor() - 1.0).max(-margin) as i32,
            (bounds.top().floor() - 1.0).max(-margin) as i32,
            (bounds.right().ceil() + 1.0).min(target.width() as f32 + margin) as i32,
            (bounds.bottom().ceil() + 1.0).min(target.height() as f32 + margin) as i32,
        ) else {
            return;
        };
        let Some(mut glyph) = Pixmap::new(area.width(), area.height()) else {
            return;
        };
        draw(
            &mut glyph,
            place.post_translate(-(area.x() as f32), -(area.y() as f32)),
        );
        if self.blur > 0.0 {
            crate::render::gaussian_blur(&mut glyph, self.blur);
        }
        target.draw_pixmap(
            area.x(),
            area.y(),
            glyph.as_ref(),
            &PixmapPaint {
                opacity: self.opacity,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }
}

fn map_bounds(rect: Rect, transform: Transform) -> Option<Rect> {
    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let mut corners = [
        Point::from_xy(l, t),
        Point::from_xy(r, t),
        Point::from_xy(r, b),
        Point::from_xy(l, b),
    ];
    transform.map_points(&mut corners);
    Rect::from_points(&corners)
}

pub(crate) fn union(a: Rect, b: Rect) -> Rect {
    Rect::from_ltrb(
        a.left().min(b.left()),
        a.top().min(b.top()),
        a.right().max(b.right()),
        a.bottom().max(b.bottom()),
    )
    .unwrap_or(a)
}

fn outset(rect: Rect, margin: f32) -> Option<Rect> {
    Rect::from_ltrb(
        rect.left() - margin,
        rect.top() - margin,
        rect.right() + margin,
        rect.bottom() + margin,
    )
}
