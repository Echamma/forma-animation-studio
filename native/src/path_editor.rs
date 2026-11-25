use crate::paths::{Contour, MAX_POINTS, PathData, Vertex, point};
use crate::transform;
use canopy_ui::color;
use egui::{Color32, Pos2, Rect, Response, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use kurbo::{ParamCurveExtrema, ParamCurveNearest};
use std::collections::HashSet;
use tiny_skia::Transform;

const HIT: f32 = 8.0;

#[derive(Default)]
pub(crate) struct Pen {
    draft: Contour,
    drag_start: Option<Pos2>,
}

impl Pen {
    pub fn empty(&self) -> bool {
        self.draft.points.is_empty()
    }

    pub fn ready(&self, closed: bool) -> bool {
        self.draft.points.len() >= if closed { 3 } else { 2 }
    }

    pub fn clear(&mut self) {
        self.draft.points.clear();
        self.drag_start = None;
    }

    pub fn finish(&mut self, closed: bool) -> Option<Contour> {
        if !self.ready(closed) {
            return None;
        }
        let mut contour = std::mem::take(&mut self.draft);
        contour.closed = closed;
        Some(contour)
    }

    pub fn canvas(
        &mut self,
        ui: &Ui,
        painter: &egui::Painter,
        response: &Response,
        view: Transform,
    ) -> Option<Contour> {
        let inverse = view.invert().filter(Transform::is_valid)?;
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let screen = |p| map(view, p);
        let local = |p: Pos2| array_at(inverse, p);
        let mouse = response.interact_pointer_pos();
        let start = self.draft.points.first().map(|v| screen(v.point));
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.drag_start = ui.input(|i| i.pointer.press_origin()).map(local);
        }
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(at) = mouse
        {
            if self.ready(true) && start.is_some_and(|start| start.distance(at) <= HIT) {
                return self.finish(true);
            }
            self.push(local(at), None);
        }
        if response.drag_stopped_by(egui::PointerButton::Primary)
            && let (Some(at), Some(origin)) = (mouse, self.drag_start.take())
        {
            if self.ready(true)
                && start.is_some_and(|start| start.distance(screen([origin.x, origin.y])) <= HIT)
            {
                if let Some(first) = self.draft.points.first_mut() {
                    let h = local(at) - origin;
                    first.handle(false, [h.x, h.y], false);
                }
                return self.finish(true);
            }
            let h = local(at) - origin;
            self.push(origin, Some([h.x, h.y]));
        }

        let mut display = self.draft.clone();
        if response.dragged_by(egui::PointerButton::Primary)
            && let (Some(at), Some(origin)) = (mouse, self.drag_start)
        {
            let h = local(at) - origin;
            display
                .points
                .push(Vertex::pen([origin.x, origin.y], Some([h.x, h.y])));
        }
        let path = PathData {
            contours: vec![display],
        };
        draw_path(painter, &path, view, color::LEAF);
        for (i, vertex) in path.contours[0].points.iter().enumerate() {
            draw_vertex(painter, vertex, view, i == 0, color::LEAF);
        }
        if let (Some(last), Some(at)) = (self.draft.points.last(), response.hover_pos())
            && !response.dragged()
        {
            painter.line_segment([screen(last.point), at], Stroke::new(1.0, color::LEAF));
        }
        None
    }

    fn push(&mut self, at: Pos2, handle: Option<[f32; 2]>) {
        if at.is_finite()
            && self.draft.points.len() < MAX_POINTS
            && self
                .draft
                .points
                .last()
                .is_none_or(|last| last.point != [at.x, at.y])
        {
            self.draft.points.push(Vertex::pen([at.x, at.y], handle));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Node(usize, usize);

#[derive(Clone, Copy)]
struct Handle(Node, bool);

enum Drag {
    Nodes {
        snapshot: PathData,
        start: Pos2,
    },
    Handle {
        snapshot: PathData,
        target: Handle,
        start: Pos2,
    },
    Box {
        origin: Pos2,
        extend: HashSet<Node>,
    },
}

#[derive(Default)]
pub(crate) struct PointEditor {
    selected: HashSet<Node>,
    drag: Option<Drag>,
    last_output: Option<PathData>,
}

impl PointEditor {
    pub fn cancel(&mut self) {
        *self = Self::default();
    }

    pub fn delete(&mut self, path: &mut PathData, minimum: usize) -> bool {
        let before = path.point_count();
        let mut next = path.clone();
        for (c, contour) in next.contours.iter_mut().enumerate() {
            let mut i = 0;
            contour.points.retain(|_| {
                let keep = !self.selected.contains(&Node(c, i));
                i += 1;
                keep
            });
        }
        next.contours.retain(|c| c.points.len() >= minimum);
        if next.valid() && next.point_count() != before {
            *path = next;
            self.cancel();
            return true;
        }
        false
    }

    pub fn canvas(
        &mut self,
        ui: &Ui,
        painter: &egui::Painter,
        response: &Response,
        path: &mut PathData,
        view: Transform,
        ink: Color32,
    ) -> bool {
        let Some(inverse) = view.invert().filter(Transform::is_valid) else {
            return false;
        };
        if self.last_output.as_ref().is_some_and(|last| last != path) {
            self.cancel();
        }
        self.selected.retain(|node| get(path, *node).is_some());
        let primary = egui::PointerButton::Primary;
        let extend = ui.input(|i| i.modifiers.shift);
        let mut changed = false;
        if response.drag_started_by(primary)
            && let Some(start) = ui.input(|i| i.pointer.press_origin())
        {
            self.drag = Some(if let Some(target) = self.hit_handle(path, view, start) {
                Drag::Handle {
                    snapshot: path.clone(),
                    target,
                    start,
                }
            } else if let Some(node) = hit_node(path, view, start) {
                if !self.selected.contains(&node) {
                    if !extend {
                        self.selected.clear();
                    }
                    self.selected.insert(node);
                }
                Drag::Nodes {
                    snapshot: path.clone(),
                    start,
                }
            } else {
                let extend = if extend {
                    self.selected.clone()
                } else {
                    HashSet::new()
                };
                Drag::Box {
                    origin: start,
                    extend,
                }
            });
        }
        if let Some(at) = response.interact_pointer_pos()
            && (response.dragged_by(primary) || response.drag_stopped_by(primary))
        {
            match &self.drag {
                Some(Drag::Nodes { snapshot, start }) => {
                    let delta = array_at(inverse, at) - array_at(inverse, *start);
                    let mut next = snapshot.clone();
                    for &node in &self.selected {
                        if let Some(v) = get_mut(&mut next, node) {
                            v.point = [v.point[0] + delta.x, v.point[1] + delta.y];
                        }
                    }
                    changed = replace(path, next);
                }
                Some(Drag::Handle {
                    snapshot,
                    target: Handle(node, incoming),
                    start,
                }) => {
                    let delta = array_at(inverse, at) - array_at(inverse, *start);
                    let mut next = snapshot.clone();
                    if let Some(v) = get_mut(&mut next, *node) {
                        let origin =
                            if *incoming { v.incoming } else { v.outgoing }.unwrap_or_default();
                        let split = ui.input(|i| i.modifiers.alt)
                            || get(path, *node).is_some_and(|live| !live.smooth);
                        v.handle(*incoming, [origin[0] + delta.x, origin[1] + delta.y], split);
                    }
                    changed = replace(path, next);
                }
                Some(Drag::Box { origin, extend }) => {
                    let box_rect = Rect::from_two_pos(*origin, at);
                    self.selected = extend.clone();
                    self.selected
                        .extend(nodes(path).filter_map(|(node, vertex)| {
                            box_rect.contains(map(view, vertex.point)).then_some(node)
                        }));
                    painter.rect_stroke(box_rect, 0.0, Stroke::new(1.0, ink), StrokeKind::Inside);
                }
                None => {}
            }
        }
        if response.drag_stopped_by(primary) {
            self.drag = None;
        }
        if (response.clicked_by(primary) || response.double_clicked_by(primary))
            && let Some(at) = response.interact_pointer_pos()
        {
            if let Some(Node(c, i)) = hit_node(path, view, at) {
                let node = Node(c, i);
                if !extend {
                    self.selected.clear();
                }
                if !self.selected.insert(node) && extend {
                    self.selected.remove(&node);
                }
                if response.double_clicked_by(primary) {
                    path.contours[c].toggle_smooth(i);
                    changed = true;
                }
            } else if path.point_count() < MAX_POINTS
                && let Some((contour, segment, t)) = nearest_segment(path, view, at)
                && let Some(i) = path.contours[contour].insert(segment, t)
            {
                self.selected.clear();
                self.selected.insert(Node(contour, i));
                changed = true;
            } else if !extend {
                self.selected.clear();
            }
        }
        if response.hovered()
            && let Some(at) = response.hover_pos()
        {
            if self.hit_handle(path, view, at).is_some() || hit_node(path, view, at).is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
        }
        draw_path(painter, path, view, ink);
        for (node, vertex) in nodes(path) {
            draw_vertex(painter, vertex, view, self.selected.contains(&node), ink);
        }
        self.last_output = Some(path.clone());
        changed
    }

    fn hit_handle(&self, path: &PathData, view: Transform, at: Pos2) -> Option<Handle> {
        nodes(path)
            .filter(|(node, _)| self.selected.contains(node))
            .flat_map(|(node, v)| {
                [(true, v.incoming), (false, v.outgoing)]
                    .into_iter()
                    .filter_map(move |(incoming, h)| {
                        let h = h?;
                        let end = map(view, [v.point[0] + h[0], v.point[1] + h[1]]);
                        (end.distance(map(view, v.point)) > HIT / 2.0)
                            .then_some((Handle(node, incoming), end.distance(at)))
                    })
            })
            .filter(|(_, distance)| *distance <= HIT)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(handle, _)| handle)
    }
}

fn replace(path: &mut PathData, next: PathData) -> bool {
    if next == *path || !next.valid() {
        return false;
    }
    *path = next;
    true
}

fn nodes(path: &PathData) -> impl Iterator<Item = (Node, &Vertex)> {
    path.contours.iter().enumerate().flat_map(|(c, contour)| {
        contour
            .points
            .iter()
            .enumerate()
            .map(move |(i, v)| (Node(c, i), v))
    })
}

fn get(path: &PathData, Node(c, i): Node) -> Option<&Vertex> {
    path.contours.get(c)?.points.get(i)
}

fn get_mut(path: &mut PathData, Node(c, i): Node) -> Option<&mut Vertex> {
    path.contours.get_mut(c)?.points.get_mut(i)
}

fn hit_node(path: &PathData, view: Transform, at: Pos2) -> Option<Node> {
    nodes(path)
        .map(|(node, v)| (node, map(view, v.point).distance(at)))
        .filter(|(_, d)| *d <= HIT)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(node, _)| node)
}

pub(crate) fn nearest_segment(
    path: &PathData,
    view: Transform,
    at: Pos2,
) -> Option<(usize, usize, f64)> {
    let affine =
        kurbo::Affine::new([view.sx, view.ky, view.kx, view.sy, view.tx, view.ty].map(f64::from));
    let mouse = point([at.x, at.y]);
    path.contours
        .iter()
        .enumerate()
        .flat_map(|(c, contour)| {
            (0..contour.segment_count()).filter_map(move |i| {
                let segment = affine * contour.segment(i)?;
                let nearest = segment.nearest(mouse, 0.2);
                (nearest.distance_sq <= f64::from(HIT * HIT) && (0.02..0.98).contains(&nearest.t))
                    .then_some((c, i, nearest.t, nearest.distance_sq))
            })
        })
        .min_by(|a, b| a.3.total_cmp(&b.3))
        .map(|(c, i, t, _)| (c, i, t))
}

pub(crate) fn hits(path: &PathData, view: Transform, at: Pos2, fill: bool, stroke: f32) -> bool {
    use kurbo::Shape as _;
    let affine =
        kurbo::Affine::new([view.sx, view.ky, view.kx, view.sy, view.tx, view.ty].map(f64::from));
    let mouse = point([at.x, at.y]);
    let mut winding = 0;
    for contour in &path.contours {
        let mut outline = affine * contour.bezier([1.0; 2], false);
        if outline.segments().any(|segment| {
            segment.nearest(mouse, 0.4).distance_sq <= f64::from(stroke.max(HIT).powi(2))
        }) {
            return true;
        }
        if fill {
            if !contour.closed {
                outline.close_path();
            }
            winding += outline.winding(mouse);
        }
    }
    winding != 0
}

pub(crate) fn shape_bounds(contour: &Contour) -> Option<Rect> {
    let bounds = contour
        .bezier([1.0; 2], false)
        .segments()
        .map(|segment| segment.bounding_box())
        .reduce(|a, b| a.union(b))?;
    let bounds = Rect::from_min_max(
        pos2(bounds.x0 as f32, bounds.y0 as f32),
        pos2(bounds.x1 as f32, bounds.y1 as f32),
    );
    bounds.is_finite().then_some(Rect::from_center_size(
        bounds.center(),
        bounds.size().max(Vec2::splat(1.0)),
    ))
}

pub(crate) fn normalize(contour: Contour, bounds: Rect) -> Contour {
    let size = bounds.size();
    let center = bounds.center();
    let scale = |[x, y]: [f32; 2]| [x / size.x, y / size.y];
    Contour {
        closed: contour.closed,
        points: contour
            .points
            .into_iter()
            .map(|v| Vertex {
                point: scale([v.point[0] - center.x, v.point[1] - center.y]),
                incoming: v.incoming.map(scale),
                outgoing: v.outgoing.map(scale),
                smooth: v.smooth,
            })
            .collect(),
    }
}

pub(crate) fn map(transform: Transform, [x, y]: [f32; 2]) -> Pos2 {
    let (x, y) = transform::point(transform, x, y);
    pos2(x, y)
}

fn array_at(transform: Transform, at: Pos2) -> Pos2 {
    map(transform, [at.x, at.y])
}

fn draw_path(painter: &egui::Painter, path: &PathData, view: Transform, color: Color32) {
    let affine =
        kurbo::Affine::new([view.sx, view.ky, view.kx, view.sy, view.tx, view.ty].map(f64::from));
    for contour in &path.contours {
        let path = affine * contour.bezier([1.0; 2], false);
        let mut points = Vec::new();
        kurbo::flatten(path, 0.35, |el| match el {
            kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => {
                points.push(pos2(p.x as f32, p.y as f32))
            }
            _ => {}
        });
        if points.len() > 1 {
            painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
        }
    }
}

fn draw_vertex(painter: &egui::Painter, v: &Vertex, view: Transform, selected: bool, ink: Color32) {
    let at = map(view, v.point);
    if selected {
        for h in [v.incoming, v.outgoing].into_iter().flatten() {
            let end = map(view, [v.point[0] + h[0], v.point[1] + h[1]]);
            painter.line_segment([at, end], Stroke::new(1.0, ink));
            painter.circle(end, 3.5, Color32::WHITE, Stroke::new(1.0, ink));
        }
    }
    let fill = if selected { ink } else { Color32::WHITE };
    let border = Stroke::new(1.4, if selected { Color32::WHITE } else { ink });
    if v.smooth {
        painter.circle(at, 4.5, fill, border);
    } else {
        painter.rect(
            Rect::from_center_size(at, vec2(8.0, 8.0)),
            0.0,
            fill,
            border,
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Modifiers, PointerButton, RawInput, Sense};

    struct Canvas {
        ctx: Context,
        bounds: Rect,
    }

    impl Canvas {
        fn new() -> Self {
            Self {
                ctx: Context::default(),
                bounds: Rect::from_min_size(Pos2::ZERO, vec2(200.0, 150.0)),
            }
        }

        fn frame(&self, events: Vec<Event>, mut draw: impl FnMut(&Ui, &Response)) {
            self.ctx
                .run_ui(
                    RawInput {
                        screen_rect: Some(self.bounds),
                        events,
                        ..RawInput::default()
                    },
                    |ui| {
                        let response = ui.interact(
                            self.bounds,
                            ui.scope_id().with("path-test"),
                            Sense::click_and_drag(),
                        );
                        draw(ui, &response);
                    },
                )
                .textures_delta
                .clear();
        }

        fn click(&self, at: Pos2, mut draw: impl FnMut(&Ui, &Response)) {
            self.frame(vec![Event::PointerMoved(at), button(at, true)], &mut draw);
            self.frame(vec![button(at, false)], draw);
        }

        fn drag(&self, start: Pos2, end: Pos2, mut draw: impl FnMut(&Ui, &Response)) {
            self.frame(
                vec![Event::PointerMoved(start), button(start, true)],
                &mut draw,
            );
            self.frame(vec![Event::PointerMoved(end)], &mut draw);
            self.frame(vec![button(end, false)], draw);
        }
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            pressed,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn pen_click_drag_and_close_builds_corners_and_mirrored_handles() {
        let mut pen = Pen::default();
        let mut result = None;
        let canvas = Canvas::new();
        let mut draw = |ui: &Ui, response: &Response| {
            if let Some(path) = pen.canvas(ui, ui.painter(), response, Transform::identity()) {
                result = Some(path);
            }
        };
        canvas.frame(vec![], &mut draw);
        canvas.click(pos2(30.0, 40.0), &mut draw);
        canvas.drag(pos2(90.0, 40.0), pos2(104.0, 63.0), &mut draw);
        canvas.click(pos2(60.0, 110.0), &mut draw);
        canvas.click(pos2(30.0, 40.0), draw);
        let contour = result.expect("clicking the first vertex closes the contour");
        assert!(contour.closed);
        assert_eq!(contour.points.len(), 3);
        assert_eq!(contour.points[0], Vertex::corner([30.0, 40.0]));
        assert_eq!(
            contour.points[1],
            Vertex::pen([90.0, 40.0], Some([14.0, 23.0]))
        );
        let kurbo::PathSeg::Cubic(segment) = contour.segment(0).unwrap() else {
            panic!("expected a cubic segment")
        };
        assert_eq!(segment.p2, kurbo::Point::new(76.0, 17.0));
    }

    #[test]
    fn segment_insertion_box_selection_and_deletion_use_the_live_path() {
        let mut path = PathData {
            contours: vec![Contour {
                points: [[30.0, 30.0], [110.0, 30.0], [110.0, 100.0], [30.0, 100.0]]
                    .map(Vertex::corner)
                    .to_vec(),
                closed: true,
            }],
        };
        let mut editor = PointEditor::default();
        let canvas = Canvas::new();
        let mut draw = |ui: &Ui, response: &Response| {
            editor.canvas(
                ui,
                ui.painter(),
                response,
                &mut path,
                Transform::identity(),
                color::SUN,
            );
        };
        canvas.frame(vec![], &mut draw);
        canvas.click(pos2(70.0, 30.0), &mut draw);
        canvas.drag(pos2(60.0, 15.0), pos2(85.0, 40.0), draw);
        assert_eq!(path.contours[0].points[1].point, [70.0, 30.0]);
        assert!(editor.delete(&mut path, 2));
        assert_eq!(path.point_count(), 4);
        editor.selected.extend([Node(0, 1), Node(0, 2)]);
        assert!(editor.delete(&mut path, 2));
        assert_eq!(path.point_count(), 2);
    }

    #[test]
    fn breaking_a_handle_preserves_its_opposite() {
        let mut vertex = Vertex::pen([0.0, 0.0], Some([10.0, 0.0]));
        vertex.handle(false, [5.0, 7.0], true);
        assert!(!vertex.smooth);
        assert_eq!(vertex.incoming, Some([-10.0, 0.0]));
        assert_eq!(vertex.outgoing, Some([5.0, 7.0]));
    }
}
