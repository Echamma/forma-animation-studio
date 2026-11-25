use kurbo::{BezPath, CubicBez, ParamCurve, ParamCurveArclen, PathEl, PathSeg, Point, Vec2};
use serde::{Deserialize, Serialize};
use tiny_skia::{Path, PathBuilder, PathSegment};

pub const MAX_POINTS: usize = 1024;
const ACCURACY: f64 = 0.01;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathData {
    pub contours: Vec<Contour>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Contour {
    pub points: Vec<Vertex>,
    pub closed: bool,
}

/// Layer-box coordinates, with handles stored as offsets from the vertex.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub point: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incoming: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outgoing: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub smooth: bool,
}

impl Vertex {
    pub fn corner(point: [f32; 2]) -> Self {
        Self {
            point,
            incoming: None,
            outgoing: None,
            smooth: false,
        }
    }

    pub fn pen(point: [f32; 2], handle: Option<[f32; 2]>) -> Self {
        Self {
            point,
            incoming: handle.map(|h| h.map(|v| -v)),
            outgoing: handle,
            smooth: handle.is_some(),
        }
    }

    pub fn handle(&mut self, incoming: bool, value: [f32; 2], split: bool) {
        if split {
            self.smooth = false;
        }
        let (handle, other) = if incoming {
            (&mut self.incoming, &mut self.outgoing)
        } else {
            (&mut self.outgoing, &mut self.incoming)
        };
        *handle = Some(value);
        if self.smooth {
            *other = Some(value.map(|v| -v));
        }
    }

    fn handle_point(&self, incoming: bool) -> Point {
        point(self.point)
            + vector(if incoming {
                self.incoming
            } else {
                self.outgoing
            })
    }
}

impl Contour {
    pub fn segment_count(&self) -> usize {
        self.points.len().saturating_sub(usize::from(!self.closed))
    }

    pub fn segment(&self, index: usize) -> Option<PathSeg> {
        if index >= self.segment_count() {
            return None;
        }
        let a = &self.points[index];
        let b = &self.points[(index + 1) % self.points.len()];
        Some(if a.outgoing.is_none() && b.incoming.is_none() {
            PathSeg::Line(kurbo::Line::new(point(a.point), point(b.point)))
        } else {
            PathSeg::Cubic(CubicBez::new(
                point(a.point),
                a.handle_point(false),
                b.handle_point(true),
                point(b.point),
            ))
        })
    }

    pub fn insert(&mut self, index: usize, t: f64) -> Option<usize> {
        let segment = self.segment(index)?;
        let (a, b) = (segment.subsegment(0.0..t), segment.subsegment(t..1.0));
        let mut vertex = Vertex::corner(array(a.end()));
        if let (PathSeg::Cubic(a), PathSeg::Cubic(b)) = (a, b) {
            let next = (index + 1) % self.points.len();
            self.points[index].outgoing = Some(offset(a.p1 - a.p0));
            self.points[index].smooth = false;
            self.points[next].incoming = Some(offset(b.p2 - b.p3));
            self.points[next].smooth = false;
            vertex.incoming = Some(offset(a.p2 - a.p3));
            vertex.outgoing = Some(offset(b.p1 - b.p0));
        }
        self.points.insert(index + 1, vertex);
        Some(index + 1)
    }

    pub fn toggle_smooth(&mut self, index: usize) {
        let n = self.points.len();
        let Some(vertex) = self.points.get(index) else {
            return;
        };
        if vertex.smooth || vertex.incoming.is_some() || vertex.outgoing.is_some() {
            self.points[index] = Vertex::corner(vertex.point);
            return;
        }
        let prev = if index == 0 && !self.closed {
            index
        } else {
            (index + n - 1) % n
        };
        let next = if index + 1 == n && !self.closed {
            index
        } else {
            (index + 1) % n
        };
        let tangent = (point(self.points[next].point) - point(self.points[prev].point)) / 6.0;
        self.points[index] = Vertex::pen(vertex.point, Some(offset(tangent)));
    }

    pub fn bezier(&self, size: [f32; 2], all_cubics: bool) -> BezPath {
        let mut path = BezPath::new();
        if let Some(first) = self.points.first() {
            let scale = |p: Point| Point::new(p.x * f64::from(size[0]), p.y * f64::from(size[1]));
            path.move_to(scale(point(first.point)));
            for segment in (0..self.segment_count()).filter_map(|i| self.segment(i)) {
                match segment {
                    PathSeg::Line(line) if all_cubics => {
                        path.curve_to(
                            scale(line.eval(1.0 / 3.0)),
                            scale(line.eval(2.0 / 3.0)),
                            scale(line.p1),
                        );
                    }
                    PathSeg::Line(line) => path.line_to(scale(line.p1)),
                    PathSeg::Cubic(c) => path.curve_to(scale(c.p1), scale(c.p2), scale(c.p3)),
                    PathSeg::Quad(q) => path.quad_to(scale(q.p1), scale(q.p2)),
                }
            }
            if self.closed {
                path.close_path();
            }
        }
        path
    }
}

impl PathData {
    pub fn valid(&self) -> bool {
        !self.contours.is_empty()
            && self.point_count() <= MAX_POINTS
            && self.contours.iter().all(|contour| {
                contour.points.len() >= 2
                    && contour.points.iter().all(|vertex| {
                        [Some(vertex.point), vertex.incoming, vertex.outgoing]
                            .into_iter()
                            .flatten()
                            .flatten()
                            .all(|v| v.is_finite() && v.abs() <= 10_000.0)
                    })
            })
    }

    pub fn point_count(&self) -> usize {
        self.contours
            .iter()
            .map(|contour| contour.points.len())
            .sum()
    }

    pub fn from_outline(path: &Path, [w, h]: [f32; 2]) -> Self {
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return Self::default();
        }
        let local = |p: tiny_skia::Point| [p.x / w, p.y / h];
        let mut result = Self::default();
        let mut previous = Point::ORIGIN;
        for segment in path.segments() {
            if let PathSegment::MoveTo(p) = segment {
                let vertex = Vertex::corner(local(p));
                previous = point(vertex.point);
                result.contours.push(Contour {
                    points: vec![vertex],
                    closed: false,
                });
                continue;
            }
            let Some(contour) = result.contours.last_mut() else {
                continue;
            };
            let (controls, end) = match segment {
                PathSegment::LineTo(p) => (None, point(local(p))),
                PathSegment::QuadTo(a, b) => {
                    let q = kurbo::QuadBez::new(previous, point(local(a)), point(local(b))).raise();
                    (Some((q.p1, q.p2)), q.p3)
                }
                PathSegment::CubicTo(a, b, p) => {
                    (Some((point(local(a)), point(local(b)))), point(local(p)))
                }
                PathSegment::Close => {
                    contour.closed = true;
                    if contour.points.len() > 1
                        && contour.points.last().map(|p| p.point)
                            == contour.points.first().map(|p| p.point)
                        && let Some(last) = contour.points.pop()
                    {
                        contour.points[0].incoming = last.incoming;
                    }
                    continue;
                }
                PathSegment::MoveTo(_) => continue,
            };
            let mut vertex = Vertex::corner(array(end));
            if let Some((a, b)) = controls {
                if let Some(last) = contour.points.last_mut() {
                    last.outgoing = Some(offset(a - previous));
                }
                vertex.incoming = Some(offset(b - end));
            }
            contour.points.push(vertex);
            previous = end;
        }
        result
    }

    pub fn outline(&self, size: [f32; 2]) -> Option<Path> {
        let mut builder = PathBuilder::new();
        for contour in &self.contours {
            add_elements(&mut builder, contour.bezier(size, false).elements());
        }
        builder.finish()
    }

    pub fn svg(&self, size: [f32; 2], all_cubics: bool) -> String {
        self.contours
            .iter()
            .map(|c| c.bezier(size, all_cubics).to_svg())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn morph(&self, other: &Self, fraction: f32) -> Self {
        let mut a = self.clone();
        let mut b = other.clone();
        a.match_topology(&mut b);
        a.interpolate(&b, fraction)
    }

    pub fn match_topology(&mut self, other: &mut Self) {
        let count = self.contours.len().max(other.contours.len());
        for i in 0..count {
            if self.contours.get(i).is_none() {
                self.contours.push(collapsed(&other.contours[i]));
            }
            if other.contours.get(i).is_none() {
                other.contours.push(collapsed(&self.contours[i]));
            }
            let (a, b) = (&mut self.contours[i], &mut other.contours[i]);
            let mixed = a.closed != b.closed;
            let size = (a.points.len() + usize::from(mixed && a.closed))
                .max(b.points.len() + usize::from(mixed && b.closed));
            let closed = a.closed && b.closed;
            if a.points.len() != b.points.len() || a.closed != b.closed {
                *a = resample(a, size, closed);
                *b = resample(b, size, closed);
            }
        }
    }

    pub fn interpolate(&self, other: &Self, fraction: f32) -> Self {
        let mix =
            |a: [f32; 2], b: [f32; 2]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * fraction);
        let handle = |a: Option<[f32; 2]>, b: Option<[f32; 2]>| {
            (a.is_some() || b.is_some()).then(|| mix(a.unwrap_or_default(), b.unwrap_or_default()))
        };
        Self {
            contours: self
                .contours
                .iter()
                .zip(&other.contours)
                .map(|(a, b)| Contour {
                    closed: a.closed,
                    points: a
                        .points
                        .iter()
                        .zip(&b.points)
                        .map(|(a, b)| Vertex {
                            point: mix(a.point, b.point),
                            incoming: handle(a.incoming, b.incoming),
                            outgoing: handle(a.outgoing, b.outgoing),
                            smooth: a.smooth && b.smooth,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

fn collapsed(contour: &Contour) -> Contour {
    let at = contour.points.first().map_or([0.0; 2], |p| p.point);
    Contour {
        closed: contour.closed,
        points: vec![Vertex::corner(at); contour.points.len()],
    }
}

fn resample(contour: &Contour, count: usize, closed: bool) -> Contour {
    let mut result = contour.clone();
    if contour.closed && !closed {
        if let Some(first) = result.points.first().cloned() {
            result.points.push(first);
        }
        result.closed = false;
    }
    result.split_to(count);
    result
}

impl Contour {
    /// Subdivide by arc length, retaining every original corner and curve.
    pub(crate) fn split_to(&mut self, count: usize) {
        if self.points.len() >= count {
            return;
        }
        let segments: Vec<_> = (0..self.segment_count())
            .filter_map(|i| self.segment(i))
            .collect();
        if segments.is_empty() {
            return;
        }
        let lengths: Vec<_> = segments.iter().map(|s| s.arclen(0.00001)).collect();
        let mut steps = vec![1; segments.len()];
        let mut longest = std::collections::BinaryHeap::new();
        for (i, &length) in lengths.iter().enumerate() {
            longest.push((length.to_bits(), i));
        }
        for _ in self.points.len()..count {
            if let Some((_, i)) = longest.pop() {
                steps[i] += 1;
                longest.push(((lengths[i] / f64::from(steps[i])).to_bits(), i));
            }
        }
        let mut points = vec![Vertex::corner(array(segments[0].start()))];
        for ((segment, length), steps) in segments.into_iter().zip(lengths).zip(steps) {
            let mut before = 0.0;
            for i in 1..=steps {
                let fraction = f64::from(i) / f64::from(steps);
                let after = if i == steps || length == 0.0 {
                    fraction
                } else {
                    segment.inv_arclen(length * fraction, 0.00001)
                };
                let sub = segment.subsegment(before..after);
                let mut vertex = Vertex::corner(array(sub.end()));
                if let PathSeg::Cubic(c) = sub {
                    if let Some(previous) = points.last_mut() {
                        previous.outgoing = Some(offset(c.p1 - c.p0));
                    }
                    vertex.incoming = Some(offset(c.p2 - c.p3));
                }
                points.push(vertex);
                before = after;
            }
        }
        if self.closed
            && let Some(last) = points.pop()
        {
            points[0].incoming = last.incoming;
        }
        self.points = points;
    }
}

#[derive(Clone)]
pub(crate) struct MeasuredPath {
    contours: Vec<Vec<(PathSeg, f64)>>,
    closed: Vec<bool>,
    total: f64,
}

impl MeasuredPath {
    pub fn new(path: &Path) -> Self {
        let mut contours = Vec::new();
        let mut closed = Vec::new();
        let mut current = BezPath::new();
        let mut close = false;
        let point = |p: tiny_skia::Point| Point::new(f64::from(p.x), f64::from(p.y));
        let mut total = 0.0;
        let mut finish = |path: &mut BezPath, close| {
            if !path.elements().is_empty() {
                contours.push(
                    path.segments()
                        .filter_map(|s| {
                            let len = s.arclen(ACCURACY);
                            total += len;
                            (len > 0.0).then_some((s, len))
                        })
                        .collect(),
                );
                closed.push(close);
                *path = BezPath::new();
            }
        };
        for element in path.segments() {
            match element {
                PathSegment::MoveTo(p) => {
                    finish(&mut current, close);
                    close = false;
                    current.move_to(point(p));
                }
                PathSegment::LineTo(p) => current.line_to(point(p)),
                PathSegment::QuadTo(a, b) => current.quad_to(point(a), point(b)),
                PathSegment::CubicTo(a, b, c) => current.curve_to(point(a), point(b), point(c)),
                PathSegment::Close => {
                    current.close_path();
                    close = true;
                }
            }
        }
        finish(&mut current, close);
        Self {
            contours,
            closed,
            total,
        }
    }

    pub fn length(&self) -> f64 {
        self.total
    }

    /// Each piece carries its starting distance within the contour for dash alignment.
    pub fn trim(&self, start: f32, end: f32, offset: f32) -> Vec<(Path, f32)> {
        let mut pieces = Vec::new();
        let mut contour_start = 0.0;
        let ranges = trim_ranges(start, end, offset);
        for (contour, &closed) in self.contours.iter().zip(&self.closed) {
            let length: f64 = contour.iter().map(|(_, len)| len).sum();
            if contour.is_empty() {
                continue;
            }
            let mut bounds: Vec<_> = ranges
                .iter()
                .map(|&(a, b)| {
                    (
                        (a * self.total - contour_start).max(0.0),
                        (b * self.total - contour_start).min(length),
                    )
                })
                .filter(|(a, b)| a < b)
                .collect();
            let joined = closed && bounds.len() == 2 && bounds[0].1 == length && bounds[1].0 == 0.0;
            if joined {
                let mut path = BezPath::new();
                append_range(&mut path, contour, bounds[0], true);
                append_range(&mut path, contour, bounds[1], false);
                if let Some(path) = skia(&path) {
                    pieces.push((path, bounds[0].0 as f32));
                }
                bounds.clear();
            }
            for (a, b) in bounds {
                let mut path = BezPath::new();
                append_range(&mut path, contour, (a, b), true);
                if closed && a == 0.0 && b == length {
                    path.close_path();
                }
                if let Some(path) = skia(&path) {
                    pieces.push((path, a as f32));
                }
            }
            contour_start += length;
        }
        pieces
    }
}

pub(crate) fn trim_ranges(start: f32, end: f32, offset: f32) -> Vec<(f64, f64)> {
    let span = f64::from((end - start).clamp(0.0, 100.0)) / 100.0;
    if span >= 1.0 {
        return vec![(0.0, 1.0)];
    }
    if span <= 0.0 {
        return Vec::new();
    }
    let from = (f64::from(start) / 100.0 + f64::from(offset) / 360.0).rem_euclid(1.0);
    let to = from + span;
    if to > 1.0 {
        vec![(from, 1.0), (0.0, to - 1.0)]
    } else {
        vec![(from, to)]
    }
}

fn append_range(
    path: &mut BezPath,
    contour: &[(PathSeg, f64)],
    (a, b): (f64, f64),
    mut first: bool,
) {
    let mut cursor = 0.0;
    for &(segment, length) in contour {
        let low = (a - cursor).max(0.0);
        let high = (b - cursor).min(length);
        cursor += length;
        if low >= high {
            continue;
        }
        let parameter = |len: f64| {
            if len <= 0.0 {
                0.0
            } else if len >= length {
                1.0
            } else {
                segment.inv_arclen(len, ACCURACY)
            }
        };
        let piece = segment.subsegment(parameter(low)..parameter(high));
        if first {
            path.move_to(piece.start());
            first = false;
        }
        path.push(piece.as_path_el());
    }
}

fn skia(path: &BezPath) -> Option<Path> {
    let mut builder = PathBuilder::new();
    add_elements(&mut builder, path.elements());
    builder.finish()
}

fn add_elements(builder: &mut PathBuilder, elements: &[PathEl]) {
    for &element in elements {
        match element {
            PathEl::MoveTo(p) => builder.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => builder.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(a, b) => builder.quad_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32),
            PathEl::CurveTo(a, b, c) => builder.cubic_to(
                a.x as f32, a.y as f32, b.x as f32, b.y as f32, c.x as f32, c.y as f32,
            ),
            PathEl::ClosePath => builder.close(),
        }
    }
}

pub(crate) fn point([x, y]: [f32; 2]) -> Point {
    Point::new(f64::from(x), f64::from(y))
}

pub(crate) fn array(p: Point) -> [f32; 2] {
    [p.x as f32, p.y as f32]
}

fn offset(v: Vec2) -> [f32; 2] {
    [v.x as f32, v.y as f32]
}

fn vector(v: Option<[f32; 2]>) -> Vec2 {
    let [x, y] = v.unwrap_or_default();
    Vec2::new(f64::from(x), f64::from(y))
}
