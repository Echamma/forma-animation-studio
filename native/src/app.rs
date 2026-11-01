//! Native Forma scene editor: shape layers, keyframes, timeline and the same project folder as Electron Forma.

use crate::animated_svg;
use crate::archive;
use crate::assets;
use crate::model::{HANDLES, Pose, RotationClip, Scene, resized_pose};
use crate::render;
use crate::storage;
use crate::video;
use crate::videos::Videos;
use canopy_ui::{CanopyApp, Host, Place, Receipt, SharedFile, color, font};
use egui::{
    Align2, Color32, Pos2, Rect, Sense, Stroke, TextureHandle,
    TextureOptions, Ui, UiBuilder, Vec2, pos2, vec2,
};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A drag on the canvas, with what it started from (scene pixels, editable poses).
#[derive(Clone, Copy)]
enum Gesture {
    Move { index: usize, start: Pos2, pose: Pose },
    Resize { index: usize, handle: (i8, i8), start: Pos2, pose: Pose, angle: f32 },
    Rotate { index: usize, center: Pos2, last: f32 },
}

/// What Electron Forma's export dialog can make.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportKind {
    Video,
    Svg,
    Picture,
    Project,
}

fn new_id() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (n >> 96) as u32,
        (n >> 80) as u16,
        (n >> 64) as u16,
        (n >> 48) as u16,
        n as u64 & 0xffff_ffff_ffff
    )
}

pub struct Forma {
    place: Place,
    scene: Scene,
    selected: Option<String>,
    time: f64,
    playing: bool,
    last_tick: Instant,
    edited_at: Option<Instant>,
    show_projects: bool,
    show_export: bool,
    export_kind: ExportKind,
    video_settings: video::Settings,
    /// Draws the preview the way exports draw frames. Made again when the scene's pictures change.
    renderer: Option<render::Renderer>,
    /// The preview picture, and what it shows: the scene, the time, its size in pixels, and which video frames.
    preview: Option<(TextureHandle, Scene, f64, [u32; 2], u64)>,
    videos: Videos,
    undo: Vec<Scene>,
    redo: Vec<Scene>,
    recording_drag: bool,
    /// A drag on the canvas: moving, resizing or turning a layer.
    gesture: Option<Gesture>,
    suppress_history: bool,
    export: Option<video::Export>,
}

impl Forma {
    pub fn new(place: Place) -> Self {
        let scene = storage::list(&place)
            .first()
            .and_then(|doc| storage::load_document(&place, &doc.id).ok())
            .unwrap_or_else(|| Scene::empty(new_id()));
        Self {
            place,
            scene,
            selected: None,
            time: 0.0,
            playing: false,
            last_tick: Instant::now(),
            edited_at: None,
            show_projects: false,
            show_export: false,
            export_kind: ExportKind::Video,
            video_settings: video::Settings::default(),
            renderer: None,
            preview: None,
            videos: Videos::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            recording_drag: false,
            gesture: None,
            suppress_history: false,
            export: None,
        }
    }

    fn edited(&mut self) {
        self.edited_at = Some(Instant::now());
    }

    fn open(&mut self, scene: Scene) {
        let _ = self.save();
        self.scene = scene;
        self.selected = None;
        self.time = 0.0;
        self.playing = false;
        self.renderer = None;
        self.preview = None;
        self.videos = Videos::default();
        self.undo.clear();
        self.redo.clear();
        self.suppress_history = true;
    }

    /// A new media layer in the middle of the scene, named and sized the way Electron Forma imports media.
    fn add_media(&mut self, file_name: &str, kind: &str, field: &str, value: String, width: f32, height: f32) -> String {
        let id = self.scene.add_shape(new_id(), kind);
        let (w, h) = self.scene.fitted_size(width, height);
        let shape = self.scene.shapes.last_mut().unwrap();
        let stem = file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem);
        shape.name = if stem.is_empty() { "Media".into() } else { stem.chars().take(70).collect() };
        shape.color = "#888888".into();
        shape.extra.insert(field.into(), value.into());
        let pose = Pose { w, h, ..shape.pose(0.0) };
        shape.edit_pose(0.0, pose);
        id
    }

    fn undo(&mut self) {
        if let Some(scene) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.scene, scene));
            self.suppress_history = true;
            self.playing = false;
            self.edited();
        }
    }

    fn redo(&mut self) {
        if let Some(scene) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.scene, scene));
            self.suppress_history = true;
            self.playing = false;
            self.edited();
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if ui.button("Projects").clicked() {
                self.show_projects = true;
            }
            if ui.button("New").clicked() {
                self.open(Scene::empty(new_id()));
            }
            if ui
                .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                .clicked()
            {
                self.redo();
            }
            let name = ui.add(
                egui::TextEdit::singleline(&mut self.scene.project_name)
                    .desired_width(180.0)
                    .font(font::bold(14.0)),
            );
            if name.changed() {
                self.edited();
            }
            if ui
                .button(if self.playing { "Pause" } else { "Play" })
                .clicked()
            {
                self.playing = !self.playing;
                self.last_tick = Instant::now();
            }
            ui.label(format!("{:.2} / {:.2} s", self.time, self.scene.duration));
            if ui.button("Save").clicked() {
                match self.save() {
                    Ok(()) => host.toast("Forma project saved".into()),
                    Err(error) => host.toast(error),
                }
            }
            if ui
                .add_enabled(self.export.is_none(), egui::Button::new("Export"))
                .on_hover_text("A video, a picture or the project file, into Files")
                .clicked()
            {
                self.show_export = true;
            }
            if let Some(export) = &self.export {
                let fraction = export.progress.load(Ordering::Relaxed) as f32 / 1000.0;
                ui.add(egui::ProgressBar::new(fraction).desired_width(75.0));
                if ui.button("Stop").clicked() {
                    export.cancel.store(true, Ordering::Relaxed);
                }
            }
        });
    }

    fn layers(&mut self, ui: &mut Ui) {
        ui.label(
            egui::RichText::new("Layers")
                .font(font::bold(14.0))
                .color(color::INK),
        );
        ui.add_space(6.0);
        for (label, kind) in [
            ("Circle", "circle"),
            ("Rectangle", "rect"),
            ("Triangle", "triangle"),
            ("Diamond", "diamond"),
        ] {
            if ui.button(format!("+ {label}")).clicked() {
                let id = self.scene.add_shape(new_id(), kind);
                self.selected = Some(id);
                self.edited();
            }
        }
        if ui.button("+ Text").clicked() {
            let id = self.scene.add_text(new_id());
            self.selected = Some(id);
            self.edited();
        }
        ui.separator();
        if self.selected.is_some() {
            if ui.button("Duplicate selected").clicked() {
                if let Some(shape) = self
                    .scene
                    .shapes
                    .iter()
                    .find(|s| Some(s.id.as_str()) == self.selected.as_deref())
                    .cloned()
                {
                    let mut copy = shape;
                    copy.id = new_id();
                    copy.name = format!("{} copy", copy.name);
                    for key in &mut copy.keys {
                        key.x += 20.0;
                        key.y += 20.0;
                    }
                    self.selected = Some(copy.id.clone());
                    self.scene.shapes.push(copy);
                    self.edited();
                }
            }
            if ui.button("Delete selected").clicked() {
                self.scene
                    .shapes
                    .retain(|shape| Some(shape.id.as_str()) != self.selected.as_deref());
                self.selected = None;
                self.edited();
            }
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for shape in &self.scene.shapes {
                if ui
                    .selectable_label(self.selected.as_deref() == Some(&shape.id), &shape.name)
                    .clicked()
                {
                    self.selected = Some(shape.id.clone());
                }
            }
        });
    }

    fn inspector(&mut self, ui: &mut Ui) {
        ui.label(
            egui::RichText::new("Layer")
                .font(font::bold(14.0))
                .color(color::INK),
        );
        let Some(index) = self
            .scene
            .shapes
            .iter()
            .position(|s| Some(s.id.as_str()) == self.selected.as_deref())
        else {
            ui.label("Select a shape to edit it.");
            ui.separator();
            ui.label(egui::RichText::new("Scene").font(font::bold(14.0)));
            let portrait = self.scene.height > self.scene.width;
            ui.horizontal(|ui| {
                if ui.selectable_label(!portrait, "Landscape").clicked() && portrait {
                    self.scene.resize(960.0, 600.0);
                    self.edited();
                }
                if ui.selectable_label(portrait, "Portrait").clicked() && !portrait {
                    self.scene.resize(1080.0, 1920.0);
                    self.edited();
                }
            });
            ui.label("Background (hex)");
            if ui.text_edit_singleline(&mut self.scene.bg).changed() {
                self.edited();
            }
            let min_duration = self.scene.minimum_duration();
            if ui
                .add(
                    egui::Slider::new(
                        &mut self.scene.duration,
                        min_duration..=120.0_f64.max(min_duration),
                    )
                    .text("Length")
                    .suffix(" s"),
                )
                .changed()
            {
                self.edited();
            }
            return;
        };
        let shape = &mut self.scene.shapes[index];
        let mut changed = false;
        changed |= ui.text_edit_singleline(&mut shape.name).changed();
        if shape.kind == "text" {
            ui.label("Text");
            let mut content = shape.extra.get("text").and_then(|value| value.as_str()).unwrap_or("").to_owned();
            if ui.add(egui::TextEdit::multiline(&mut content).desired_rows(3)).changed() {
                shape.extra.insert("text".into(), content.into());
                changed = true;
            }
        }
        ui.label("Fill (hex)");
        changed |= ui.text_edit_singleline(&mut shape.color).changed();
        let mut pose = shape.pose(self.time);
        for (label, value, range) in [
            ("X", &mut pose.x, -4000.0..=4000.0),
            ("Y", &mut pose.y, -4000.0..=4000.0),
            ("Width", &mut pose.w, 1.0..=4000.0),
            ("Height", &mut pose.h, 1.0..=4000.0),
            ("Angle", &mut pose.r, -3600.0..=3600.0),
            ("Opacity", &mut pose.o, 0.0..=100.0),
        ] {
            changed |= ui
                .add(egui::Slider::new(value, range).text(label))
                .changed();
        }
        if changed {
            shape.edit_pose(self.time, pose);
        }
        ui.separator();
        let added = ui.button("Add keyframe here").clicked();
        if added {
            shape.edit_pose(self.time, pose);
        }
        ui.label(format!("{} keyframes", shape.keys.len()));
        let mut animated = false;
        ui.horizontal(|ui| {
            if ui.button("Move →").clicked() {
                shape.edit_pose(self.time, pose);
                shape.edit_pose(
                    self.time + 1.0,
                    Pose {
                        x: pose.x + 180.0,
                        ..pose
                    },
                );
                animated = true;
            }
            if ui.button("Fade out").clicked() {
                shape.edit_pose(self.time, pose);
                shape.edit_pose(self.time + 1.0, Pose { o: 0.0, ..pose });
                animated = true;
            }
            if ui.button("Spin").clicked() {
                shape.rotation_clips.push(RotationClip {
                    id: new_id(),
                    start_time: self.time,
                    duration: 1.0,
                    degrees: 360.0,
                    ease: "linear".into(),
                    extra: Default::default(),
                });
                animated = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Easing");
            egui::ComboBox::from_id_salt(("ease", &shape.id))
                .selected_text(&shape.ease)
                .show_ui(ui, |ui| {
                    for (label, kind) in [
                        ("Smooth", "smooth"),
                        ("Steady", "linear"),
                        ("Hold", "hold"),
                        ("Speed up", "ease-in"),
                        ("Slow down", "ease-out"),
                    ] {
                        if ui
                            .selectable_value(&mut shape.ease, kind.into(), label)
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });
        if animated {
            self.scene.duration = self.scene.duration.max(self.time + 1.0);
        }
        if changed || added || animated {
            self.edited();
        }
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let area = ui.max_rect().shrink(14.0);
        let scale = (area.width() / self.scene.width)
            .min(area.height() / self.scene.height)
            .max(0.01);
        let canvas = Rect::from_center_size(
            area.center(),
            vec2(self.scene.width * scale, self.scene.height * scale),
        );
        if self.renderer.as_ref().is_none_or(|r| !r.matches(&self.scene)) {
            self.renderer = Some(render::Renderer::new(&self.scene, &self.place));
        }
        let renderer = self.renderer.as_ref().unwrap();
        // Drawn at the screen's pixel size, again only when the scene, the time or the size changes.
        let pixels = ui.ctx().pixels_per_point();
        let size = [(canvas.width() * pixels).round() as u32, (canvas.height() * pixels).round() as u32];
        let scale = size[0] as f32 / self.scene.width;
        if self.videos.update(&self.scene, renderer, self.time, self.playing, scale) {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        let generation = self.videos.generation;
        let stale = self.preview.as_ref().is_none_or(|(_, scene, time, drawn, shown)| {
            *drawn != size || *time != self.time || *scene != self.scene || *shown != generation
        });
        if stale {
            if let Some(pixmap) = renderer.preview(&self.scene, self.time, scale, &self.videos.frames) {
                let image = egui::ColorImage::from_rgba_premultiplied(
                    [pixmap.width() as usize, pixmap.height() as usize],
                    pixmap.data(),
                );
                match &mut self.preview {
                    Some((texture, scene, time, drawn, shown)) => {
                        texture.set(image, TextureOptions::LINEAR);
                        (*scene, *time, *drawn, *shown) = (self.scene.clone(), self.time, size, generation);
                    }
                    None => {
                        let texture = ui.ctx().load_texture("forma-preview", image, TextureOptions::LINEAR);
                        self.preview = Some((texture, self.scene.clone(), self.time, size, generation));
                    }
                }
            }
        }
        let painter = ui.painter();
        painter.rect_filled(area, 10.0, Color32::from_rgb(24, 29, 37));
        match &self.preview {
            Some((texture, ..)) => painter.image(
                texture.id(),
                canvas,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            ),
            None => painter.rect_filled(canvas, 3.0, hex(&self.scene.bg, 255)),
        };
        // Layers as drawn on screen: center, size in points and angle, for hit testing (topmost last).
        let on_screen = |pose: Pose| (pos2(canvas.left() + pose.x * scale, canvas.top() + pose.y * scale), vec2(pose.w, pose.h) * scale, pose.r);
        let local = |p: Pos2, (center, _, angle): (Pos2, Vec2, f32)| {
            let (d, a) = (p - center, (-angle).to_radians());
            vec2(d.x * a.cos() - d.y * a.sin(), d.x * a.sin() + d.y * a.cos())
        };
        let turn = |v: Vec2, angle: f32| {
            let a = angle.to_radians();
            vec2(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos())
        };
        let mut hits = Vec::new();
        for shape in &self.scene.shapes {
            if !shape.visible(self.time, self.scene.duration) {
                continue;
            }
            let pose = self.scene.render_pose(shape, self.time);
            let (center, size, angle) = on_screen(pose);
            if renderer.leaves_out(shape) {
                let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| center + turn(vec2(x, y) * size / 2.0, angle));
                painter.add(egui::Shape::convex_polygon(corners.to_vec(), hex(&shape.color_at(self.time), (pose.o * 2.55).round() as u8), Stroke::NONE));
                painter.text(center, Align2::CENTER_CENTER, format!("{} layer", shape.kind), font::ui(12.0), Color32::BLACK);
            }
            hits.push((shape.id.clone(), (center, size, angle)));
        }
        // The selected layer's box, resize handles and rotation handle (Electron's selection control).
        let selected = self.selected.as_deref().and_then(|id| hits.iter().find(|(hit, _)| hit == id)).map(|(_, place)| *place);
        let mut handles = Vec::new();
        let mut rotate_at = None;
        if let (Some((center, size, angle)), false) = (selected, self.playing) {
            let smallest = size.x.min(size.y);
            let (knob, reach) = ((smallest * 0.28).clamp(8.0, 12.0), (smallest * 0.5).clamp(12.0, 28.0));
            let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| center + turn(vec2(x, y) * size / 2.0, angle));
            painter.add(egui::Shape::closed_line(corners.to_vec(), Stroke::new(1.5, color::SUN)));
            for (sx, sy) in HANDLES {
                if (sx == 0 && size.x < 48.0) || (sy == 0 && size.y < 48.0) {
                    continue;
                }
                let at = center + turn(vec2(sx as f32 * size.x, sy as f32 * size.y) / 2.0, angle);
                let square = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| at + turn(vec2(x, y) * knob / 2.0, angle));
                painter.add(egui::Shape::convex_polygon(square.to_vec(), Color32::WHITE, Stroke::new(1.0, color::SUN)));
                handles.push(((sx, sy), at, reach));
            }
            let top = center + turn(vec2(0.0, -size.y / 2.0), angle);
            let knob_at = center + turn(vec2(0.0, -size.y / 2.0 - 34.0), angle);
            painter.line_segment([top, knob_at], Stroke::new(1.5, color::SUN));
            painter.circle(knob_at, 10.0, Color32::WHITE, Stroke::new(1.5, color::SUN));
            painter.circle_stroke(knob_at, 4.5, Stroke::new(1.5, color::SUN));
            rotate_at = Some(knob_at);
        }
        let response = ui.interact(
            canvas.expand(48.0).intersect(ui.max_rect()),
            ui.scope_id().with("forma-canvas"),
            Sense::click_and_drag(),
        );
        let near_handle = |p: Pos2| {
            handles.iter().find(|(_, at, reach)| {
                let d = local(p, (*at, Vec2::ZERO, selected.map_or(0.0, |s| s.2)));
                d.x.abs() <= reach / 2.0 && d.y.abs() <= reach / 2.0
            }).map(|(handle, ..)| *handle)
        };
        let on_rotate = |p: Pos2| rotate_at.is_some_and(|at| at.distance(p) <= 12.0);
        let topmost = |p: Pos2| hits.iter().rev().find(|(_, place)| {
            let d = local(p, *place);
            d.x.abs() <= place.1.x / 2.0 && d.y.abs() <= place.1.y / 2.0
        }).map(|(id, _)| id.clone());
        if let Some(p) = response.hover_pos() {
            if on_rotate(p) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            } else if let Some((sx, sy)) = near_handle(p) {
                // Electron's resizeCursor: the handle's direction on screen, to the nearest 45 degrees.
                let degrees = ((sy as f32).atan2(sx as f32).to_degrees() + selected.map_or(0.0, |s| s.2)).rem_euclid(180.0);
                ui.ctx().set_cursor_icon([egui::CursorIcon::ResizeHorizontal, egui::CursorIcon::ResizeNwSe, egui::CursorIcon::ResizeVertical, egui::CursorIcon::ResizeNeSw][(degrees / 45.0).round() as usize % 4]);
            } else if topmost(p).is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            }
        }
        if response.drag_started() || response.clicked() {
            // Where the button went down: by the time a drag starts, the pointer has moved off a small handle.
            if let Some(p) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos()) {
                let index = |id: &str| self.scene.shapes.iter().position(|s| s.id == id);
                let to_scene = |p: Pos2| pos2((p.x - canvas.left()) / scale, (p.y - canvas.top()) / scale);
                self.gesture = None;
                if let (true, Some(i)) = (on_rotate(p), self.selected.as_deref().and_then(index)) {
                    let center = self.scene.render_pose(&self.scene.shapes[i], self.time);
                    let point = to_scene(p);
                    self.gesture = Some(Gesture::Rotate { index: i, center: pos2(center.x, center.y), last: (point.y - center.y).atan2(point.x - center.x).to_degrees() });
                } else if let (Some(handle), Some(i)) = (near_handle(p), self.selected.as_deref().and_then(index)) {
                    let shape = &self.scene.shapes[i];
                    self.gesture = Some(Gesture::Resize { index: i, handle, start: to_scene(p), pose: self.scene.editable_pose(shape, self.time), angle: self.scene.render_pose(shape, self.time).r });
                } else {
                    self.selected = topmost(p);
                    if let Some(i) = self.selected.as_deref().and_then(index) {
                        self.gesture = Some(Gesture::Move { index: i, start: to_scene(p), pose: self.scene.editable_pose(&self.scene.shapes[i], self.time) });
                    }
                }
                if response.clicked() {
                    self.gesture = None;
                }
            }
        }
        if response.dragged() {
            if let (Some(gesture), Some(p)) = (self.gesture, response.interact_pointer_pos()) {
                let point = pos2((p.x - canvas.left()) / scale, (p.y - canvas.top()) / scale);
                let freeform = ui.input(|i| i.modifiers.shift);
                match gesture {
                    Gesture::Move { index, start, pose } => {
                        let moved = Pose { x: pose.x + point.x - start.x, y: pose.y + point.y - start.y, ..pose };
                        self.scene.set_pose(index, self.time, moved);
                    }
                    Gesture::Resize { index, handle, start, pose, angle } => {
                        let resized = resized_pose(pose, angle, handle, (point.x - start.x, point.y - start.y), freeform);
                        self.scene.set_pose(index, self.time, resized);
                    }
                    // Turning a layer by hand changes its base rotation, as in Electron, not its keyframes.
                    Gesture::Rotate { index, center, last } => {
                        let angle = (point.y - center.y).atan2(point.x - center.x).to_degrees();
                        let delta = (angle - last + 540.0).rem_euclid(360.0) - 180.0;
                        let shape = &mut self.scene.shapes[index];
                        shape.base_rotation = (shape.base_rotation + delta).clamp(-10000.0, 10000.0);
                        self.gesture = Some(Gesture::Rotate { index, center, last: angle });
                    }
                }
                self.edited();
            }
        }
        if response.drag_stopped() {
            self.gesture = None;
        }
    }

    fn timeline(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, 8.0, Color32::from_rgb(31, 37, 48));
        let left = rect.left() + 130.0;
        let width = (rect.width() - 150.0).max(1.0);
        let x = |t: f64| left + width * (t / self.scene.duration).clamp(0.0, 1.0) as f32;
        for tick in 0..=self.scene.duration.ceil() as usize {
            let pos = x(tick as f64);
            ui.painter().line_segment(
                [pos2(pos, rect.top() + 6.0), pos2(pos, rect.bottom())],
                Stroke::new(0.5, color::HAIRLINE),
            );
            ui.painter().text(
                pos2(pos + 3.0, rect.top() + 4.0),
                Align2::LEFT_TOP,
                format!("{tick}s"),
                font::ui(10.0),
                color::INK_MUTED,
            );
        }
        let mut selected = None;
        for (index, shape) in self.scene.shapes.iter().enumerate() {
            let y = rect.top() + 29.0 + index as f32 * 30.0;
            if y + 24.0 > rect.bottom() {
                break;
            }
            let row =
                Rect::from_min_size(pos2(rect.left() + 5.0, y), vec2(rect.width() - 10.0, 24.0));
            if ui
                .interact(row, ui.scope_id().with(("row", &shape.id)), Sense::click())
                .clicked()
            {
                selected = Some(shape.id.clone());
            }
            ui.painter().text(
                pos2(row.left() + 5.0, y + 12.0),
                Align2::LEFT_CENTER,
                &shape.name,
                font::ui(11.0),
                color::INK,
            );
            let bar = Rect::from_min_max(
                pos2(x(shape.in_time.unwrap_or(0.0)), y + 3.0),
                pos2(x(shape.out_time.unwrap_or(self.scene.duration)), y + 21.0),
            );
            ui.painter().rect_filled(
                bar,
                4.0,
                if self.selected.as_deref() == Some(&shape.id) {
                    color::SUN
                } else {
                    color::INK_SOFT
                },
            );
            for key in &shape.keys {
                ui.painter()
                    .circle_filled(pos2(x(key.t), y + 12.0), 3.0, Color32::WHITE);
            }
        }
        if let Some(id) = selected {
            self.selected = Some(id);
        }
        let play_x = x(self.time);
        ui.painter().line_segment(
            [pos2(play_x, rect.top()), pos2(play_x, rect.bottom())],
            Stroke::new(2.0, color::SUN),
        );
        let ruler = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 25.0));
        let response = ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
        if (response.clicked() || response.dragged()) && response.interact_pointer_pos().is_some() {
            self.time = (((response.interact_pointer_pos().unwrap().x - left) / width) as f64
                * self.scene.duration)
                .clamp(0.0, self.scene.duration);
            self.playing = false;
        }
    }

    /// Electron Forma's export dialog: what to make, the video's options, and what it will contain.
    fn export_window(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.show_export {
            return;
        }
        let mut run = false;
        let modal = egui::Modal::new(ui.scope_id().with("forma-export")).show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new("Export").font(font::bold(18.0)).color(color::INK));
            ui.add_space(6.0);
            for (kind, title, detail) in [
                (ExportKind::Video, "Video", "MP4 or WebM, with sound. Plays anywhere."),
                (ExportKind::Svg, "Animated SVG", "A small looping file for websites."),
                (ExportKind::Picture, "Picture", "A PNG of the frame at the playhead."),
                (ExportKind::Project, "Project file", "Everything, to open in Forma elsewhere."),
            ] {
                ui.radio_value(&mut self.export_kind, kind, egui::RichText::new(format!("{title}  ·  {detail}")).font(font::ui(13.0)));
            }
            ui.separator();
            let scene = &self.scene;
            if self.export_kind == ExportKind::Video {
                let settings = &mut self.video_settings;
                egui::Grid::new("forma-video-options").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Format");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut settings.format, video::Format::Mp4, "MP4");
                        ui.selectable_value(&mut settings.format, video::Format::WebM, "WebM");
                    });
                    ui.end_row();
                    ui.label("Smoothness");
                    ui.horizontal(|ui| {
                        for fps in [24, 30, 60] {
                            ui.selectable_value(&mut settings.fps, fps, fps.to_string());
                        }
                        ui.label(egui::RichText::new("frames per second").color(color::INK_MUTED));
                    });
                    ui.end_row();
                    ui.label("Size");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut settings.scale, 1, "Normal");
                        ui.selectable_value(&mut settings.scale, 2, "Large");
                        let (w, h) = settings.size(scene);
                        ui.label(egui::RichText::new(format!("{w} × {h} pixels")).color(color::INK_MUTED));
                    });
                    ui.end_row();
                });
                ui.separator();
            }
            let summary = match self.export_kind {
                ExportKind::Video => {
                    let sound = if scene.shapes.iter().any(|s| s.kind == "video") { "  ·  Includes sound" } else { "" };
                    format!("{:.1} seconds · {} frames{sound}", scene.duration, self.video_settings.frames(scene))
                }
                ExportKind::Svg => {
                    let video = if scene.shapes.iter().any(|s| s.kind == "video") { "  ·  Video layers show their first frame." } else { "" };
                    format!("{:.1} seconds, looping{video}", scene.duration)
                }
                ExportKind::Picture => format!("The frame at {:.2} s · {} × {}", self.time, scene.width, scene.height),
                ExportKind::Project => {
                    let n = scene.shapes.len();
                    format!("{n} layer{} with their pictures and video", if n == 1 { "" } else { "s" })
                }
            };
            ui.label(egui::RichText::new(summary).font(font::bold(13.0)));
            ui.add_space(6.0);
            let label = match self.export_kind {
                ExportKind::Video => "Export video",
                ExportKind::Svg => "Export SVG",
                ExportKind::Picture => "Save picture",
                ExportKind::Project => "Save project file",
            };
            run = ui.button(label).clicked();
        });
        if run {
            self.show_export = false;
            self.export(host, ui.ctx());
        } else if modal.should_close() {
            self.show_export = false;
        }
    }

    /// Makes the export chosen in the dialog, named after the project, into Files (a video on its own thread).
    fn export(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let extension = match self.export_kind {
            ExportKind::Video => self.video_settings.extension(),
            ExportKind::Svg => "svg",
            ExportKind::Picture => "png",
            ExportKind::Project => "forma",
        };
        if let Err(error) = std::fs::create_dir_all(&self.place.exports_dir) {
            return host.toast(error.to_string());
        }
        let name = canopy_ui::canopy_core::files::unique_name(
            &self.place.exports_dir,
            &format!("{}.{extension}", self.scene.project_name),
        );
        let path = self.place.exports_dir.join(name);
        let done = match self.export_kind {
            ExportKind::Video => match video::start(&self.scene, &self.place, path, self.video_settings, ctx.clone()) {
                Ok(export) => {
                    self.export = Some(export);
                    return;
                }
                Err(error) => Err(error),
            },
            ExportKind::Svg => animated_svg::animated(&self.scene, &self.place)
                .and_then(|svg| std::fs::write(&path, svg).map_err(|e| e.to_string()))
                .map(|_| path),
            ExportKind::Picture => render::save_png(&self.scene, &self.place, self.time, &path).map(|_| path),
            ExportKind::Project => archive::pack(&self.scene, &self.place, &path).map(|_| path),
        };
        match done {
            Ok(path) => host.exported(path),
            Err(error) => host.toast(error),
        }
    }

    fn projects_window(&mut self, ui: &mut Ui) {
        if !self.show_projects {
            return;
        }
        let mut opened = None;
        let modal = egui::Modal::new(ui.scope_id().with("forma-projects")).show(ui.ctx(), |ui| {
            ui.set_width(350.0);
            ui.heading("Forma projects");
            for doc in storage::list(&self.place) {
                if ui
                    .button(format!("{}  ·  {}", doc.name, doc.detail))
                    .clicked()
                {
                    opened = Some(doc.id);
                }
            }
        });
        if let Some(id) = opened {
            if let Ok(scene) = storage::load_document(&self.place, &id) {
                self.open(scene);
            }
            self.show_projects = false;
        } else if modal.should_close() {
            self.show_projects = false;
        }
    }
}

impl CanopyApp for Forma {
    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if let Some(export) = &self.export {
            match export.result.try_recv() {
                Ok(Ok(path)) => {
                    host.exported(path);
                    self.export = None;
                }
                Ok(Err(error)) => {
                    host.toast(error);
                    self.export = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    host.toast("Video export stopped unexpectedly".into());
                    self.export = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        let before = self.scene.clone();
        if host.focused() {
            let undo = ui.input(|i| {
                i.modifiers.command && i.key_pressed(egui::Key::Z) && !i.modifiers.shift
            });
            let redo = ui.input(|i| {
                i.modifiers.command
                    && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z)))
            });
            if undo {
                self.undo();
            } else if redo {
                self.redo();
            }
        }
        if self.playing {
            let now = Instant::now();
            self.time += now.duration_since(self.last_tick).as_secs_f64();
            self.last_tick = now;
            if self.time >= self.scene.duration {
                self.time = 0.0;
            }
            ui.ctx().request_repaint();
        }
        if self
            .edited_at
            .is_some_and(|at| at.elapsed() > Duration::from_millis(800))
        {
            if let Err(error) = self.save() {
                host.toast(format!("Forma couldn't save: {error}"));
            }
        } else if self.edited_at.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(850));
        }
        let rect = ui.max_rect();
        let top = Rect::from_min_size(rect.min, vec2(rect.width(), 43.0));
        ui.scope_builder(
            UiBuilder::new().max_rect(top.shrink2(vec2(7.0, 4.0))),
            |ui| self.toolbar(ui, host),
        );
        let timeline_h = (rect.height() * 0.28).clamp(150.0, 230.0);
        let middle = Rect::from_min_max(
            pos2(rect.left(), top.bottom() + 4.0),
            pos2(rect.right(), rect.bottom() - timeline_h - 6.0),
        );
        let left = Rect::from_min_size(middle.min, vec2(170.0, middle.height()));
        let right = Rect::from_min_max(pos2(middle.right() - 230.0, middle.top()), middle.max);
        let canvas = Rect::from_min_max(
            pos2(left.right() + 5.0, middle.top()),
            pos2(right.left() - 5.0, middle.bottom()),
        );
        ui.painter()
            .rect_filled(left, 8.0, Color32::from_rgb(31, 37, 48));
        ui.painter()
            .rect_filled(right, 8.0, Color32::from_rgb(31, 37, 48));
        ui.scope_builder(UiBuilder::new().max_rect(left.shrink(8.0)), |ui| {
            self.layers(ui)
        });
        ui.scope_builder(UiBuilder::new().max_rect(canvas), |ui| self.canvas(ui));
        ui.scope_builder(UiBuilder::new().max_rect(right.shrink(8.0)), |ui| {
            self.inspector(ui)
        });
        let timeline = Rect::from_min_max(pos2(rect.left(), middle.bottom() + 6.0), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(timeline), |ui| self.timeline(ui));
        self.projects_window(ui);
        self.export_window(ui, host);
        if !self.suppress_history && self.scene != before {
            let dragging = ui.input(|i| i.pointer.any_down());
            if !self.recording_drag || !dragging {
                self.undo.push(before);
                if self.undo.len() > 100 {
                    self.undo.remove(0);
                }
                self.redo.clear();
            }
            self.recording_drag = dragging;
            self.edited();
        } else if !ui.input(|i| i.pointer.any_down()) {
            self.recording_drag = false;
        }
        self.suppress_history = false;
    }

    fn receive(&mut self, files: Vec<SharedFile>, host: &mut dyn Host) -> Receipt {
        let mut receipt = Receipt::default();
        for file in files {
            let lower = file.name.to_lowercase();
            if lower.ends_with(".json") || lower.ends_with(".forma") {
                let read = if lower.ends_with(".forma") { archive::unpack(&file.path, &self.place) } else { storage::load(&file.path) };
                match read {
                    // Like Electron Forma, an opened file becomes a project of its own: saving it never replaces another.
                    Ok(mut scene) => {
                        scene.project_id = new_id();
                        scene.project_name = match scene.project_name.trim() {
                            "" => file.name.rsplit_once('.').map_or(file.name.as_str(), |(stem, _)| stem).to_owned(),
                            name => name.to_owned(),
                        };
                        self.open(scene);
                        self.edited();
                        receipt.accepted += 1;
                    }
                    Err(error) => {
                        receipt.note = Some(format!("{}: {error}", file.name));
                        receipt.skipped.push(file.name);
                    }
                }
            } else {
                // A video also keeps its poster and its length, as Electron Forma's video layers do.
                let media = if lower.ends_with(".svg") {
                    assets::import_svg(&file.path).map(|(source, w, h)| ("svg", "svgSource", source, w, h, None))
                } else if [".mp4", ".webm", ".ogv"].iter().any(|ext| lower.ends_with(ext)) {
                    assets::import_video(&file.path, &self.place).map(|(url, poster, info)| {
                        ("video", "assetData", url, info.width as f32, info.height as f32, Some((poster, info.duration)))
                    })
                } else {
                    assets::import_picture(&file.path, &self.place)
                        .map(|(url, w, h)| ("image", "assetData", url, w as f32, h as f32, None))
                };
                match media {
                    Ok((kind, field, value, width, height, video)) => {
                        let id = self.add_media(&file.name, kind, field, value, width, height);
                        if let Some((poster, duration)) = video {
                            let shape = self.scene.shapes.last_mut().unwrap();
                            shape.extra.insert("posterData".into(), poster.into());
                            shape.extra.insert("mediaDuration".into(), duration.into());
                        }
                        self.selected = Some(id);
                        self.edited();
                        receipt.accepted += 1;
                    }
                    Err(_) => receipt.skipped.push(file.name),
                }
            }
        }
        if receipt.accepted > 0 {
            host.toast(format!("Forma took {} file(s)", receipt.accepted));
        }
        receipt
    }

    fn open_document(&mut self, id: &str, host: &mut dyn Host) {
        match storage::load_document(&self.place, id) {
            Ok(scene) => self.open(scene),
            Err(error) => host.toast(error),
        }
    }

    fn save(&mut self) -> Result<(), String> {
        self.edited_at = None;
        storage::save(&self.place, &self.scene)
    }

    fn title(&self) -> String {
        self.scene.project_name.clone()
    }
}

fn hex(value: &str, alpha: u8) -> Color32 {
    let bytes = value.as_bytes();
    if bytes.len() == 7 && bytes[0] == b'#' {
        let channel = |i| u8::from_str_radix(&value[i..i + 2], 16).unwrap_or(0);
        Color32::from_rgba_unmultiplied(channel(1), channel(3), channel(5), alpha)
    } else {
        Color32::from_rgba_unmultiplied(170, 160, 206, alpha)
    }
}
