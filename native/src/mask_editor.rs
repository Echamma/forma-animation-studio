use crate::compositing::{
    Blend, LayerMask, MaskMode, MaskPoint, MaskPose, MaskProperty, MaskShape, MatteMode, TrackMatte,
};
use crate::model::{Scene, Shape};
use crate::path_editor::{self, Pen, PointEditor};
use crate::paths::{Contour, PathData, Vertex};
use crate::transform::{LayerFrame, point};
use canopy_ui::color;
use egui::{Color32, Pos2, Rect, Response, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use tiny_skia::{Path, PathSegment, Point, Transform};

const CORNERS: [Vec2; 4] = [
    vec2(-1.0, -1.0),
    vec2(1.0, -1.0),
    vec2(1.0, 1.0),
    vec2(-1.0, 1.0),
];
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Edit,
    Rectangle,
    Ellipse,
    Path,
    Points,
}

impl Tool {
    fn label(self) -> &'static str {
        match self {
            Self::Edit => "Edit masks",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Path => "Freeform path",
            Self::Points => "Edit mask points",
        }
    }
}

#[derive(Default)]
pub(crate) struct MaskEditor {
    layer: Option<String>,
    selected: Option<u64>,
    tool: Option<Tool>,
    time: Option<f64>,
    path: Pen,
    points: PointEditor,
    drawing: Option<Pos2>,
    dragging: Option<MaskDrag>,
    open_panel: bool,
    pub reveal_lanes: Option<(String, u64)>,
}

struct MaskDrag {
    id: u64,
    time: f64,
    start: Pos2,
    pose: MaskPose,
    inverse: Transform,
    corner: Option<Vec2>,
}

struct Outline {
    id: u64,
    points: Vec<Pos2>,
    corners: [Pos2; 4],
    space: Transform,
    pose: MaskPose,
}

impl MaskEditor {
    pub fn sync(&mut self, layer: Option<&str>) {
        if self.layer.as_deref() != layer {
            *self = Self {
                layer: layer.map(str::to_owned),
                ..Self::default()
            };
        }
    }

    pub fn active(&self) -> bool {
        self.tool.is_some()
    }

    pub fn stop(&mut self) {
        self.tool = None;
        self.cancel_gesture();
    }

    pub fn cancel_gesture(&mut self) {
        self.path.clear();
        self.points.cancel();
        self.drawing = None;
        self.dragging = None;
    }

    pub fn escape(&mut self) {
        if self.path.empty() {
            self.stop();
        } else {
            self.cancel_gesture();
        }
    }

    pub fn toggle(&mut self, scene: &Scene) {
        if self.active() {
            self.stop();
        } else if let Some(shape) = self
            .layer
            .as_deref()
            .and_then(|id| scene.shapes.iter().find(|s| s.id == id))
        {
            self.tool = Some(if shape.masks.is_empty() {
                Tool::Rectangle
            } else {
                Tool::Edit
            });
            self.open_panel = true;
        }
    }

    pub fn select(&mut self, layer: &str, mask: u64) {
        self.sync(Some(layer));
        self.selected = Some(mask);
    }

    pub fn panel(
        &mut self,
        ui: &mut Ui,
        scene: &mut Scene,
        index: usize,
        time: f64,
        playing: &mut bool,
    ) {
        let id = scene.shapes[index].id.clone();
        self.sync(Some(&id));
        let open = std::mem::take(&mut self.open_panel).then_some(true);
        let section = egui::CollapsingHeader::new("Blend & mask")
            .open(open)
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 32.0;
                let mut blend = scene.shapes[index].blend();
                ui.label("Blend mode");
                if choose(ui, "Blend mode", &mut blend, Blend::ALL, Blend::label) {
                    scene.shapes[index].blend_mode = (blend != Blend::Normal).then_some(blend);
                }
                matte_panel(ui, scene, index);
                ui.separator();
                self.tool_picker(ui, playing);
                if self.tool == Some(Tool::Path)
                    && self.path.ready(true)
                    && ui.button("Finish path").clicked()
                {
                    let frame = scene.layer_frame(&scene.shapes[index], time);
                    if let Some(contour) = self.path.finish(true) {
                        self.finish_path(&mut scene.shapes[index], frame, contour);
                    }
                }
                self.mask_fields(ui, &mut scene.shapes[index], time);
            });
        if open.is_some() {
            section.header_response.scroll_to_me(Some(egui::Align::TOP));
        }
    }

    fn tool_picker(&mut self, ui: &mut Ui, playing: &mut bool) {
        ui.label("Mask tool");
        let mut tool = self.tool;
        if choose(
            ui,
            "mask-tool",
            &mut tool,
            &[
                None,
                Some(Tool::Rectangle),
                Some(Tool::Ellipse),
                Some(Tool::Path),
                Some(Tool::Edit),
                Some(Tool::Points),
            ],
            |tool| tool.map_or("Off", Tool::label),
        ) {
            self.cancel_gesture();
            self.tool = tool;
            *playing = false;
        }
        let hint = match self.tool {
            Some(Tool::Rectangle | Tool::Ellipse) => {
                "Drag on the canvas over the area you want to keep."
            }
            Some(Tool::Path) => {
                "Click for a corner, drag for curves. Click the first point or press Enter to close."
            }
            Some(Tool::Edit) => "Drag a mask to move it, or drag a corner to resize it.",
            Some(Tool::Points) => "Drag mask points or handles. Click a segment to add a point.",
            None => return,
        };
        ui.label(egui::RichText::new(hint).color(color::INK_MUTED));
    }

    fn mask_fields(&mut self, ui: &mut Ui, shape: &mut Shape, time: f64) {
        let Some(id) = self.mask_selection(shape) else {
            return;
        };
        self.selected = Some(id);
        ui.separator();
        let mut selected = id;
        let label = |id| {
            let number = shape
                .masks
                .iter()
                .position(|mask| mask.id == id)
                .map_or(1, |i| i + 1);
            format!("Mask {number}")
        };
        let ids: Vec<_> = shape.masks.iter().map(|mask| mask.id).collect();
        if choose(ui, "selected-mask", &mut selected, &ids, label) {
            self.selected = Some(selected);
            self.points.cancel();
        }
        let ease = shape.ease.clone();
        let Some(mask) = shape.mask_mut(selected) else {
            return;
        };
        let mut mode = mask.mode;
        ui.label("Mask mode");
        choose(ui, "Mask mode", &mut mode, MaskMode::ALL, MaskMode::label);
        mask.mode = mode;
        ui.checkbox(&mut mask.inverted, "Invert mask");
        let pose = mask.pose(time, &ease);
        for (property, name, suffix) in [
            (MaskProperty::Feather, "Feather", " px"),
            (MaskProperty::Expansion, "Expansion", " px"),
            (MaskProperty::Opacity, "Opacity", "%"),
        ] {
            mask_number(ui, shape, selected, time, pose, property, 0, name, suffix);
        }
        egui::CollapsingHeader::new("Mask position & size").show(ui, |ui| {
            for (property, channel, name, suffix) in [
                (MaskProperty::Position, 0, "X", " px"),
                (MaskProperty::Position, 1, "Y", " px"),
                (MaskProperty::Size, 0, "Width", " px"),
                (MaskProperty::Size, 1, "Height", " px"),
                (MaskProperty::Rotation, 0, "Rotation", "°"),
            ] {
                mask_number(
                    ui, shape, selected, time, pose, property, channel, name, suffix,
                );
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Show mask lanes").clicked() {
                self.reveal_lanes = Some((shape.id.clone(), selected));
            }
            if ui.button("Remove mask").clicked() {
                shape.masks.retain(|mask| mask.id != selected);
                self.selected = None;
                self.cancel_gesture();
            }
        });
    }

    fn mask_selection(&self, shape: &Shape) -> Option<u64> {
        self.selected
            .filter(|id| shape.mask(*id).is_some())
            .or_else(|| shape.masks.last().map(|mask| mask.id))
    }

    pub fn canvas(
        &mut self,
        ui: &Ui,
        painter: &egui::Painter,
        response: &Response,
        shape: &mut Shape,
        frame: LayerFrame,
        view: Transform,
        time: f64,
        keyboard: bool,
    ) {
        self.sync(Some(&shape.id));
        if self.time != Some(time) {
            self.cancel_gesture();
            self.time = Some(time);
        }
        let layer = frame.transform.post_concat(view);
        let Some(inverse) = layer.invert().filter(Transform::is_valid) else {
            self.cancel_gesture();
            return;
        };
        let selected = self.mask_selection(shape);
        let outlines: Vec<_> = shape
            .masks
            .iter()
            .filter_map(|mask| {
                let pose = mask.pose(time, &shape.ease);
                let space = mask.space(frame).post_concat(view);
                let transform = pose.placement().post_concat(space);
                if !transform.is_valid() {
                    return None;
                }
                let [w, h] = pose.size;
                let points = screen_outline(&mask.shape.outline(pose.size)?, transform);
                let corners = CORNERS.map(|p| map(transform, pos2(p.x * w / 2.0, p.y * h / 2.0)));
                if !points.iter().chain(&corners).all(|p| p.is_finite()) {
                    return None;
                }
                Some(Outline {
                    id: mask.id,
                    points,
                    corners,
                    space,
                    pose,
                })
            })
            .collect();
        for outline in &outlines {
            let active = selected == Some(outline.id);
            let color = if active {
                color::LEAF
            } else {
                color::LEAF.gamma_multiply(0.6)
            };
            if outline.points.len() >= 3 {
                painter.add(egui::Shape::closed_line(
                    outline.points.clone(),
                    Stroke::new(1.5, color),
                ));
            }
            if active && self.tool == Some(Tool::Edit) {
                for at in outline.corners {
                    painter.rect(
                        Rect::from_center_size(at, Vec2::splat(8.0)),
                        1.0,
                        Color32::WHITE,
                        Stroke::new(1.5, color),
                        StrokeKind::Inside,
                    );
                }
            }
        }
        match self.tool {
            Some(Tool::Edit) => self.edit_on_canvas(ui, response, shape, &outlines, selected, time),
            Some(Tool::Points) => {
                if let Some(id) = selected {
                    self.edit_points(ui, painter, response, shape, id, frame, view, time, keyboard);
                }
            }
            Some(tool) => {
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                match tool {
                    Tool::Rectangle | Tool::Ellipse => {
                        self.draw_box(ui, painter, response, shape, frame, layer, inverse, tool)
                    }
                    Tool::Path => {
                        let enter = keyboard && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && (response.has_focus() || !ui.ctx().egui_wants_keyboard_input());
                        let contour = if enter { self.path.finish(true) } else { self.path.canvas(ui, painter, response, layer) };
                        if let Some(contour) = contour {
                            self.finish_path(shape, frame, contour);
                        }
                    }
                    Tool::Edit | Tool::Points => {}
                }
            }
            None => {}
        }
    }

    fn edit_on_canvas(
        &mut self,
        ui: &Ui,
        response: &Response,
        shape: &mut Shape,
        outlines: &[Outline],
        selected: Option<u64>,
        time: f64,
    ) {
        let hit = |p| {
            outlines
                .iter()
                .find(|outline| Some(outline.id) == selected)
                .and_then(|outline| {
                    outline
                        .corners
                        .iter()
                        .position(|corner| corner.distance(p) <= 9.0)
                        .map(|i| (outline, Some(CORNERS[i])))
                })
                .or_else(|| {
                    outlines
                        .iter()
                        .rev()
                        .find(|outline| contains(&outline.points, p))
                        .map(|outline| (outline, None))
                })
        };
        if response.hovered()
            && let Some((_, corner)) = response.hover_pos().and_then(hit)
        {
            let cursor = match corner {
                Some(corner) if corner.x == corner.y => egui::CursorIcon::ResizeNwSe,
                Some(_) => egui::CursorIcon::ResizeNeSw,
                None => egui::CursorIcon::Grab,
            };
            ui.ctx().set_cursor_icon(cursor);
        }
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some((outline, _)) = response.interact_pointer_pos().and_then(hit)
        {
            self.selected = Some(outline.id);
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
            && let Some((outline, corner)) = hit(p)
        {
            let space = if corner.is_some() {
                outline.pose.placement().post_concat(outline.space)
            } else {
                outline.space
            };
            if let Some(inverse) = space.invert().filter(Transform::is_valid) {
                self.selected = Some(outline.id);
                self.dragging = Some(MaskDrag {
                    id: outline.id,
                    time,
                    start: p,
                    pose: outline.pose,
                    inverse,
                    corner,
                });
            }
        }
        let done = response.drag_stopped_by(egui::PointerButton::Primary);
        if (response.dragged_by(egui::PointerButton::Primary) || done)
            && let (Some(drag), Some(p)) = (&self.dragging, response.interact_pointer_pos())
            && drag.time == time
        {
            drag.apply(shape, p);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if done || !ui.input(|i| i.pointer.primary_down()) {
            self.dragging = None;
        }
    }

    fn draw_box(
        &mut self,
        ui: &Ui,
        painter: &egui::Painter,
        response: &Response,
        shape: &mut Shape,
        frame: LayerFrame,
        layer: Transform,
        inverse: Transform,
        tool: Tool,
    ) {
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.drawing = ui
                .input(|i| i.pointer.press_origin())
                .map(|p| map(inverse, p));
        }
        let done = response.drag_stopped_by(egui::PointerButton::Primary);
        if let (Some(start), Some(end)) = (self.drawing, response.interact_pointer_pos()) {
            let bounds = Rect::from_two_pos(start, map(inverse, end));
            let geometry = if tool == Tool::Ellipse {
                MaskShape::Ellipse
            } else {
                MaskShape::Rectangle
            };
            let size = [bounds.width(), bounds.height()];
            let center = bounds.center();
            if !done {
                if let Some(path) = geometry.outline(size) {
                    let at = Transform::from_translate(center.x, center.y).post_concat(layer);
                    painter.add(egui::Shape::closed_line(
                        screen_outline(&path, at),
                        Stroke::new(1.5, color::LEAF),
                    ));
                }
            } else if map(layer, start).distance(end) >= 3.0 && valid_bounds(bounds) {
                self.add(shape, frame, geometry, bounds);
            }
        }
        if done || !ui.input(|i| i.pointer.primary_down()) {
            self.drawing = None;
        }
    }

    fn edit_points(
        &mut self, ui: &Ui, painter: &egui::Painter, response: &Response,
        shape: &mut Shape, id: u64, frame: LayerFrame, view: Transform, time: f64, keyboard: bool,
    ) {
        let ease = shape.ease.clone();
        let Some(mask) = shape.mask_mut(id) else { return; };
        let pose = mask.pose(time, &ease);
        let space = Transform::from_scale(pose.size[0], pose.size[1])
            .post_concat(pose.placement()).post_concat(mask.space(frame)).post_concat(view);
        let mut path = match &mask.shape {
            MaskShape::Path { points } => PathData {
                contours: vec![Contour {
                    closed: true,
                    points: points.iter().map(|p| Vertex {
                        point: p.point,
                        incoming: p.incoming,
                        outgoing: p.outgoing,
                        smooth: p.outgoing.is_some_and(|h| p.incoming == Some(h.map(|v| -v))),
                    }).collect(),
                }],
            },
            geometry => {
                let Some(outline) = geometry.outline(pose.size) else { return; };
                PathData::from_outline(&outline, pose.size)
            }
        };
        let delete = keyboard
            && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        let changed = if delete {
            self.points.delete(&mut path, 3)
        } else {
            self.points.canvas(ui, painter, response, &mut path, space, color::LEAF)
        };
        if changed && let Some(contour) = path.contours.into_iter().next() {
            mask.shape = mask_path(contour);
        }
    }

    pub fn editing_points(&self) -> bool {
        self.tool == Some(Tool::Points)
    }

    fn finish_path(&mut self, shape: &mut Shape, frame: LayerFrame, contour: Contour) {
        let Some(bounds) = path_editor::shape_bounds(&contour).filter(|&bounds| valid_bounds(bounds)) else { return; };
        self.add(shape, frame, mask_path(path_editor::normalize(contour, bounds)), bounds);
    }

    fn add(&mut self, shape: &mut Shape, frame: LayerFrame, geometry: MaskShape, bounds: Rect) {
        let reference_size = [frame.pose.w, frame.pose.h];
        if !reference_size.iter().all(|v| v.is_finite() && *v >= 0.0001) {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        let id = shape
            .masks
            .iter()
            .map(|mask| mask.id)
            .max()
            .and_then(|id| id.checked_add(1))
            .unwrap_or_default()
            .max(now);
        let center = bounds.center();
        shape.masks.push(LayerMask {
            id,
            shape: geometry,
            mode: MaskMode::Add,
            inverted: false,
            reference_size,
            rest: MaskPose::new([center.x, center.y], [bounds.width(), bounds.height()]),
            keys: Vec::new(),
        });
        self.selected = Some(id);
        self.tool = Some(Tool::Edit);
        self.cancel_gesture();
        self.reveal_lanes = Some((shape.id.clone(), id));
    }
}

fn mask_path(contour: Contour) -> MaskShape {
    MaskShape::Path {
        points: contour.points.into_iter().map(|v| MaskPoint {
            point: v.point, incoming: v.incoming, outgoing: v.outgoing,
        }).collect(),
    }
}

impl MaskDrag {
    fn apply(&self, shape: &mut Shape, at: Pos2) {
        let delta = map(self.inverse, at) - map(self.inverse, self.start);
        if !delta.is_finite() {
            return;
        }
        let mut position = self.pose.position;
        if let Some(corner) = self.corner {
            let [w, h] = self.pose.size;
            let size = [w + delta.x * corner.x, h + delta.y * corner.y]
                .map(|v| MaskProperty::Size.clamp(v));
            let center = pos2(
                (size[0] - w) * corner.x / 2.0,
                (size[1] - h) * corner.y / 2.0,
            );
            let at = map(self.pose.placement(), center);
            position = [at.x, at.y];
            shape.edit_mask(self.id, MaskProperty::Size, self.time, size);
        } else {
            position[0] += delta.x;
            position[1] += delta.y;
        }
        shape.edit_mask(self.id, MaskProperty::Position, self.time, position);
    }
}

fn matte_panel(ui: &mut Ui, scene: &mut Scene, index: usize) {
    let layer = &scene.shapes[index];
    let id = layer.id.clone();
    let mut matte = layer.track_matte.clone();
    let mut source = matte.as_ref().map(|m| m.source.as_str());
    ui.label("Matte");
    let name = source
        .and_then(|id| scene.shapes.iter().find(|s| s.id == id))
        .map_or("None", |s| s.name.as_str());
    let previous = source;
    egui::ComboBox::from_id_salt("matte-source")
        .width(ui.available_width())
        .selected_text(name)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut source, None, "None");
            for shape in scene.shapes.iter().rev() {
                if scene.can_use_matte(&id, &shape.id) {
                    ui.selectable_value(&mut source, Some(shape.id.as_str()), &shape.name);
                }
            }
        });
    let above = scene
        .shapes
        .get(index + 1)
        .filter(|s| scene.can_use_matte(&id, &s.id));
    if matte.is_none()
        && let Some(above) = above
        && ui
            .button("Use layer above")
            .on_hover_text(&above.name)
            .clicked()
    {
        source = Some(&above.id);
    }
    if source != previous {
        matte = source.map(|source| TrackMatte {
            source: source.to_owned(),
            mode: matte.as_ref().map_or(MatteMode::Alpha, |m| m.mode),
            show_layer: false,
        });
    }
    if let Some(matte) = &mut matte {
        ui.label("Matte mode");
        choose(
            ui,
            "Matte mode",
            &mut matte.mode,
            MatteMode::ALL,
            MatteMode::label,
        );
        ui.checkbox(&mut matte.show_layer, "Show matte layer")
            .on_hover_text("Show the source in the picture as well as using it as a matte.");
    }
    if scene.shapes[index].track_matte != matte {
        scene.set_matte(&id, matte);
    }
}

fn choose<T: Copy + PartialEq, S: Into<egui::WidgetText>>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    choices: &[T],
    label: impl Fn(T) -> S,
) -> bool {
    let old = *value;
    egui::ComboBox::from_id_salt(id)
        .width(ui.available_width())
        .selected_text(label(*value))
        .show_ui(ui, |ui| {
            for &choice in choices {
                ui.selectable_value(value, choice, label(choice));
            }
        });
    *value != old
}

fn mask_number(
    ui: &mut Ui,
    shape: &mut Shape,
    id: u64,
    time: f64,
    pose: MaskPose,
    property: MaskProperty,
    channel: usize,
    name: &str,
    suffix: &str,
) {
    let mut values = pose.values(property);
    ui.horizontal(|ui| {
        ui.label(name);
        if ui
            .add(
                egui::DragValue::new(&mut values[channel])
                    .range(property.limits())
                    .speed(0.5)
                    .suffix(suffix),
            )
            .changed()
        {
            shape.edit_mask(id, property, time, values);
        }
    });
}

fn valid_bounds(bounds: Rect) -> bool {
    bounds.is_finite()
        && [bounds.width(), bounds.height()]
            .iter()
            .all(|size| MaskProperty::Size.limits().contains(size))
        && [bounds.center().x, bounds.center().y]
            .iter()
            .all(|center| MaskProperty::Position.limits().contains(center))
}

fn map(transform: Transform, p: Pos2) -> Pos2 {
    let (x, y) = point(transform, p.x, p.y);
    pos2(x, y)
}

fn screen_outline(path: &Path, transform: Transform) -> Vec<Pos2> {
    let to_screen = |p: Point| map(transform, pos2(p.x, p.y));
    let mut points = Vec::new();
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) | PathSegment::LineTo(p) => points.push(to_screen(p)),
            PathSegment::QuadTo(b, c) => {
                let [b, c] = [b, c].map(to_screen);
                let a = points.last().copied().unwrap_or(b);
                points.extend((1..=16).map(|i| {
                    let t = i as f32 / 16.0;
                    a.lerp(b, t).lerp(b.lerp(c, t), t)
                }));
            }
            PathSegment::CubicTo(b, c, d) => {
                let [b, c, d] = [b, c, d].map(to_screen);
                let a = points.last().copied().unwrap_or(b);
                points.extend((1..=16).map(|i| {
                    let t = i as f32 / 16.0;
                    let middle = b.lerp(c, t);
                    a.lerp(b, t)
                        .lerp(middle, t)
                        .lerp(middle.lerp(c.lerp(d, t), t), t)
                }));
            }
            PathSegment::Close => {}
        }
    }
    points
}

fn contains(points: &[Pos2], p: Pos2) -> bool {
    let mut inside = false;
    for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)) {
        let span = b - a;
        let length = span.length_sq();
        if length > 0.0 {
            let t = ((p - a).dot(span) / length).clamp(0.0, 1.0);
            if p.distance(a + span * t) <= 5.0 {
                return true;
            }
        }
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * span.x / span.y {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Modifiers, PointerButton, RawInput, Sense};

    struct Canvas {
        ctx: Context,
        editor: MaskEditor,
        scene: Scene,
    }

    impl Canvas {
        fn new(tool: Tool) -> Self {
            let mut scene = Scene::empty("mask-tool".into());
            (scene.width, scene.height) = (128.0, 128.0);
            scene.add_shape("layer".into(), "rect");
            Self {
                ctx: Context::default(),
                editor: MaskEditor {
                    layer: Some("layer".into()),
                    tool: Some(tool),
                    ..MaskEditor::default()
                },
                scene,
            }
        }

        fn show(&mut self, events: Vec<Event>) {
            let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::splat(128.0));
            let frame = self.scene.layer_frame(&self.scene.shapes[0], 0.0);
            self.ctx
                .run_ui(
                    RawInput {
                        screen_rect: Some(bounds),
                        events,
                        ..RawInput::default()
                    },
                    |ui| {
                        let response = ui.interact(
                            bounds,
                            ui.scope_id().with("test-canvas"),
                            Sense::click_and_drag(),
                        );
                        self.editor.canvas(
                            ui,
                            ui.painter(),
                            &response,
                            &mut self.scene.shapes[0],
                            frame,
                            Transform::identity(),
                            0.0,
                            true,
                        );
                    },
                )
                .textures_delta
                .clear();
        }

        fn click(&mut self, p: Pos2) {
            self.show(vec![Event::PointerMoved(p), button(p, true)]);
            self.show(vec![button(p, false)]);
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.show(vec![Event::PointerMoved(from), button(from, true)]);
            self.show(vec![Event::PointerMoved(to)]);
            self.show(vec![button(to, false)]);
        }
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn drawing_moving_and_resizing_a_mask_leave_the_layer_pose_alone() {
        let mut canvas = Canvas::new(Tool::Rectangle);
        let keys = canvas.scene.shapes[0].keys.clone();
        canvas.show(vec![]);
        canvas.drag(pos2(40.0, 40.0), pos2(80.0, 80.0));
        let [mask] = canvas.scene.shapes[0].masks.as_slice() else {
            panic!("expected a new mask")
        };
        assert_eq!(mask.rest.position, [-4.0, -4.0]);
        assert_eq!(mask.rest.size, [40.0, 40.0]);
        canvas.drag(pos2(58.0, 58.0), pos2(68.0, 62.0));
        canvas.drag(pos2(90.0, 84.0), pos2(104.0, 92.0));
        let shape = &canvas.scene.shapes[0];
        assert_eq!(shape.masks[0].rest.size, [54.0, 48.0]);
        assert_eq!(shape.masks[0].rest.position, [13.0, 4.0]);
        assert_eq!(shape.keys, keys);
    }

    #[test]
    fn clicking_the_first_point_closes_a_freeform_mask() {
        let mut canvas = Canvas::new(Tool::Path);
        canvas.show(vec![]);
        for p in [
            pos2(40.0, 40.0),
            pos2(80.0, 40.0),
            pos2(60.0, 80.0),
            pos2(40.0, 40.0),
        ] {
            canvas.click(p);
        }
        let [mask] = canvas.scene.shapes[0].masks.as_slice() else {
            panic!("expected a new mask")
        };
        let MaskShape::Path { points } = &mask.shape else {
            panic!("expected a freeform path")
        };
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].point, [-0.5, -0.5]);
        assert_eq!(points[2].point, [0.0, 0.5]);
        assert!(
            points
                .iter()
                .all(|p| p.incoming.is_none() && p.outgoing.is_none())
        );
    }

    #[test]
    fn masks_use_the_same_curved_pen_and_point_drag() {
        let mut canvas = Canvas::new(Tool::Path);
        canvas.show(vec![]);
        canvas.click(pos2(40.0, 40.0));
        canvas.drag(pos2(80.0, 40.0), pos2(88.0, 54.0));
        canvas.click(pos2(60.0, 80.0));
        canvas.click(pos2(40.0, 40.0));
        let MaskShape::Path { points } = &canvas.scene.shapes[0].masks[0].shape else {
            panic!("expected a closed mask");
        };
        let outgoing = points[1].outgoing.unwrap();
        let initial = points[1].point;
        assert_eq!(points[1].incoming, Some(outgoing.map(|v| -v)));
        canvas.editor.tool = Some(Tool::Points);
        canvas.drag(pos2(80.0, 40.0), pos2(95.0, 45.0));
        let MaskShape::Path { points } = &canvas.scene.shapes[0].masks[0].shape else {
            panic!("expected a closed mask");
        };
        assert!(points[1].point[0] > initial[0] && points[1].point[1] > initial[1]);
        assert_eq!(points[1].outgoing, Some(outgoing));
    }
}
