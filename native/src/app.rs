//! Native Forma scene editor: shape layers, keyframes, timeline and the same project folder as Electron Forma.

use crate::assets;
use crate::model::{Pose, RotationClip, Scene};
use crate::render;
use crate::storage;
use crate::video;
use canopy_ui::{CanopyApp, Host, Place, Receipt, SharedFile, color, font};
use egui::{
    Align2, Color32, Rect, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Ui, UiBuilder, pos2, vec2,
};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    /// Draws the preview the way exports draw frames. Made again when the scene's pictures change.
    renderer: Option<render::Renderer>,
    /// The preview picture, and what it shows: the scene, the time, and its size in pixels.
    preview: Option<(TextureHandle, Scene, f64, [u32; 2])>,
    undo: Vec<Scene>,
    redo: Vec<Scene>,
    recording_drag: bool,
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
            renderer: None,
            preview: None,
            undo: Vec::new(),
            redo: Vec::new(),
            recording_drag: false,
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
                .button("PNG")
                .on_hover_text("Export this frame into Files")
                .clicked()
            {
                let _ = std::fs::create_dir_all(&self.place.exports_dir);
                let name = canopy_ui::canopy_core::files::unique_name(
                    &self.place.exports_dir,
                    &format!("{}.png", self.scene.project_name),
                );
                let path = self.place.exports_dir.join(name);
                match render::save_png(&self.scene, &self.place, self.time, &path) {
                    Ok(()) => host.exported(path),
                    Err(error) => host.toast(error),
                }
            }
            if ui
                .add_enabled(self.export.is_none(), egui::Button::new("MP4"))
                .on_hover_text("Export a silent video into Files")
                .clicked()
            {
                if let Err(error) = std::fs::create_dir_all(&self.place.exports_dir) {
                    host.toast(error.to_string());
                } else {
                    let name = canopy_ui::canopy_core::files::unique_name(
                        &self.place.exports_dir,
                        &format!("{}.mp4", self.scene.project_name),
                    );
                    let path = self.place.exports_dir.join(name);
                    match video::start(&self.scene, &self.place, path, ui.ctx().clone()) {
                        Ok(export) => self.export = Some(export),
                        Err(error) => host.toast(error),
                    }
                }
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
        let stale = self.preview.as_ref().is_none_or(|(_, scene, time, drawn)| {
            *drawn != size || *time != self.time || *scene != self.scene
        });
        if stale {
            if let Some(pixmap) = renderer.preview(&self.scene, self.time, size[0] as f32 / self.scene.width) {
                let image = egui::ColorImage::from_rgba_premultiplied(
                    [pixmap.width() as usize, pixmap.height() as usize],
                    pixmap.data(),
                );
                match &mut self.preview {
                    Some((texture, scene, time, drawn)) => {
                        texture.set(image, TextureOptions::LINEAR);
                        (*scene, *time, *drawn) = (self.scene.clone(), self.time, size);
                    }
                    None => {
                        let texture = ui.ctx().load_texture("forma-preview", image, TextureOptions::LINEAR);
                        self.preview = Some((texture, self.scene.clone(), self.time, size));
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
        let mut hits = Vec::new();
        for shape in &self.scene.shapes {
            if !shape.visible(self.time, self.scene.duration) {
                continue;
            }
            let pose = self.scene.render_pose(shape, self.time);
            let bounds = Rect::from_center_size(
                pos2(canvas.left() + pose.x * scale, canvas.top() + pose.y * scale),
                vec2(pose.w * scale, pose.h * scale),
            );
            if renderer.leaves_out(shape) {
                painter.rect_filled(bounds, 3.0, hex(&shape.color_at(self.time), (pose.o * 2.55).round() as u8));
                painter.text(
                    bounds.center(),
                    Align2::CENTER_CENTER,
                    format!("{} layer", shape.kind),
                    font::ui(12.0),
                    Color32::BLACK,
                );
            }
            if self.selected.as_deref() == Some(&shape.id) {
                painter.rect_stroke(
                    bounds.expand(4.0),
                    3.0,
                    Stroke::new(1.5, color::SUN),
                    StrokeKind::Outside,
                );
            }
            hits.push((shape.id.clone(), bounds));
        }
        let response = ui.interact(
            canvas,
            ui.scope_id().with("forma-canvas"),
            Sense::click_and_drag(),
        );
        if response.clicked() || response.drag_started() {
            let pointer = response.interact_pointer_pos();
            self.selected = pointer.and_then(|p| {
                hits.iter()
                    .rev()
                    .find(|(_, rect)| rect.contains(p))
                    .map(|(id, _)| id.clone())
            });
        }
        if response.dragged() {
            let delta = ui.input(|i| i.pointer.delta()) / scale;
            if delta != egui::Vec2::ZERO {
                if let Some(shape) = self
                    .scene
                    .shapes
                    .iter_mut()
                    .find(|s| Some(s.id.as_str()) == self.selected.as_deref())
                {
                    let mut pose = shape.pose(self.time);
                    pose.x += delta.x;
                    pose.y += delta.y;
                    shape.edit_pose(self.time, pose);
                    self.edited();
                }
            }
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
            if file.name.to_lowercase().ends_with(".json") {
                match storage::load(&file.path) {
                    Ok(scene) => {
                        self.open(scene);
                        self.edited();
                        receipt.accepted += 1;
                    }
                    Err(_) => receipt.skipped.push(file.name),
                }
            } else {
                let svg = file.name.to_lowercase().ends_with(".svg");
                let media = if svg {
                    assets::import_svg(&file.path).map(|(source, w, h)| ("svg", "svgSource", source, w, h))
                } else {
                    assets::import_picture(&file.path, &self.place)
                        .map(|(url, w, h)| ("image", "assetData", url, w as f32, h as f32))
                };
                match media {
                    Ok((kind, field, value, width, height)) => {
                        let id = self.add_media(&file.name, kind, field, value, width, height);
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
