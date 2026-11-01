//! Native Forma scene editor: shape layers, keyframes, timeline and the same project folder as Electron Forma.

use crate::animated_svg;
use crate::archive;
use crate::assets;
use crate::model::{self, HANDLES, Pose, RotationClip, Scene, Shape, resized_pose};
use crate::motion;
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

/// A timeline row (see `Forma::rows`).
enum Row {
    Layer { name: String, clips: Vec<usize> },
    Fade(usize),
    Motion(usize, usize),
    Spin(usize, usize),
}

/// What a timeline drag holds: a clip (moving it or one of its edges) or a spin (moving it or its end).
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Clip,
    Start,
    End,
    Spin(usize),
    SpinLength(usize),
}

/// A timeline drag, with the layer and scene length as they were when it began.
#[derive(Clone)]
struct TimelineDrag {
    index: usize,
    part: Part,
    origin: f32,
    shape: Shape,
    duration: f64,
}

/// Easing names as the editor shows them, and as projects store them.
const EASES: [(&str, &str); 6] = [("Smooth", "smooth"), ("Steady", "linear"), ("Hold", "hold"), ("Speed up", "ease-in"), ("Slow down", "ease-out"), ("Custom curve", "custom")];

/// Deletes a keyframe, and the drawn move it started (Electron's `removeKeyframe` and `detachMotionKey`).
fn remove_key(shape: &mut Shape, key: usize) {
    if shape.keys.len() <= 1 {
        return;
    }
    let removed = shape.keys.remove(key);
    let motion = removed.extra.get("motionId").and_then(|v| v.as_str()).map(str::to_owned)
        .or_else(|| model::motions(shape).find(|(_, start, _)| (start - removed.t).abs() < 0.011).and_then(|(m, ..)| m.get("id")?.as_str().map(str::to_owned)));
    if let Some(motion) = motion {
        if let Some(list) = shape.extra.get_mut("motions").and_then(|v| v.as_array_mut()) {
            list.retain(|m| m.get("id").and_then(|v| v.as_str()) != Some(motion.as_str()));
        }
        for key in &mut shape.keys {
            if key.extra.get("motionId").and_then(|v| v.as_str()) == Some(motion.as_str()) {
                key.extra.remove("motionId");
            }
        }
    }
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
    /// A drag on the timeline.
    timeline_drag: Option<TimelineDrag>,
    /// Drawing a move: the tool ("arrow" or "path"), its options, and what's drawn so far (from the layer's pose).
    motion_mode: Option<&'static str>,
    motion_options: motion::Options,
    motion_draft: Option<(Pose, Vec<motion::Point>)>,
    /// The move chosen in the inspector, and its fields being edited (start, length, options).
    selected_motion: Option<String>,
    motion_edit: (f64, f64, motion::Options),
    /// A message for the next toast.
    notice: Option<String>,
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
            timeline_drag: None,
            motion_mode: None,
            motion_options: motion::Options::default(),
            motion_draft: None,
            selected_motion: None,
            motion_edit: (0.0, 1.0, motion::Options::default()),
            notice: None,
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
        egui::ScrollArea::vertical().id_salt("forma-layers").show(ui, |ui| {
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
        let mut pose = self.scene.editable_pose(&self.scene.shapes[index], self.time);
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
        let mut posed = false;
        for (label, value, range) in [
            ("X", &mut pose.x, -4000.0..=4000.0),
            ("Y", &mut pose.y, -4000.0..=4000.0),
            ("Width", &mut pose.w, 1.0..=4000.0),
            ("Height", &mut pose.h, 1.0..=4000.0),
            ("Angle", &mut pose.r, -3600.0..=3600.0),
            ("Opacity", &mut pose.o, 0.0..=100.0),
        ] {
            posed |= ui
                .add(egui::Slider::new(value, range).text(label))
                .changed();
        }
        ui.separator();
        let added = ui.button("Add keyframe here").clicked();
        if posed || added {
            self.scene.set_pose(index, self.time, pose);
            changed = true;
        }
        let shape = &mut self.scene.shapes[index];
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
        self.motion_panel(ui, index);
    }

    /// Electron's drawn moves: draw an arrow or record a path on the canvas from the playhead, then retime, rescale
    /// or delete it.
    fn motion_panel(&mut self, ui: &mut Ui, index: usize) {
        ui.separator();
        ui.label(egui::RichText::new("Draw a move").font(font::bold(13.0)).color(color::INK));
        ui.horizontal(|ui| {
            for (label, kind) in [("Draw arrow", "arrow"), ("Record path", "path")] {
                if ui.selectable_label(self.motion_mode == Some(kind), label).clicked() {
                    self.motion_mode = if self.motion_mode == Some(kind) { None } else { Some(kind) };
                    self.motion_draft = None;
                    self.playing = false;
                }
            }
        });
        let help = match self.motion_mode {
            Some("arrow") => "Drag anywhere on the canvas to set the arrow destination.",
            Some(_) => "Hold and drag on the canvas. The selected shape will follow your mouse.",
            None => "Choose a tool, then drag anywhere on the canvas. The motion begins at the playhead.",
        };
        ui.label(egui::RichText::new(help).font(font::ui(11.0)).color(color::INK_MUTED));
        let options = &mut self.motion_options;
        egui::Grid::new("forma-motion-options").num_columns(2).show(ui, |ui| {
            ui.label("Seconds");
            ui.add(egui::DragValue::new(&mut options.seconds).range(0.1..=120.0).speed(0.1).fixed_decimals(1));
            ui.end_row();
            ui.label("Distance %");
            ui.add(egui::DragValue::new(&mut options.distance).range(0.0..=500.0).speed(5.0));
            ui.end_row();
            ui.label("Spin °");
            ui.add(egui::DragValue::new(&mut options.spin).range(-10000.0..=10000.0).speed(5.0));
            ui.end_row();
        });
        ui.checkbox(&mut options.face, "Turn to face where it's going");
        let list = motion::motions(&self.scene.shapes[index]);
        for (i, m) in list.iter().enumerate() {
            let name = format!("{} {} · {:.1}–{:.1} s · {}%", if m.kind == "path" { "Mouse path" } else { "Arrow move" }, i + 1, m.start_time, m.start_time + m.duration, m.distance);
            if ui.selectable_label(self.selected_motion.as_deref() == Some(&m.id), name).clicked() {
                self.selected_motion = Some(m.id.clone());
                self.motion_edit = (m.start_time, m.duration, motion::Options { seconds: m.duration, distance: m.distance, spin: m.spin, face: m.face });
                (self.time, self.playing) = (m.start_time, false);
            }
        }
        let Some(id) = self.selected_motion.clone().filter(|id| list.iter().any(|m| &m.id == id)) else { return };
        let (start, length, options) = &mut self.motion_edit;
        egui::Grid::new("forma-motion-edit").num_columns(2).show(ui, |ui| {
            ui.label("Starts at (s)");
            ui.add(egui::DragValue::new(start).range(0.0..=3600.0).speed(0.01).fixed_decimals(2));
            ui.end_row();
            ui.label("Length (s)");
            ui.add(egui::DragValue::new(length).range(0.1..=3600.0).speed(0.01).fixed_decimals(2));
            ui.end_row();
            ui.label("Distance %");
            ui.add(egui::DragValue::new(&mut options.distance).range(0.0..=500.0).speed(5.0));
            ui.end_row();
            ui.label("Spin °");
            ui.add(egui::DragValue::new(&mut options.spin).range(-10000.0..=10000.0).speed(5.0));
            ui.end_row();
        });
        ui.checkbox(&mut options.face, "Turn to face where it's going");
        let (start, length, options) = self.motion_edit;
        ui.horizontal(|ui| {
            if ui.button("Update").clicked() {
                match motion::edit(&mut self.scene, index, &id, start, length, options) {
                    Ok(()) => {
                        (self.time, self.playing) = (start, false);
                        self.edited();
                    }
                    Err(error) => self.notice = Some(error),
                }
            }
            if ui.button("Delete").clicked() {
                motion::remove(&mut self.scene, index, &id);
                self.selected_motion = None;
                self.time = start;
                self.edited();
            }
        });
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
        let to_screen = |p: motion::Point| pos2(canvas.left() + p.x * scale, canvas.top() + p.y * scale);
        // The selected move's path, and one being drawn.
        if let Some(m) = self.selected.as_deref().and_then(|id| self.scene.shapes.iter().find(|s| s.id == id))
            .and_then(|s| motion::motions(s).into_iter().find(|m| Some(&m.id) == self.selected_motion.as_ref()))
        {
            let points: Vec<Pos2> = motion::scaled_points(&m).into_iter().map(to_screen).collect();
            painter.add(egui::Shape::line(points, Stroke::new(2.0, color::SUN.gamma_multiply(0.7))));
        }
        if let Some((start, points)) = &self.motion_draft {
            let mut line = vec![to_screen(motion::Point { x: start.x, y: start.y })];
            line.extend(points.iter().map(|p| to_screen(*p)));
            if self.motion_mode == Some("arrow") {
                line = vec![line[0], *line.last().unwrap()];
            }
            painter.add(egui::Shape::line(line.clone(), Stroke::new(2.5, color::SUN)));
            if let [.., a, b] = line[..] {
                let back = (a - b).normalized() * 12.0;
                let side = vec2(-back.y, back.x) * 0.5;
                painter.add(egui::Shape::convex_polygon(vec![b, b + back + side, b + back - side], color::SUN, Stroke::NONE));
            }
        }
        if let Some(kind) = self.motion_mode {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            let to_scene = |p: Pos2| motion::Point { x: (p.x - canvas.left()) / scale, y: (p.y - canvas.top()) / scale };
            let index = self.selected.as_deref().and_then(|id| self.scene.shapes.iter().position(|s| s.id == id));
            if let (true, Some(index), Some(p)) = (response.drag_started(), index, ui.input(|i| i.pointer.press_origin())) {
                self.motion_draft = Some((self.scene.editable_pose(&self.scene.shapes[index], self.time), vec![to_scene(p)]));
            }
            if let (true, Some((_, points)), Some(p)) = (response.dragged(), self.motion_draft.as_mut(), response.interact_pointer_pos()) {
                let p = to_scene(p);
                let last = *points.last().unwrap();
                if kind == "arrow" {
                    *points.last_mut().unwrap() = p;
                } else if (p.x - last.x).hypot(p.y - last.y) > 3.0 {
                    points.push(p);
                }
            }
            if response.drag_stopped() {
                if let (Some((start, mut points)), Some(index)) = (self.motion_draft.take(), index) {
                    if kind == "path" {
                        points.insert(0, motion::Point { x: start.x, y: start.y });
                    }
                    match motion::draw(&mut self.scene, index, kind, start, &points, self.time, self.motion_options, new_id()) {
                        Ok(id) => {
                            let m = motion::motions(&self.scene.shapes[index]).into_iter().find(|m| m.id == id).unwrap();
                            self.motion_edit = (m.start_time, m.duration, motion::Options { seconds: m.duration, ..self.motion_options });
                            self.selected_motion = Some(id);
                            self.time = m.start_time + m.duration;
                            self.edited();
                        }
                        Err(error) => self.notice = Some(error),
                    }
                    self.motion_mode = None;
                }
            }
            return;
        }
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

    /// Electron Forma's timeline rows: one per layer (clips sharing a track side by side), each followed by its
    /// fade-out, drawn moves and spins. On a track of clips only the selected clip shows those.
    fn rows(&self) -> Vec<Row> {
        let mut groups: Vec<(String, Vec<Row>)> = Vec::new();
        for (index, shape) in self.scene.shapes.iter().enumerate() {
            let track = shape.extra.get("trackId").and_then(|v| v.as_str()).filter(|t| !t.is_empty());
            let key = track.unwrap_or(&shape.id).to_owned();
            let group = match groups.iter().position(|(k, _)| *k == key) {
                Some(group) => group,
                None => {
                    let name = if track.is_some() { shape.name.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end().to_owned() } else { shape.name.clone() };
                    groups.push((key, vec![Row::Layer { name, clips: Vec::new() }]));
                    groups.len() - 1
                }
            };
            let rows = &mut groups[group].1;
            if let Row::Layer { clips, .. } = &mut rows[0] {
                clips.push(index);
            }
            if track.is_some() && self.selected.as_deref() != Some(&shape.id) {
                continue;
            }
            if shape.disappear.is_some() {
                rows.push(Row::Fade(index));
            }
            rows.extend((0..model::motions(shape).count()).map(|m| Row::Motion(index, m)));
            rows.extend((0..shape.rotation_clips.len()).map(|r| Row::Spin(index, r)));
        }
        groups.into_iter().flat_map(|(_, rows)| rows).collect()
    }

    fn timeline(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 8.0, Color32::from_rgb(31, 37, 48));
        let left = rect.left() + 130.0;
        let width = (rect.width() - 150.0).max(1.0);
        let duration = self.scene.duration;
        let x = |t: f64| left + width * (t / duration).clamp(0.0, 1.0) as f32;
        for tick in 0..=duration.ceil() as usize {
            let pos = x(tick as f64);
            ui.painter().line_segment([pos2(pos, rect.top() + 6.0), pos2(pos, rect.bottom())], Stroke::new(0.5, color::HAIRLINE));
            ui.painter().text(pos2(pos + 3.0, rect.top() + 4.0), Align2::LEFT_TOP, format!("{tick}s"), font::ui(10.0), color::INK_MUTED);
        }
        let ruler = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 25.0));
        let response = ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
        if let (true, Some(p)) = (response.clicked() || response.dragged(), response.interact_pointer_pos()) {
            self.time = (((p.x - left) / width) as f64 * duration).clamp(0.0, duration);
            self.playing = false;
        }
        let rows = self.rows();
        let area = Rect::from_min_max(pos2(rect.left(), rect.top() + 27.0), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            egui::ScrollArea::vertical().id_salt("forma-timeline").auto_shrink(false).show(ui, |ui| {
                for row in &rows {
                    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
                    self.timeline_row(ui, row, line, &x, width);
                }
            });
        });
        let play_x = x(self.time);
        ui.painter().line_segment([pos2(play_x, rect.top()), pos2(play_x, rect.bottom())], Stroke::new(2.0, color::SUN));
        if ui.input(|i| !i.pointer.any_down()) && self.timeline_drag.take().is_some() {
            // A clip that moved away from the playhead brings the playhead along, as in Electron.
            if let Some(shape) = self.selected.as_deref().and_then(|id| self.scene.shapes.iter().find(|s| s.id == id)) {
                if !shape.visible(self.time, self.scene.duration) {
                    self.time = shape.clip_start();
                }
            }
        }
    }

    fn timeline_row(&mut self, ui: &mut Ui, row: &Row, line: Rect, x: &dyn Fn(f64) -> f32, width: f32) {
        let bar = |from: f64, to: f64| Rect::from_min_max(pos2(x(from), line.top() + 3.0), pos2(x(to).max(x(from) + 3.0), line.bottom() - 3.0));
        let label = |ui: &Ui, text: &str, swatch: Option<Color32>, sub: bool| {
            let mut at = line.left() + if sub { 18.0 } else { 6.0 };
            if let Some(fill) = swatch {
                ui.painter().rect_filled(Rect::from_center_size(pos2(at + 5.0, line.center().y), vec2(10.0, 10.0)), 2.0, fill);
                at += 15.0;
            }
            ui.painter().text(pos2(at, line.center().y), Align2::LEFT_CENTER, text, font::ui(11.0), if sub { color::INK_MUTED } else { color::INK });
        };
        ui.painter().rect_filled(Rect::from_min_max(pos2(x(0.0), line.top() + 3.0), pos2(x(self.scene.duration), line.bottom() - 3.0)), 4.0, Color32::from_rgb(38, 45, 58));
        match row {
            Row::Layer { name, clips } => {
                let first = &self.scene.shapes[clips[0]];
                label(ui, name, Some(hex(&first.color, 255)), false);
                let head = ui.interact(Rect::from_min_max(line.min, pos2(x(0.0) - 4.0, line.max.y)), ui.scope_id().with(("track", &first.id)), Sense::click());
                if head.clicked() {
                    let pick = clips.iter().find(|&&i| self.selected.as_deref() == Some(&self.scene.shapes[i].id))
                        .or_else(|| clips.iter().find(|&&i| self.scene.shapes[i].visible(self.time, self.scene.duration)))
                        .unwrap_or(&clips[0]);
                    self.select_clip(*pick);
                }
                for &index in clips {
                    self.clip_bar(ui, index, line, x, width);
                }
            }
            Row::Fade(index) => {
                let shape = &self.scene.shapes[*index];
                label(ui, "Fade out", None, true);
                let d = shape.disappear.as_ref().unwrap();
                let end = self.scene.reappear_at(shape).min(self.scene.duration);
                let span = bar(d.start_time, end.max(d.start_time + self.scene.duration * 0.015));
                ui.painter().rect_filled(span, 4.0, Color32::from_rgb(92, 84, 120));
                ui.painter().text(span.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, "Hidden", font::ui(10.0), color::INK);
                let tip = format!("Fade out {:.2}–{:.2} s{}", d.start_time, d.start_time + d.duration, if end.is_finite() && end < self.scene.duration { format!("; reappear {end:.2} s") } else { "; stay hidden".into() });
                if ui.interact(span, ui.scope_id().with(("fade", &shape.id)), Sense::click()).on_hover_text(tip).clicked() {
                    (self.selected, self.time, self.playing) = (Some(shape.id.clone()), d.start_time, false);
                }
            }
            Row::Motion(index, m) => {
                let shape = &self.scene.shapes[*index];
                let Some((motion, start, length)) = model::motions(shape).nth(*m) else { return };
                let path = motion.get("type").and_then(|t| t.as_str()) == Some("path");
                label(ui, &format!("{} {}", if path { "Path" } else { "Move" }, m + 1), None, true);
                let span = bar(start, start + length);
                ui.painter().rect_filled(span, 4.0, Color32::from_rgb(72, 110, 140));
                ui.painter().text(span.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, if path { "Mouse path" } else { "Arrow move" }, font::ui(10.0), color::INK);
                if ui.interact(span, ui.scope_id().with(("motion", &shape.id, *m)), Sense::click()).on_hover_text(format!("{}: {start:.2}–{:.2} s", shape.name, start + length)).clicked() {
                    (self.selected, self.time, self.playing) = (Some(shape.id.clone()), start, false);
                }
            }
            Row::Spin(index, r) => {
                let clip = &self.scene.shapes[*index].rotation_clips[*r];
                let id = self.scene.shapes[*index].id.clone();
                label(ui, &format!("Spin {}", r + 1), None, true);
                let span = bar(clip.start_time, clip.start_time + clip.duration);
                ui.painter().rect_filled(span, 4.0, Color32::from_rgb(120, 96, 64));
                ui.painter().text(span.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, format!("Spin {}°", clip.degrees), font::ui(10.0), color::INK);
                let tip = format!("Rotation {}° · {:.2}–{:.2} s. Drag to move, drag the end to change its length.", clip.degrees, clip.start_time, clip.start_time + clip.duration);
                let start = clip.start_time;
                let response = ui.interact(span, ui.scope_id().with(("spin", &id, *r)), Sense::click_and_drag()).on_hover_text(tip);
                self.timeline_gesture(ui, &response, *index, span, Part::Spin(*r), width);
                if response.clicked() {
                    (self.selected, self.time, self.playing) = (Some(id), start, false);
                }
            }
        }
    }

    /// A layer's clip bar, its keyframes and its color keyframes.
    fn clip_bar(&mut self, ui: &mut Ui, index: usize, line: Rect, x: &dyn Fn(f64) -> f32, width: f32) {
        let shape = &self.scene.shapes[index];
        let (start, end) = (shape.clip_start(), shape.out_time.unwrap_or(self.scene.duration).min(self.scene.duration));
        let span = Rect::from_min_max(pos2(x(start), line.top() + 3.0), pos2(x(end).max(x(start) + 3.0), line.bottom() - 3.0));
        let selected = self.selected.as_deref() == Some(&shape.id);
        ui.painter().rect_filled(span, 4.0, if selected { color::SUN } else { color::INK_SOFT });
        let dragging = self.timeline_drag.as_ref().is_some_and(|d| d.index == index);
        let track = shape.extra.get("trackId").and_then(|v| v.as_str()).is_some_and(|t| !t.is_empty());
        if dragging || track {
            let text = if dragging { format!("{start:.2}–{end:.2} s") } else { shape.name.clone() };
            ui.painter().text(span.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, text, font::ui(10.0), Color32::from_rgb(24, 29, 37));
        }
        let tip = format!("{}: visible {start:.2}–{end:.2} s. Drag to move, drag an edge to trim.", shape.name);
        let id = shape.id.clone();
        let response = ui.interact(span, ui.scope_id().with(("clip", &id)), Sense::click_and_drag()).on_hover_text(tip);
        if let Some(p) = response.hover_pos() {
            let edge = (p.x - span.left()).abs() <= 6.0 || (p.x - span.right()).abs() <= 6.0;
            ui.ctx().set_cursor_icon(if edge { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::Grab });
        }
        self.timeline_gesture(ui, &response, index, span, Part::Clip, width);
        if response.clicked() {
            self.select_clip(index);
        }
        // Keyframes inside the clip: click to go to one, right-click to move, ease or delete it.
        let shape = &self.scene.shapes[index];
        let keys: Vec<(usize, f64)> = shape.keys.iter().enumerate().map(|(i, k)| (i, k.t)).filter(|(_, t)| *t >= start - 1e-6 && *t <= end + 1e-6).collect();
        for (key, t) in keys {
            let at = pos2(x(t), line.center().y);
            let dot = [vec2(0.0, -5.0), vec2(5.0, 0.0), vec2(0.0, 5.0), vec2(-5.0, 0.0)].map(|v| at + v);
            ui.painter().add(egui::Shape::convex_polygon(dot.to_vec(), Color32::WHITE, Stroke::new(1.0, Color32::from_rgb(24, 29, 37))));
            let response = ui.interact(Rect::from_center_size(at, vec2(11.0, 13.0)), ui.scope_id().with(("key", &id, key)), Sense::click())
                .on_hover_text(format!("{} at {t:.2} seconds — right-click to move or delete", self.scene.shapes[index].name));
            if response.clicked() {
                (self.selected, self.time, self.playing) = (Some(id.clone()), t, false);
            }
            response.context_menu(|ui| self.key_menu(ui, index, key));
        }
        let colors: Vec<(usize, f64, String)> = self.scene.shapes[index].color_keys.iter().enumerate().map(|(i, k)| (i, k.t, k.color.clone()))
            .filter(|(_, t, _)| *t >= start - 1e-6 && *t <= end + 1e-6).collect();
        for (key, t, fill) in colors {
            let at = pos2(x(t), line.bottom() - 4.0);
            ui.painter().circle(at, 4.0, hex(&fill, 255), Stroke::new(1.0, Color32::WHITE));
            let response = ui.interact(Rect::from_center_size(at, vec2(10.0, 10.0)), ui.scope_id().with(("color", &id, key)), Sense::click())
                .on_hover_text(format!("Color {} at {t:.2} s — right-click to delete", fill.to_uppercase()));
            if response.clicked() {
                (self.selected, self.time, self.playing) = (Some(id.clone()), t, false);
            }
            response.context_menu(|ui| {
                if ui.button("Delete color keyframe").clicked() {
                    let shape = &mut self.scene.shapes[index];
                    let removed = shape.color_keys.remove(key);
                    // The last color keyframe gone: the layer keeps that color (Electron does the same).
                    if shape.color_keys.is_empty() {
                        shape.color = removed.color;
                    }
                    ui.close();
                }
            });
        }
    }

    /// Selects a clip and brings the playhead into it (Electron's `selectClip`).
    fn select_clip(&mut self, index: usize) {
        let shape = &self.scene.shapes[index];
        self.selected = Some(shape.id.clone());
        self.playing = false;
        if !shape.visible(self.time, self.scene.duration) {
            self.time = shape.clip_start();
        }
    }

    /// Drags on the timeline, applied to the layer as it was when the drag began, so every frame starts afresh.
    fn timeline_gesture(&mut self, ui: &Ui, response: &egui::Response, index: usize, span: Rect, part: Part, width: f32) {
        if response.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(span.center());
            let part = match part {
                Part::Clip if (origin.x - span.left()).abs() <= 6.0 => Part::Start,
                Part::Clip if (origin.x - span.right()).abs() <= 6.0 => Part::End,
                Part::Spin(r) if (origin.x - span.right()).abs() <= 8.0 => Part::SpinLength(r),
                part => part,
            };
            self.timeline_drag = Some(TimelineDrag { index, part, origin: origin.x, shape: self.scene.shapes[index].clone(), duration: self.scene.duration });
            self.selected = Some(self.scene.shapes[index].id.clone());
            self.playing = false;
        }
        let Some(drag) = self.timeline_drag.clone().filter(|d| d.index == index && response.dragged()) else { return };
        let Some(pointer) = response.interact_pointer_pos() else { return };
        let duration = drag.duration;
        let delta = ((pointer.x - drag.origin) / width) as f64 * duration;
        // Snap to the playhead, the scene's ends and other clips' edges within 8 px, else to tenths; Alt is free.
        let mut targets = vec![0.0, self.time, duration];
        for (_, other) in self.scene.shapes.iter().enumerate().filter(|(i, _)| *i != index) {
            targets.push(other.clip_start());
            targets.extend(other.out_time);
        }
        let tolerance = 8.0 / width as f64 * duration;
        let free = ui.input(|i| i.modifiers.alt);
        let snap = |t: f64| -> (f64, bool) {
            if free {
                return (model::hundredths(t), false);
            }
            let nearest = targets.iter().copied().fold(f64::INFINITY, |best, x| if (x - t).abs() < (best - t).abs() { x } else { best });
            if (nearest - t).abs() <= tolerance { (nearest, true) } else { ((t * 10.0).round() / 10.0, false) }
        };
        self.scene.shapes[index] = drag.shape.clone();
        self.scene.duration = duration;
        let (start0, end0) = (drag.shape.clip_start(), drag.shape.out_time.unwrap_or(duration));
        match drag.part {
            Part::Clip => {
                let (a, b) = (snap(start0 + delta), snap(end0 + delta));
                let use_end = b.1 && (!a.1 || (b.0 - end0 - delta).abs() < (a.0 - start0 - delta).abs());
                self.scene.shift_layer(index, if use_end { b.0 - end0 } else { a.0 - start0 });
            }
            Part::Start => {
                let start = snap(start0 + delta).0.min(end0 - model::MIN_CLIP).max(0.0);
                let _ = self.scene.trim_clip(index, start, drag.shape.out_time.map(|_| end0));
            }
            Part::End => {
                let end = snap(end0 + delta).0.max(start0 + model::MIN_CLIP);
                let _ = self.scene.trim_clip(index, start0, Some(end));
            }
            Part::Spin(r) | Part::SpinLength(r) => {
                let clip = &drag.shape.rotation_clips[r];
                let target = &mut self.scene.shapes[index].rotation_clips[r];
                if matches!(drag.part, Part::SpinLength(_)) {
                    target.duration = model::hundredths(clip.duration + delta).clamp(0.1, (duration - clip.start_time).max(0.1));
                } else {
                    target.start_time = model::hundredths(clip.start_time + delta).clamp(0.0, (duration - clip.duration).max(0.0));
                }
            }
        }
        self.edited();
    }

    /// A keyframe's menu: when it is, how the stretch after it eases, and deleting it (Electron's keyframe menu).
    fn key_menu(&mut self, ui: &mut Ui, index: usize, key: usize) {
        let Some(k) = self.scene.shapes[index].keys.get(key) else { return };
        let mut t = k.t;
        ui.horizontal(|ui| {
            ui.label("At");
            if ui.add(egui::DragValue::new(&mut t).range(0.0..=3600.0).speed(0.01).fixed_decimals(2).suffix(" s")).changed() {
                let t = model::hundredths(t);
                let shape = &mut self.scene.shapes[index];
                // Moving onto another keyframe would make two at one moment: keep this one's place instead.
                if !shape.keys.iter().enumerate().any(|(i, other)| i != key && (other.t - t).abs() < 0.011) {
                    shape.keys[key].t = t;
                    shape.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
                    self.scene.duration = self.scene.duration.max(t);
                    ui.close();
                }
            }
        });
        let shape = &mut self.scene.shapes[index];
        let k = &mut shape.keys[key];
        let mut ease = k.ease.clone().unwrap_or_default();
        egui::ComboBox::from_label("Easing after it")
            .selected_text(match ease.as_str() { "" => "Layer's easing", e => EASES.iter().find(|(_, v)| *v == e).map_or(e, |(name, _)| name) })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut ease, String::new(), "Layer's easing");
                for (name, value) in EASES {
                    ui.selectable_value(&mut ease, value.to_owned(), name);
                }
            });
        if ease != k.ease.clone().unwrap_or_default() {
            k.ease = (!ease.is_empty()).then_some(ease.clone());
            if ease == "custom" && k.curve.is_none() {
                k.curve = Some([0.42, 0.0, 0.58, 1.0]);
            }
        }
        if k.ease.as_deref() == Some("custom") {
            let curve = k.curve.get_or_insert([0.42, 0.0, 0.58, 1.0]);
            ui.horizontal(|ui| {
                ui.label("Curve");
                for (i, value) in curve.iter_mut().enumerate() {
                    // Electron's validCurve: the x handles stay between 0 and 1, the y handles within ±5.
                    let range = if i % 2 == 0 { 0.0..=1.0 } else { -5.0..=5.0 };
                    ui.add(egui::DragValue::new(value).range(range).speed(0.01).fixed_decimals(2));
                }
            });
        }
        let only = shape.keys.len() <= 1;
        if ui.add_enabled(!only, egui::Button::new("Delete keyframe")).on_disabled_hover_text("A layer keeps at least one keyframe").clicked() {
            remove_key(shape, key);
            ui.close();
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
            egui::ScrollArea::vertical().id_salt("forma-inspector").show(ui, |ui| self.inspector(ui));
        });
        if let Some(notice) = self.notice.take() {
            host.toast(notice);
        }
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
