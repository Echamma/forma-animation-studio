use crate::model::Shape;
use crate::path_editor;
use crate::paths::PathData;
use crate::shape_style::{
    Cap, Dash, Gradient, Join, PaintMode, PaintStyle, PaintTarget, Stop, StrokeStyle, Trim,
    VectorLayer, VectorProperty, VectorValue,
};
use canopy_ui::color;
use egui::{Color32, Pos2, Rect, Response, Stroke, Ui, vec2};
use tiny_skia::Transform;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tool {
    Pen,
    Subpath,
    Points,
    Gradient(PaintTarget),
}

#[derive(Default)]
pub(crate) struct VectorTools {
    pub tool: Option<Tool>,
    pub pen: path_editor::Pen,
    points: path_editor::PointEditor,
    layer: Option<String>,
    time: Option<f64>,
    gradient_end: Option<usize>,
}

impl VectorTools {
    pub fn set(&mut self, tool: Option<Tool>, layer: Option<&str>) {
        *self = Self {
            tool,
            layer: layer.map(str::to_owned),
            ..Self::default()
        };
    }

    pub fn toggle(&mut self, tool: Tool, layer: Option<&str>) {
        self.set((self.tool != Some(tool)).then_some(tool), layer);
    }

    pub fn sync(&mut self, layer: Option<&str>, time: f64) {
        if self.tool.is_some_and(|tool| tool != Tool::Pen) && self.layer.as_deref() != layer {
            self.set(None, layer);
        }
        if self.time != Some(time) {
            self.points.cancel();
            self.gradient_end = None;
            self.time = Some(time);
        }
    }

    pub fn edit_points(
        &mut self,
        ui: &Ui,
        painter: &egui::Painter,
        response: &Response,
        shape: &mut Shape,
        mut path: PathData,
        view: Transform,
        time: f64,
        delete: bool,
    ) {
        let changed = if delete {
            self.points.delete(&mut path, 2)
        } else {
            self.points
                .canvas(ui, painter, response, &mut path, view, color::SUN)
        };
        if changed && let Some(vector) = &mut shape.vector {
            vector.edit(VectorProperty::Path, VectorValue::Path(path), time);
        }
    }

    pub fn gradient_canvas(
        &mut self,
        ui: &Ui,
        painter: &egui::Painter,
        response: &Response,
        shape: &mut Shape,
        target: PaintTarget,
        view: Transform,
        time: f64,
    ) {
        let Some(vector) = &mut shape.vector else {
            return;
        };
        let property = VectorProperty::GradientEnds(target);
        let Some(mut ends) = vector.numbers_at(property, time, &shape.ease) else {
            return;
        };
        let positions = |[x, y, a, b, _]: [f32; 5]| {
            [
                path_editor::map(view, [x, y]),
                path_editor::map(view, [a, b]),
            ]
        };
        let hit = move |at: Pos2| {
            positions(ends)
                .into_iter()
                .enumerate()
                .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
                .filter(|(_, pos)| pos.distance(at) <= 12.0)
                .map(|(i, _)| i)
        };
        let primary = egui::PointerButton::Primary;
        if response.drag_started_by(primary) {
            self.gradient_end = ui.input(|i| i.pointer.press_origin()).and_then(hit);
        }
        if let (Some(end), Some(at), Some(inverse)) = (
            self.gradient_end,
            response.interact_pointer_pos(),
            view.invert(),
        ) && (response.dragged_by(primary) || response.drag_stopped_by(primary))
        {
            let pos = path_editor::map(inverse, [at.x, at.y]);
            ends[end * 2] = property.clamp(end * 2, pos.x);
            ends[end * 2 + 1] = property.clamp(end * 2 + 1, pos.y);
            vector.edit(property, VectorValue::Numbers(ends), time);
        }
        if response.drag_stopped_by(primary) {
            self.gradient_end = None;
        }
        if response.hover_pos().and_then(hit).is_some() || self.gradient_end.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let points = positions(ends);
        painter.line_segment(points, Stroke::new(2.0, color::SUN));
        for (index, at) in points.into_iter().enumerate() {
            painter.circle(
                at,
                7.0,
                if index == 0 {
                    Color32::WHITE
                } else {
                    color::SUN
                },
                Stroke::new(2.0, color::SUN),
            );
        }
    }
}

pub(crate) fn fill_panel(
    ui: &mut Ui,
    shape: &mut Shape,
    size: [f32; 2],
    tools: &mut VectorTools,
    t: f64,
) -> bool {
    let id = shape.id.clone();
    ui.spacing_mut().interact_size.y = 32.0;
    if shape.kind == "path" {
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(tools.tool == Some(Tool::Points), "Edit points")
                .clicked()
            {
                tools.toggle(Tool::Points, Some(&id));
            }
            if ui
                .selectable_label(tools.tool == Some(Tool::Subpath), "Add subpath")
                .clicked()
            {
                tools.toggle(Tool::Subpath, Some(&id));
            }
        });
    } else if ui.button("Convert to path").clicked() && shape.convert_to_path(size) {
        tools.set(Some(Tool::Points), Some(&id));
    }
    let vector = shape.vector.as_mut();
    let mut mode = vector
        .as_ref()
        .and_then(|v| v.fill.as_ref())
        .map_or(PaintMode::Solid, |paint| paint.mode);
    if choose(ui, "Fill", &mut mode, &PaintMode::ALL, paint_label) {
        let vector = shape.vector.get_or_insert_with(VectorLayer::default);
        set_paint(&mut vector.fill, mode);
        vector.prune_keys();
    }
    if mode.gradient()
        && let Some(vector) = &mut shape.vector
    {
        gradient_panel(ui, vector, PaintTarget::Fill, tools, &id, t, &shape.ease);
    }
    mode == PaintMode::Solid
}

pub(crate) fn stroke_and_trim(ui: &mut Ui, shape: &mut Shape, tools: &mut VectorTools, t: f64) {
    let id = shape.id.clone();
    egui::CollapsingHeader::new("Stroke").show(ui, |ui| {
        let mut mode = shape
            .vector
            .as_ref()
            .and_then(|v| v.stroke.as_ref())
            .map_or(PaintMode::None, |s| s.paint.mode);
        if choose(ui, "Stroke paint", &mut mode, &PaintMode::ALL, paint_label) {
            let vector = shape.vector.get_or_insert_with(VectorLayer::default);
            if mode == PaintMode::None {
                vector.stroke = None;
            } else {
                let stroke = vector.stroke.get_or_insert_with(StrokeStyle::default);
                stroke.paint.mode = mode;
                if mode.gradient() {
                    stroke.paint.gradient.get_or_insert_with(Gradient::default);
                } else {
                    stroke.paint.gradient = None;
                }
            }
            vector.prune_keys();
        }
        let Some(vector) = shape.vector.as_mut().filter(|v| v.stroke.is_some()) else {
            return;
        };
        let ease = &shape.ease;
        if mode.gradient() {
            gradient_panel(ui, vector, PaintTarget::Stroke, tools, &id, t, ease);
        } else {
            let property = VectorProperty::StrokeColor;
            if let Some(mut values) = vector.numbers_at(property, t, ease)
                && rgba(ui, &mut values[..4])
            {
                vector.edit(property, VectorValue::Numbers(values), t);
            }
        }
        number(
            ui,
            vector,
            VectorProperty::StrokeWidth,
            "Width",
            " px",
            0.0..=4000.0,
            t,
            ease,
        );
        if let Some(stroke) = &mut vector.stroke {
            choose(
                ui,
                "Cap",
                &mut stroke.cap,
                &[Cap::Butt, Cap::Round, Cap::Square],
                |v| match v {
                    Cap::Butt => "Butt",
                    Cap::Round => "Round",
                    Cap::Square => "Square",
                },
            );
            choose(
                ui,
                "Join",
                &mut stroke.join,
                &[Join::Miter, Join::Round, Join::Bevel],
                |v| match v {
                    Join::Miter => "Miter",
                    Join::Round => "Round",
                    Join::Bevel => "Bevel",
                },
            );
            let mut dashed = stroke.dash.is_some();
            if ui.checkbox(&mut dashed, "Dashes").changed() {
                stroke.dash = dashed.then(Dash::default);
            }
            if let Some(dash) = &mut stroke.dash {
                field(ui, "Dash", &mut dash.length, 0.1..=4000.0, " px");
                field(ui, "Gap", &mut dash.gap, 0.1..=4000.0, " px");
            }
        }
        vector.prune_keys();
        number(
            ui,
            vector,
            VectorProperty::DashOffset,
            "Dash offset",
            " px",
            -36_000.0..=36_000.0,
            t,
            ease,
        );
    });
    egui::CollapsingHeader::new("Trim paths").show(ui, |ui| {
        let mut enabled = shape
            .vector
            .as_ref()
            .is_some_and(|vector| vector.trim.is_some());
        if ui.checkbox(&mut enabled, "Trim the outline").changed() {
            let vector = shape.vector.get_or_insert_with(VectorLayer::default);
            vector.trim = enabled.then(Trim::default);
            vector.prune_keys();
        }
        let Some(vector) = shape.vector.as_mut().filter(|v| v.trim.is_some()) else {
            return;
        };
        let ease = &shape.ease;
        number(
            ui,
            vector,
            VectorProperty::TrimStart,
            "Start",
            "%",
            0.0..=100.0,
            t,
            ease,
        );
        number(
            ui,
            vector,
            VectorProperty::TrimEnd,
            "End",
            "%",
            0.0..=100.0,
            t,
            ease,
        );
        number(
            ui,
            vector,
            VectorProperty::TrimOffset,
            "Offset",
            "°",
            -36_000.0..=36_000.0,
            t,
            ease,
        );
        if let Some(trim) = &mut vector.trim {
            ui.checkbox(&mut trim.fill, "Trim the fill too");
        }
    });
    if shape.vector.as_ref() == Some(&VectorLayer::default()) {
        shape.vector = None;
    }
}

fn gradient_panel(
    ui: &mut Ui,
    vector: &mut VectorLayer,
    target: PaintTarget,
    tools: &mut VectorTools,
    layer: &str,
    t: f64,
    ease: &str,
) {
    let Some(count) = vector
        .paint(target)
        .and_then(|p| p.gradient.as_ref())
        .map(|g| g.stops.len())
    else {
        return;
    };
    let tool = Tool::Gradient(target);
    if ui
        .add(
            egui::Button::selectable(tools.tool == Some(tool), "Edit gradient on canvas")
                .min_size(vec2(0.0, 32.0)),
        )
        .clicked()
    {
        tools.toggle(tool, Some(layer));
    }
    egui::CollapsingHeader::new("Gradient stops")
        .id_salt(target)
        .show(ui, |ui| {
            for i in 0..count {
                let property = VectorProperty::GradientStop(target, i);
                let Some(mut values) = vector.numbers_at(property, t, ease) else {
                    continue;
                };
                let mut remove = false;
                let changed = ui
                    .push_id((target, i), |ui| {
                        let mut changed = field(
                            ui,
                            &format!("Stop {}", i + 1),
                            &mut values[0],
                            0.0..=100.0,
                            "%",
                        );
                        changed |= rgba(ui, &mut values[1..]);
                        if count > 2 {
                            remove = ui.button("Remove stop").clicked();
                        }
                        changed
                    })
                    .inner;
                if remove {
                    vector.remove_stop(target, i);
                    return;
                }
                if changed {
                    vector.edit(property, VectorValue::Numbers(values), t);
                }
                ui.separator();
            }
            if count < 64
                && ui.button("Add stop").clicked()
                && let Some(gradient) = vector
                    .paint_mut(target)
                    .and_then(|paint| paint.gradient.as_mut())
            {
                gradient.stops.push(Stop {
                    offset: 50.0,
                    rgba: [172.0, 172.0, 172.0, 100.0],
                });
            }
        });
}

fn set_paint(paint: &mut Option<PaintStyle>, mode: PaintMode) {
    if mode == PaintMode::Solid {
        *paint = None;
        return;
    }
    let paint = paint.get_or_insert_with(|| PaintStyle::new(mode));
    paint.mode = mode;
    if mode.gradient() {
        paint.gradient.get_or_insert_with(Gradient::default);
    } else {
        paint.gradient = None;
    }
}

fn paint_label(mode: PaintMode) -> &'static str {
    match mode {
        PaintMode::None => "None",
        PaintMode::Solid => "Solid",
        PaintMode::Linear => "Linear gradient",
        PaintMode::Radial => "Radial gradient",
    }
}

fn choose<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    items: &[T],
    text: impl Fn(T) -> &'static str,
) -> bool {
    let before = *value;
    ui.label(label);
    egui::ComboBox::from_id_salt(label)
        .selected_text(text(*value))
        .show_ui(ui, |ui| {
            for &item in items {
                ui.selectable_value(value, item, text(item));
            }
        });
    *value != before
}

fn number(
    ui: &mut Ui,
    vector: &mut VectorLayer,
    property: VectorProperty,
    label: &str,
    suffix: &str,
    range: std::ops::RangeInclusive<f32>,
    t: f64,
    ease: &str,
) {
    if let Some(mut values) = vector.numbers_at(property, t, ease)
        && field(ui, label, &mut values[0], range, suffix)
    {
        vector.edit(property, VectorValue::Numbers(values), t);
    }
}

fn field(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::DragValue::new(value)
                .range(range)
                .speed(0.5)
                .suffix(suffix),
        )
        .changed()
    })
    .inner
}

fn rgba(ui: &mut Ui, values: &mut [f32]) -> bool {
    let mut rgb = [values[0] as u8, values[1] as u8, values[2] as u8];
    let changed = ui
        .horizontal(|ui| {
            ui.label("Color");
            egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed()
        })
        .inner;
    if changed {
        for (channel, value) in rgb.into_iter().zip(&mut values[..3]) {
            *value = f32::from(channel);
        }
    }
    field(ui, "Opacity", &mut values[3], 0.0..=100.0, "%") || changed
}

pub(crate) fn draw_hint(ui: &mut Ui, rect: Rect, tool: Tool) {
    let label = match tool {
        Tool::Pen | Tool::Subpath => {
            "Click for a corner, drag for curves. Click the first point to close, or press Enter to finish."
        }
        Tool::Points => {
            "Drag points or handles. Click a segment to add a point, or double-click a point to change it."
        }
        Tool::Gradient(_) => "Drag the gradient ends.",
    };
    let rect = Rect::from_min_max(rect.min + vec2(8.0, 42.0), rect.max - vec2(8.0, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        egui::Frame::new()
            .fill(canopy_ui::Mood::Ember.pane_solid())
            .inner_margin(7.0)
            .corner_radius(6.0)
            .show(ui, |ui| {
                ui.label(label);
            });
    });
}
