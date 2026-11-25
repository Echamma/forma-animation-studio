//! Native Forma scene editor: shape layers, keyframes, timeline and the same project folder as Electron Forma.

use crate::animated_svg;
use crate::archive;
use crate::assets;
use crate::compositing::{MaskProperty, MatteLinks};
use crate::key_editor::{KeyEditor, LABEL_WIDTH, TIME_MARGIN, TimeView};
use crate::keyframes::{Lane, Property};
use crate::mask_editor::MaskEditor;
use crate::shape_panel::{self, Tool as VectorTool, VectorTools};
use crate::shape_style::{PaintMode, PaintStyle, StrokeStyle, VectorLayer, VectorProperty, VectorValue};
use crate::model::{self, HANDLES, Pose, RotationClip, Scene, Shape};
use crate::motion;
use crate::render;
use crate::sound::Sound;
use crate::storage;
use crate::svg;
use crate::text_panel;
use crate::transform::{self, Anchor, LayerFrame};
use crate::video;
use crate::videos::Videos;
use canopy_ui::{
    Accent, CanopyApp, Host, Icon, Mood, Place, Receipt, SharedFile, color, font, icon_button,
    pane_frame, pill, primary_button, quiet_button, section_label,
};
use egui::{
    Align2, Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Ui, UiBuilder, Vec2,
    pos2, vec2,
};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "composition_panel.rs"]
mod composition_panel;

/// A drag on the canvas, with what it started from (scene pixels, editable poses).
#[derive(Clone)]
enum Gesture {
    Move {
        start: Pos2,
        poses: Vec<(String, Pose)>,
    },
    Resize {
        id: String,
        handle: (i8, i8),
        start: Pos2,
        pose: Pose,
        inverse: tiny_skia::Transform,
        anchor: Anchor,
        height_factor: f32,
    },
    Anchor {
        start: Pos2,
        initial: Anchor,
        inverse: tiny_skia::Transform,
        size: Vec2,
        shape: Box<Shape>,
    },
    Rotate {
        id: String,
        center: Pos2,
        inverse: tiny_skia::Transform,
        last: f32,
    },
}

/// The SVG editor's state: which layer, its markup being edited, the recolor color and a message.
struct SvgEdit {
    id: String,
    source: String,
    color: [u8; 3],
    selected_part: Option<usize>,
    status: String,
}

/// The inspector's tabs (Electron's Layer, Animate and Scene).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Layer,
    Animate,
    Scene,
}

/// What the drawer beside the tool rail shows (Electron's Add and Layers drawer).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drawer {
    Add,
    Layers,
}

/// Electron's clip menu and shortcuts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ClipAction {
    Next,
    PasteAfter,
    Paste,
    Split,
    Duplicate,
    Copy,
    Delete,
}

/// A timeline row (see `Forma::rows`).
enum Row {
    Layer { name: String, clips: Vec<usize> },
    Property(usize, Lane),
    Mask(usize, u64, usize),
    TextAnimator(usize, u64),
    Effects(usize),
    Effect(usize, u64),
    Fade(usize),
    Motion(usize, usize),
    Spin(usize, usize),
}

/// What a timeline drag holds: a clip, a drawn move, a spin or a fade (moving it, or one of its edges to change
/// how long it lasts), or a keyframe.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Clip,
    Start,
    End,
    Spin(usize),
    SpinStart(usize),
    SpinLength(usize),
    Motion(usize),
    MotionStart(usize),
    MotionEnd(usize),
    Fade,
    FadeEnd,
}

/// How close (px) to a bar's end a drag grabs the edge instead of the bar.
const EDGE: f32 = 6.0;

const ANCHOR_GLYPHS: [[&str; 3]; 3] = [
    ["\u{2196}", "\u{2191}", "\u{2197}"],
    ["\u{2190}", "\u{2022}", "\u{2192}"],
    ["\u{2199}", "\u{2193}", "\u{2198}"],
];
const DISCLOSURE_OPEN: &str = "\u{25be}";
const DISCLOSURE_CLOSED: &str = "\u{25b8}";

fn control_symbol(glyph: &str) -> egui::RichText {
    // egui ships these symbols in Hack, its monospace face.
    egui::RichText::new(glyph).monospace().size(14.0)
}

fn disclosure_symbol(open: bool) -> egui::RichText {
    control_symbol(if open { DISCLOSURE_OPEN } else { DISCLOSURE_CLOSED })
}

enum RailMark {
    Symbol(Icon),
    Pen,
    Mask,
    Move,
}

fn rail_button(
    ui: &mut Ui,
    mark: RailMark,
    label: &str,
    tip: &str,
    active: bool,
    enabled: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), 52.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, active, label));
    let painter = ui.painter();
    if active || response.hovered() && enabled {
        painter.rect_filled(
            rect,
            8.0,
            if active {
                color::SELECT_FILL
            } else {
                color::VEIL_HOVER
            },
        );
    }
    if active || response.has_focus() {
        let stroke = if response.has_focus() {
            color::LEAF
        } else {
            color::SELECT_LINE
        };
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(1.5, stroke),
            egui::StrokeKind::Inside,
        );
    }
    let ink = if active {
        color::SELECT_INK
    } else if enabled {
        color::INK
    } else {
        color::INK_MUTED
    };
    let center = rect.center_top() + vec2(0.0, 17.0);
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| center + vec2(x, y)).collect(),
            Stroke::new(1.5, ink),
        ));
    };
    match mark {
        RailMark::Symbol(icon) => canopy_ui::icon(
            painter,
            Rect::from_center_size(center, Vec2::splat(20.0)),
            icon,
            ink,
        ),
        RailMark::Pen => line(&[
            (-7.0, 7.0),
            (-5.0, 1.0),
            (4.0, -8.0),
            (8.0, -4.0),
            (-1.0, 5.0),
            (-7.0, 7.0),
        ]),
        RailMark::Mask => {
            painter.circle_stroke(center, 7.0, Stroke::new(1.5, ink));
            for offset in [
                vec2(7.0, 0.0),
                vec2(0.0, 7.0),
                vec2(-7.0, 0.0),
                vec2(0.0, -7.0),
            ] {
                painter.rect_filled(
                    Rect::from_center_size(center + offset, Vec2::splat(3.5)),
                    0.5,
                    ink,
                );
            }
        }
        RailMark::Move => {
            line(&[(-8.0, 5.0), (-2.0, -1.0), (2.0, 3.0), (8.0, -5.0)]);
            line(&[(3.0, -5.0), (8.0, -5.0), (8.0, 0.0)]);
        }
    }
    painter.text(
        rect.center_bottom() - vec2(0.0, 13.0),
        Align2::CENTER_CENTER,
        label,
        font::bold(12.0),
        ink,
    );
    response.on_hover_text(tip)
}

/// The pointer over a timeline bar: a resize arrow on an end that can be dragged, else a hand.
fn edge_cursor(ui: &Ui, response: &egui::Response, span: Rect, start: bool, end: bool) {
    if let Some(p) = response.hover_pos() {
        let edge = (start && (p.x - span.left()).abs() <= EDGE)
            || (end && (p.x - span.right()).abs() <= EDGE);
        ui.ctx().set_cursor_icon(if edge {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::Grab
        });
    }
}

/// A timeline drag, with the layer and scene length as they were when it began.
#[derive(Clone)]
struct TimelineDrag {
    part: Part,
    origin: f32,
    seconds_per_pixel: f64,
    shape: Shape,
    duration: f64,
}

/// Easing names as the editor shows them, and as projects store them.
const EASES: [(&str, &str); 6] = [
    ("Smooth", "smooth"),
    ("Steady", "linear"),
    ("Hold", "hold"),
    ("Speed up", "ease-in"),
    ("Slow down", "ease-out"),
    ("Custom curve", "custom"),
];

/// What Electron Forma's export dialog can make.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportKind {
    Video,
    Svg,
    Picture,
    Project,
    Lottie,
    StreamCuts,
}

#[derive(Clone, PartialEq)]
enum PreviewTarget {
    Motion(String, String),
    Spin(String, String),
    Fade(String),
}

fn fade_preview_range(scene: &Scene, shape: &Shape) -> Option<(f64, f64)> {
    let fade = shape.disappear.as_ref()?;
    let reappear = scene.reappear_at(shape);
    let end = if reappear.is_finite() {
        (fade.start_time + fade.duration).max(reappear) + 0.25
    } else {
        scene.duration
    };
    Some(((fade.start_time - 0.25).max(0.0), end.min(scene.duration)))
}

fn solid_fill_color(ui: &mut Ui, shape: &mut Shape, time: f64) -> bool {
    let current = shape.color_at(time);
    let mut rgb = crate::render::rgb(&current);
    let mut changed = ui.horizontal(|ui| {
        ui.label("Fill color");
        let changed = egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed();
        ui.label(egui::RichText::new(current.to_uppercase()).color(color::INK_MUTED));
        changed
    }).inner;
    if changed {
        let [r, g, b] = rgb;
        shape.key_color(time, format!("#{r:02x}{g:02x}{b:02x}"));
    }
    let mut animated = !shape.color_keys.is_empty();
    if ui.checkbox(&mut animated, "Animate color")
        .on_hover_text("Then move the playhead and pick another color")
        .changed()
    {
        if animated {
            shape.add_lane_key(Lane::Color, time);
        } else {
            shape.color = shape.color_at(time);
            shape.color_keys.clear();
        }
        changed = true;
    }
    changed
}

pub(crate) fn new_id() -> String {
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

type FrameKey = (f64, render::View, bool);
type CachedFrame = (std::sync::Arc<tiny_skia::Pixmap>, render::TextHits);

pub struct Forma {
    place: Place,
    scene: Scene,
    selected: Option<String>,
    multi_selected: Vec<String>,
    layer_drag: Option<String>,
    time: f64,
    playing: bool,
    loop_playback: bool,
    last_tick: Instant,
    edited_at: Option<Instant>,
    save_failed: bool,
    clip_edit_id: Option<String>,
    clip_shown: (f64, Option<f64>),
    clip_in_edit: String,
    clip_out_edit: String,
    show_projects: bool,
    show_compositions: bool,
    composition_path: Vec<(String, f64)>,
    pending_composition: Option<(String, f64)>,
    show_help: bool,
    show_export: bool,
    export_error: Option<String>,
    export_kind: ExportKind,
    video_settings: video::Settings,
    /// Draws the preview the way exports draw frames. Made again when the scene's pictures change.
    renderer: Option<render::Renderer>,
    camera_tool: bool,
    quality: crate::preview_cache::Quality,
    playing_at_half: bool,
    frame_time_ms: f64,
    frame_cache: crate::preview_cache::Cache<FrameKey, CachedFrame>,
    /// The preview picture, and what it shows: the scene, the time, its size in pixels, and which video frames.
    preview: Option<(TextureHandle, Scene, f64, (render::View, bool), u64)>,
    /// The canvas's zoom (1 fits the space, 0.2-5) and how far it's panned from the middle, in points.
    zoom: f32,
    pan: Vec2,
    lock_ratio: bool,
    /// The canvas bar's scene button asked for Scene settings.
    open_scene: bool,
    videos: Videos,
    sound: Sound,
    undo: Vec<Scene>,
    redo: Vec<Scene>,
    recording_drag: bool,
    /// A drag on the canvas: moving, resizing or turning a layer.
    gesture: Option<Gesture>,
    /// A drag on the timeline.
    timeline_drag: Option<TimelineDrag>,
    key_editor: KeyEditor,
    mask_editor: MaskEditor,
    vector_tools: VectorTools,
    vector_keyboard: bool,
    /// Drawing a move: the tool ("arrow" or "path"), its options, and what's drawn so far (from the layer's pose).
    motion_mode: Option<&'static str>,
    motion_options: motion::Options,
    motion_draft: Option<(Pose, Vec<motion::Point>)>,
    /// The move chosen in the inspector, and its fields being edited (start, length, options).
    selected_motion: Option<String>,
    /// The spin chosen in the inspector.
    selected_spin: Option<String>,
    motion_edit: (f64, f64, motion::Options),
    motion_shown: Option<(String, motion::Motion)>,
    live_animation_preview: bool,
    pending_auto_preview: Option<(Instant, PreviewTarget)>,
    /// A message for the next toast.
    notice: Option<String>,
    /// A layer copied with Ctrl+C.
    clipboard: Vec<Shape>,
    /// A clip menu choice (the layer's id and the action), run once the timeline has finished drawing: running it
    /// inside the menu deleted or added layers while the timeline still drew rows by their old positions, and
    /// deleting the last layer that way crashed Canopy.
    pending_clip: Option<(String, ClipAction)>,
    /// Forma's pane this frame: its dialogs dim and sit in it, not the whole Canopy window.
    pane: Rect,
    tab: Tab,
    /// The move and spin picked last frame, to bring up the Animate tab when another is picked.
    last_picked: (Option<String>, Option<String>),
    /// The drawer beside the tool rail, if open; `None` until the pane's width first decides it.
    drawer: Option<Drawer>,
    drawer_decided: bool,
    timeline_height: Option<f32>,
    timeline_at_drag_start: Option<f32>,
    /// Where playing a new animation's preview stops.
    preview_end: Option<f64>,
    /// Files picked with the Add button, taken in like dropped files on the next frame.
    pending_import: Vec<std::path::PathBuf>,
    /// Pictures picked as stop-motion poses, in order.
    pending_poses: Vec<std::path::PathBuf>,
    /// The start screen's canvas shape for a new project.
    start_portrait: bool,
    /// Whether editing a clip on a track shows the clip before it faintly (Electron's onion skin, on by default).
    onion_skin: bool,
    /// The SVG editor's markup for a layer, and its preview (by the markup's hash).
    svg_edit: Option<SvgEdit>,
    svg_preview: Option<(u64, TextureHandle)>,
    suppress_history: bool,
    export: Option<video::Export>,
    /// Forma MCP control (Electron's MCP button): the bridge while it's on, whether `--mcp` asked for it at start,
    /// and a video an assistant asked for, with where its answer goes once it's rendered.
    bridge: Option<crate::bridge::Bridge>,
    mcp_wanted: bool,
    mcp_video: Option<(
        crate::automation::PendingVideo,
        std::sync::mpsc::Sender<Result<serde_json::Value, crate::contract::ToolError>>,
    )>,
}

impl Forma {
    pub fn new(place: Place) -> Self {
        // The latest project opens again; a first launch (nothing to open) shows the start screen, as in Electron.
        let restored = storage::list(&place)
            .first()
            .and_then(|doc| storage::load_document(&place, &doc.id).ok());
        let first_launch = restored.is_none();
        let timeline_height = storage::timeline_height(&place);
        let scene = restored.unwrap_or_else(|| Scene::empty(new_id()));
        Self {
            place,
            scene,
            selected: None,
            multi_selected: Vec::new(),
            layer_drag: None,
            time: 0.0,
            playing: false,
            loop_playback: true,
            last_tick: Instant::now(),
            edited_at: None,
            save_failed: false,
            clip_edit_id: None,
            clip_shown: (0.0, None),
            clip_in_edit: String::new(),
            clip_out_edit: String::new(),
            show_projects: first_launch,
            show_compositions: false,
            composition_path: Vec::new(),
            pending_composition: None,
            show_help: false,
            show_export: false,
            export_error: None,
            export_kind: ExportKind::Video,
            video_settings: video::Settings::default(),
            renderer: None,
            camera_tool: false,
            quality: Default::default(),
            playing_at_half: false,
            frame_time_ms: 0.0,
            frame_cache: crate::preview_cache::Cache::new(crate::preview_cache::FRAME_BYTES),
            preview: None,
            zoom: 1.0,
            pan: Vec2::ZERO,
            lock_ratio: true,
            open_scene: false,
            videos: Videos::default(),
            sound: Sound::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            recording_drag: false,
            gesture: None,
            timeline_drag: None,
            key_editor: KeyEditor::default(),
            mask_editor: MaskEditor::default(),
            vector_tools: VectorTools::default(),
            vector_keyboard: false,
            motion_mode: None,
            motion_options: motion::Options::default(),
            motion_draft: None,
            selected_motion: None,
            selected_spin: None,
            motion_edit: (0.0, 1.0, motion::Options::default()),
            motion_shown: None,
            live_animation_preview: false,
            pending_auto_preview: None,
            notice: None,
            clipboard: Vec::new(),
            pending_clip: None,
            pane: Rect::NOTHING,
            tab: Tab::Layer,
            last_picked: (None, None),
            drawer: None,
            drawer_decided: false,
            timeline_height,
            timeline_at_drag_start: None,
            preview_end: None,
            pending_import: Vec::new(),
            pending_poses: Vec::new(),
            start_portrait: false,
            onion_skin: true,
            svg_edit: None,
            svg_preview: None,
            suppress_history: false,
            export: None,
            bridge: None,
            mcp_wanted: std::env::args().any(|arg| arg == "--mcp"),
            mcp_video: None,
        }
    }

    fn edited(&mut self) {
        self.frame_cache.invalidate();
        if let Some(renderer) = &self.renderer {
            renderer.invalidate_preview_pixels();
        }
        self.edited_at = Some(Instant::now());
        self.save_failed = false;
    }

    fn select_layer(&mut self, id: Option<String>, extend: bool) {
        self.key_editor.clear_selection();
        if !extend {
            self.multi_selected = id.clone().into_iter().collect();
            self.selected = id;
        } else if let Some(id) = id {
            if let Some(index) = self
                .multi_selected
                .iter()
                .position(|selected| selected == &id)
            {
                self.multi_selected.remove(index);
                if self.selected.as_deref() == Some(&id) {
                    self.selected = self.multi_selected.last().cloned();
                }
            } else {
                self.multi_selected.push(id.clone());
                self.selected = Some(id);
            }
        }
    }

    fn open(&mut self, scene: Scene) {
        self.frame_cache.invalidate();
        self.camera_tool = false;
        self.playing_at_half = false;
        self.sound.pause();
        let _ = self.save();
        self.scene = scene;
        self.composition_path.clear();
        self.pending_composition = None;
        self.show_compositions = false;
        self.key_editor = KeyEditor::default();
        self.mask_editor = MaskEditor::default();
        self.vector_tools = VectorTools::default();
        self.selected = None;
        self.multi_selected.clear();
        self.layer_drag = None;
        self.time = 0.0;
        self.playing = false;
        self.clip_edit_id = None;
        self.renderer = None;
        self.preview = None;
        (self.zoom, self.pan) = (1.0, Vec2::ZERO);
        self.videos = Videos::default();
        self.undo.clear();
        self.redo.clear();
        self.motion_shown = None;
        self.pending_auto_preview = None;
        self.suppress_history = true;
    }

    /// A new media layer in the middle of the scene, named and sized the way Electron Forma imports media.
    fn add_media(
        &mut self,
        file_name: &str,
        kind: &str,
        field: &str,
        value: String,
        width: f32,
        height: f32,
    ) -> String {
        let id = self.scene.add_shape(new_id(), kind);
        let (w, h) = self.scene.fitted_size(width, height);
        let shape = self.scene.shapes.last_mut().unwrap();
        let stem = file_name
            .rsplit_once('.')
            .map_or(file_name, |(stem, _)| stem);
        shape.name = if stem.is_empty() {
            "Media".into()
        } else {
            stem.chars().take(70).collect()
        };
        shape.color = "#888888".into();
        shape.extra.insert(field.into(), value.into());
        let pose = Pose {
            w,
            h,
            ..shape.pose(0.0)
        };
        shape.edit_pose(0.0, pose);
        id
    }

    /// Turns Forma MCP control on or off.
    fn set_mcp(&mut self, on: bool, ctx: &egui::Context) -> Result<(), String> {
        if !on {
            (self.bridge, self.mcp_video) = (None, None);
        } else if self.bridge.is_none() {
            let ctx = ctx.clone();
            self.bridge = Some(crate::bridge::Bridge::enable(move || {
                ctx.request_repaint()
            })?);
        }
        Ok(())
    }

    /// Where exports for assistants go: `FORMA_EXPORT_DIR` (as in Electron Forma), or the app's export folder.
    fn mcp_exports(&self) -> std::path::PathBuf {
        std::env::var_os("FORMA_EXPORT_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.place.exports_dir.clone())
    }

    /// Answers Forma MCP: one call a frame, inside the frame's undo step (so each edit can be undone and is saved), and
    /// a video it asked for once it's rendered (calls wait meanwhile).
    fn automation(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        use crate::automation::{Editor, Outcome, dispatch, snapshot};
        if std::mem::take(&mut self.mcp_wanted) {
            if let Err(error) = self.set_mcp(true, ctx) {
                host.toast(format!("MCP could not start: {error}"));
            }
        }
        if let Some((pending, _)) = &self.mcp_video {
            let rendered = match pending.export.result.try_recv() {
                Ok(rendered) => rendered,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Err("The video export stopped unexpectedly".into())
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                    return;
                }
            };
            let (pending, reply) = self.mcp_video.take().unwrap();
            let answer = pending.finish(rendered);
            if answer.is_ok() && pending.destination.starts_with(&self.place.exports_dir) {
                host.exported(pending.destination.clone());
            }
            let _ = reply.send(answer);
        }
        let Some(call) = self
            .bridge
            .as_ref()
            .and_then(|bridge| bridge.calls.try_recv().ok())
        else {
            return;
        };
        let (was_playing, exports) = (self.playing, self.mcp_exports());
        let mut editor = Editor {
            scene: &mut self.scene,
            selected: &mut self.selected,
            time: &mut self.time,
            playing: &mut self.playing,
            place: &self.place,
            exports: &exports,
        };
        let answer = match dispatch(&mut editor, &call.name, &call.args) {
            Ok(Outcome::Done(value)) => Ok(value),
            Ok(Outcome::Open(scene)) => {
                // A new or opened scene is a project of its own; the one that was open is saved as it was.
                self.open(scene);
                self.edited();
                let editor = Editor {
                    scene: &mut self.scene,
                    selected: &mut self.selected,
                    time: &mut self.time,
                    playing: &mut self.playing,
                    place: &self.place,
                    exports: &exports,
                };
                Ok(snapshot(&editor, false))
            }
            Ok(Outcome::Video(pending)) => {
                self.mcp_video = Some((pending, call.reply));
                ctx.request_repaint();
                return;
            }
            Err(error) => Err(error),
        };
        if self.playing && !was_playing {
            (self.last_tick, self.preview_end) = (Instant::now(), None);
        }
        self.multi_selected = self.selected.clone().into_iter().collect();
        if let Some(path) = answer
            .as_ref()
            .ok()
            .and_then(|a| a["path"].as_str())
            .map(std::path::PathBuf::from)
        {
            if call.name == "forma_export" && path.starts_with(&self.place.exports_dir) {
                host.exported(path);
            }
        }
        let _ = call.reply.send(answer);
        // Another call may be waiting.
        ctx.request_repaint();
    }

    fn undo(&mut self) {
        if let Some(scene) = self.undo.pop() {
            self.pending_auto_preview = None;
            let current = self.project_snapshot();
            self.redo.push(current);
            self.restore_snapshot(scene);
            self.key_editor.clear_selection();
            self.timeline_drag = None;
            self.gesture = None;
            self.mask_editor.cancel_gesture();
            self.vector_tools.set(None, None);
            self.suppress_history = true;
            self.playing = false;
            self.edited();
        }
    }

    fn redo(&mut self) {
        if let Some(scene) = self.redo.pop() {
            self.pending_auto_preview = None;
            let current = self.project_snapshot();
            self.undo.push(current);
            self.restore_snapshot(scene);
            self.key_editor.clear_selection();
            self.timeline_drag = None;
            self.gesture = None;
            self.mask_editor.cancel_gesture();
            self.vector_tools.set(None, None);
            self.suppress_history = true;
            self.playing = false;
            self.edited();
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        // Canopy's toolbar: the name, history and the project's buttons on the left; status, MCP and the one filled
        // button (Export) on the right. Playback sits on the timeline. In a narrow pane (a split) the buttons drop
        // their words and the status goes, so nothing overlaps.
        let compact = ui.available_width() < 780.0;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let name = ui.add(
                egui::TextEdit::singleline(&mut self.scene.project_name)
                    .desired_width(if compact { 120.0 } else { 170.0 })
                    .font(font::bold(14.0)),
            );
            if name.changed() {
                self.edited();
            }
            ui.add_enabled_ui(!self.undo.is_empty(), |ui| {
                if icon_button(ui, Icon::Undo, "Undo (Ctrl+Z)", false).clicked() {
                    self.undo();
                }
            });
            ui.add_enabled_ui(!self.redo.is_empty(), |ui| {
                if icon_button(ui, Icon::Redo, "Redo (Ctrl+Shift+Z or Ctrl+Y)", false).clicked() {
                    self.redo();
                }
            });
            let projects = if compact {
                icon_button(ui, Icon::Project, "Your projects, and a new one", false)
            } else {
                quiet_button(ui, "Projects", Some(Icon::Project))
                    .on_hover_text("Your projects, and a new one")
            };
            if projects.clicked() {
                self.show_projects = true;
            }
            self.composition_toolbar(ui);
            let new = if compact {
                icon_button(ui, Icon::Plus, "New: an empty canvas", false)
            } else {
                quiet_button(ui, "New", Some(Icon::Plus)).on_hover_text("An empty canvas")
            };
            if new.clicked() {
                self.open(Scene::empty(new_id()));
            }
            if quiet_button(ui, if compact { "?" } else { "Help" }, None)
                .on_hover_text("The quick guide")
                .clicked()
            {
                self.show_help = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(self.export.is_none(), |ui| {
                    if primary_button(ui, "Export", Mood::Ember)
                        .on_hover_text("A video, a picture or the project file, into Files")
                        .clicked()
                    {
                        self.export_error = None;
                        self.show_export = true;
                    }
                });
                if let Some(export) = &self.export {
                    if quiet_button(ui, "Stop", None).clicked() {
                        export.cancel.store(true, Ordering::Relaxed);
                    }
                    let fraction = export.progress.load(Ordering::Relaxed) as f32 / 1000.0;
                    ui.add(egui::ProgressBar::new(fraction).desired_width(75.0));
                }
                // Electron Forma's MCP button: lets Codex or Claude drive this editor until it's turned off.
                let on = self.bridge.is_some();
                let hint = if on {
                    format!("Assistants can control this editor. Click to stop. Their exports go to {}", self.mcp_exports().display())
                } else {
                    "Click to let Codex or Claude control this editor through Forma MCP (Forma.exe --mcp-server)".to_owned()
                };
                if pill(ui, if on { "MCP on" } else { "MCP off" }, None, on).on_hover_text(hint).clicked() {
                    if let Err(error) = self.set_mcp(!on, ui.ctx()) {
                        host.toast(format!("MCP could not start: {error}"));
                    }
                }
                if compact {
                    return;
                }
                ui.label(
                    egui::RichText::new(if self.mcp_video.is_some() {
                        "Assistant exporting…"
                    } else if self.edited_at.is_some() {
                        "Saving..."
                    } else if self.save_failed {
                        "Autosave failed"
                    } else {
                        "Saved locally"
                    })
                    .font(font::ui(12.0))
                    .color(color::INK_MUTED),
                );
            });
        });
    }

    /// Electron's tool rail: Add and Layers open the drawer beside it; Text, Picture and Draw move act at once;
    /// Scene opens the inspector's Scene tab.
    fn rail(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let drawer = self.drawer;
            if rail_button(
                ui,
                RailMark::Symbol(Icon::Plus),
                "Add",
                "Add a shape, text, or picture",
                drawer == Some(Drawer::Add),
                true,
            )
            .clicked()
            {
                self.drawer = if drawer == Some(Drawer::Add) {
                    None
                } else {
                    Some(Drawer::Add)
                };
            }
            if rail_button(
                ui,
                RailMark::Symbol(Icon::Sidebar),
                "Layers",
                "View and reorder all layers",
                drawer == Some(Drawer::Layers),
                true,
            )
            .clicked()
            {
                self.drawer = if drawer == Some(Drawer::Layers) {
                    None
                } else {
                    Some(Drawer::Layers)
                };
            }
            ui.separator();
            if rail_button(
                ui,
                RailMark::Symbol(Icon::Text),
                "Text",
                "Add text. With a layer selected, the text follows it.",
                false,
                true,
            )
            .clicked()
            {
                self.add_text();
            }
            if rail_button(
                ui,
                RailMark::Symbol(Icon::Image),
                "Picture",
                "Add pictures, SVG artwork, or video",
                false,
                true,
            )
            .clicked()
            {
                self.pick_media();
            }
            if rail_button(
                ui,
                RailMark::Pen,
                "Pen",
                "Draw an editable path",
                self.vector_tools.tool == Some(VectorTool::Pen),
                true,
            )
            .clicked()
            {
                self.camera_tool = false;
                self.vector_tools.toggle(VectorTool::Pen, None);
                self.mask_editor.stop();
                (self.motion_mode, self.motion_draft) = (None, None);
                self.gesture = None;
                self.playing = false;
                self.tab = Tab::Layer;
            }
            self.mask_editor.sync(self.selected.as_deref());
            let masking = self.mask_editor.active();
            if rail_button(
                ui,
                RailMark::Mask,
                "Mask",
                "Draw and edit masks on the selected layer",
                masking,
                self.selected.is_some(),
            )
            .clicked()
            {
                self.camera_tool = false;
                self.mask_editor.toggle(&self.scene);
                if self.mask_editor.active() {
                    self.vector_tools.set(None, None);
                    (self.motion_mode, self.motion_draft) = (None, None);
                    self.gesture = None;
                    self.playing = false;
                    self.tab = Tab::Layer;
                }
            }
            let drawing = self.motion_mode.is_some();
            let can_draw = self.selected.is_some();
            if rail_button(
                ui,
                RailMark::Move,
                "Draw move",
                "Draw a move for the selected layer (Escape stops)",
                drawing,
                can_draw || drawing,
            )
            .clicked()
            {
                self.camera_tool = false;
                if drawing {
                    (self.motion_mode, self.motion_draft) = (None, None);
                } else {
                    self.mask_editor.stop();
                    self.vector_tools.set(None, None);
                    self.motion_mode = Some("arrow");
                    self.tab = Tab::Animate;
                }
            }
            if rail_button(
                ui,
                RailMark::Symbol(Icon::Project),
                "Scene",
                "Canvas size, length, and background",
                self.tab == Tab::Scene,
                true,
            )
            .clicked()
            {
                self.tab = Tab::Scene;
            }
        });
    }

    /// Text on its own, or following the selected layer when that isn't text (Electron's addText).
    fn add_text(&mut self) {
        let target = self
            .selected
            .as_deref()
            .and_then(|id| self.scene.shapes.iter().position(|s| s.id == id))
            .filter(|&i| self.scene.shapes[i].kind != "text");
        let id = match target {
            Some(target) => self.scene.add_linked_text(new_id(), target),
            None => self.scene.add_text(new_id()),
        };
        self.select_layer(Some(id), false);
        self.tab = Tab::Layer;
        self.edited();
    }

    fn pick_media(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title("Add pictures, SVG artwork or video")
            .add_filter(
                "Pictures, SVG and video",
                &["png", "jpg", "jpeg", "webp", "svg", "mp4", "webm", "ogv"],
            )
            .pick_files();
        self.pending_import.extend(picked.unwrap_or_default());
    }

    /// The drawer beside the rail: Add (shapes, text, files, stop-motion) or Layers (the list), as tabs.
    fn drawer(&mut self, ui: &mut Ui) {
        if !self
            .selected
            .as_ref()
            .is_some_and(|id| self.multi_selected.contains(id))
        {
            self.multi_selected.clear();
        }
        ui.horizontal(|ui| {
            for (which, label) in [(Drawer::Add, "Add"), (Drawer::Layers, "Layers")] {
                let on = self.drawer == Some(which);
                let text = egui::RichText::new(label).font(if on {
                    font::bold(14.0)
                } else {
                    font::ui(14.0)
                });
                if ui
                    .add(egui::Button::selectable(on, text).min_size(vec2(0.0, 40.0)))
                    .clicked()
                {
                    self.drawer = Some(which);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Close, "Close", false).clicked() {
                    self.drawer = None;
                }
            });
        });
        ui.separator();
        crate::panel_widgets::scroll("drawer-content", ui, |ui| match self.drawer {
            Some(Drawer::Add) => self.drawer_add(ui),
            Some(Drawer::Layers) => self.drawer_layers(ui),
            None => {}
        });
    }

    fn drawer_add(&mut self, ui: &mut Ui) {
        section_label(ui, "Shapes");
        ui.horizontal_wrapped(|ui| {
            for (label, kind) in [
                ("Circle", "circle"),
                ("Rectangle", "rect"),
                ("Triangle", "triangle"),
                ("Diamond", "diamond"),
                ("Arrow", "arrow"),
            ] {
                if quiet_button(ui, label, Some(Icon::Plus)).clicked() {
                    let id = self.scene.add_layer(new_id(), kind);
                    self.select_layer(Some(id), false);
                    self.tab = Tab::Layer;
                    self.edited();
                }
            }
        });
        ui.add_space(6.0);
        let follows = self
            .selected
            .as_deref()
            .and_then(|id| self.scene.shapes.iter().find(|s| s.id == id))
            .is_some_and(|s| s.kind != "text");
        if quiet_button(ui, "Text", Some(Icon::Text))
            .on_hover_text(if follows { "Text that follows the selected layer" } else { "Text on its own" })
            .clicked()
        {
            self.add_text();
        }
        ui.add_space(6.0);
        if quiet_button(ui, "Adjustment layer", Some(Icon::Plus))
            .on_hover_text("Apply effects to the layers below")
            .clicked()
        {
            let id = self.scene.add_shape(new_id(), "adjustment");
            self.select_layer(Some(id), false);
            self.tab = Tab::Layer;
            self.edited();
        }
        if !self.scene.shapes.iter().any(Shape::is_camera)
            && quiet_button(ui, "Camera", Some(Icon::Plus))
                .on_hover_text("Add a camera for 3D layers")
                .clicked()
        {
            self.add_camera();
        }
        section_label(ui, "Your files");
        if quiet_button(ui, "Picture, SVG or video…", Some(Icon::Image)).clicked() {
            self.pick_media();
        }
        if quiet_button(ui, "Stop-motion poses…", Some(Icon::Image))
            .on_hover_text("Pictures in the order picked become poses, half a second each, on one track")
            .clicked()
        {
            let picked = rfd::FileDialog::new()
                .set_title("Pick the poses in order")
                .add_filter("Pictures and SVG", &["png", "jpg", "jpeg", "webp", "svg"])
                .pick_files();
            self.pending_poses.extend(picked.unwrap_or_default());
        }
        ui.label(egui::RichText::new("Or drop files onto the canvas.").color(color::INK_MUTED));
        ui.add_space(8.0);
        if ui.button("Load default project").clicked() {
            // As Electron's: the default project if there is one, else the example scene.
            let id = new_id();
            let (scene, notice) = match storage::load_default(&self.place, id.clone()) {
                Ok(Some(scene)) => (scene, "Default project loaded".to_owned()),
                Ok(None) => (Scene::example(id), "Example scene loaded".to_owned()),
                Err(error) => (
                    Scene::example(id),
                    format!(
                        "The default project couldn't be opened ({error}); here's the example scene"
                    ),
                ),
            };
            self.open(scene);
            self.edited();
            self.notice = Some(notice);
        }
        ui.separator();
        ui.label(egui::RichText::new("New to animation?").font(font::bold(12.0)));
        ui.label("Move the playhead, change a layer, then press play.");
        if ui.link("Show the quick guide").clicked() {
            self.show_help = true;
        }
    }

    fn drawer_layers(&mut self, ui: &mut Ui) {
        let several = self.targets().len() > 1;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.selected.is_some(), egui::Button::new("Duplicate"))
                .on_hover_text("Ctrl+D")
                .clicked()
            {
                self.clip_action(ClipAction::Duplicate);
            }
            let label = if several { "Delete all picked" } else { "Delete" };
            if ui
                .add_enabled(self.selected.is_some(), egui::Button::new(label))
                .on_hover_text("Delete")
                .clicked()
            {
                self.clip_action(ClipAction::Delete);
            }
        });
        let hint = if self.scene.shapes.is_empty() {
            "No layers yet: add one from Add."
        } else {
            "Shift-click picks several; drag one to reorder."
        };
        if self.selected.is_some() && quiet_button(ui, "Precompose", None).clicked() {
            self.precompose_selection();
        }
        ui.label(egui::RichText::new(hint).color(color::INK_MUTED));
        let mut destination = None;
        let mut clicked = None;
        let mut open = None;
        for (index, shape) in self.scene.shapes.iter().enumerate() {
            let response = ui
                .add(
                    egui::Button::selectable(
                        self.multi_selected.contains(&shape.id)
                            || self.selected.as_deref() == Some(&shape.id),
                        &shape.name,
                    )
                    .sense(Sense::click_and_drag())
                    .truncate(),
                )
                .on_hover_text(&shape.name);
            if response.double_clicked() {
                open = Some(shape.id.clone());
            }
            if response.clicked() {
                clicked = Some(shape.id.clone());
            }
            if response.drag_started() {
                self.layer_drag = Some(shape.id.clone());
            }
            if self.layer_drag.is_some()
                && ui.input(|i| i.pointer.any_released())
                && response
                    .rect
                    .contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or_default())
            {
                destination = Some(index);
            }
            if self.layer_drag.is_some() && response.hovered() {
                ui.painter().rect_stroke(
                    response.rect,
                    3.0,
                    egui::Stroke::new(1.0, color::INK),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if let Some(id) = clicked {
            self.select_layer(Some(id), ui.input(|i| i.modifiers.shift));
        }
        if let Some(id) = open {
            self.queue_composition(&id);
        }
        if ui.input(|i| i.pointer.any_released()) {
            if let (Some(id), Some(target)) = (self.layer_drag.take(), destination) {
                if let Some(from) = self.scene.shapes.iter().position(|shape| shape.id == id) {
                    if from != target {
                        let shape = self.scene.shapes.remove(from);
                        self.scene.shapes.insert(target, shape);
                        self.edited();
                    }
                }
            }
        }
    }

    /// The Scene tab: canvas size, background and length (Electron's Scene panel).
    fn scene_settings(&mut self, ui: &mut Ui) {
        section_label(ui, "Canvas");
        ui.checkbox(&mut self.scene.transparent_background, "Transparent canvas");
        if quiet_button(ui, "Compositions", None).clicked() {
            self.show_compositions = true;
        }
        ui.scope(|ui| {
                let (mut width, mut height) = (self.scene.width as u32, self.scene.height as u32);
                // Wrapped: the inspector is narrower than the three sizes side by side.
                ui.horizontal_wrapped(|ui| {
                    for (label, w, h) in [
                        ("Landscape", 960, 600),
                        ("Portrait", 1080, 1920),
                        ("Square", 1080, 1080),
                    ] {
                        if ui
                            .selectable_label(width == w && height == h, label)
                            .clicked()
                        {
                            (width, height) = (w, h);
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Width");
                    ui.add(egui::DragValue::new(&mut width).range(100..=4000).speed(1));
                    ui.label("Height");
                    ui.add(egui::DragValue::new(&mut height).range(100..=4000).speed(1));
                });
                if width as f32 != self.scene.width || height as f32 != self.scene.height {
                    self.scene.resize(width as f32, height as f32);
                    self.edited();
                }
                // Electron's background swatches, a custom color, and the color as text.
                ui.label("Background");
                ui.horizontal_wrapped(|ui| {
                    for (bg, name) in [
                        ("#f4f0e8", "Warm white"),
                        ("#141617", "Charcoal"),
                        ("#d2f583", "Lime"),
                        ("#aaa0ce", "Lavender"),
                        ("#ed795e", "Coral"),
                    ] {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                        let chosen = self.scene.bg.eq_ignore_ascii_case(bg);
                        ui.painter().circle(
                            rect.center(),
                            10.0,
                            hex(bg, 255),
                            Stroke::new(
                                if chosen { 2.0 } else { 1.0 },
                                if chosen { color::SUN } else { color::HAIRLINE },
                            ),
                        );
                        if response
                            .on_hover_text(format!("{name} background"))
                            .clicked()
                            && !chosen
                        {
                            self.scene.bg = bg.into();
                            self.edited();
                        }
                    }
                    let current = hex(&self.scene.bg, 255);
                    let mut rgb = [current.r(), current.g(), current.b()];
                    if egui::color_picker::color_edit_button_srgb(ui, &mut rgb)
                        .on_hover_text("Custom")
                        .changed()
                    {
                        self.scene.bg = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                        self.edited();
                    }
                });
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.scene.bg)
                            .desired_width(ui.available_width()),
                    )
                    .changed()
                {
                    self.edited();
                }
                // Any length (Electron's field has no top either); the animations already in the scene set the least.
                let min_duration = self.scene.minimum_duration();
                if ui
                    .horizontal(|ui| {
                        ui.label("Length");
                        ui.add(
                            egui::DragValue::new(&mut self.scene.duration)
                                .range(min_duration..=86_400.0)
                                .speed(0.1)
                                .suffix(" s"),
                        )
                    })
                    .inner
                    .changed()
                {
                    self.time = self.time.min(self.scene.duration);
                    self.edited();
                }
            });
        self.motion_blur_settings(ui);
    }

    fn motion_blur_settings(&mut self, ui: &mut Ui) {
        crate::panel_widgets::section(ui, egui::CollapsingHeader::new("Motion blur"), |ui| {
            ui.spacing_mut().interact_size.y = 32.0;
            let mut settings = self.scene.motion_blur.unwrap_or_default();
            let previous = settings;
            ui.horizontal_wrapped(|ui| {
                ui.label("Shutter angle");
                ui.add(
                    egui::DragValue::new(&mut settings.shutter_angle)
                        .range(0.0..=720.0)
                        .suffix("\u{00b0}"),
                );
                ui.label("Samples");
                ui.add(egui::DragValue::new(&mut settings.samples).range(2..=32));
            });
            if settings.valid() && settings != previous {
                self.scene.motion_blur =
                    (settings != crate::motion_blur::Settings::default()).then_some(settings);
            }
            ui.checkbox(&mut self.scene.motion_blur_preview, "Motion blur preview");
        });
    }

    /// The selected layers, in the order they were picked (the main one included once).
    fn selection(&self) -> Vec<usize> {
        let mut ids: Vec<&String> = self.multi_selected.iter().collect();
        if let Some(main) = &self.selected {
            if !ids.contains(&main) {
                ids.push(main);
            }
        }
        ids.iter()
            .filter_map(|id| self.scene.shapes.iter().position(|s| &&s.id == id))
            .collect()
    }

    /// Keys each layer at the playhead with its center moved to `centers` (Electron's arrange `place`). Text that
    /// follows a layer goes after the layers, so it keeps up with where they went.
    fn place_centers(&mut self, mut layers: Vec<(usize, (f32, f32))>) {
        layers.sort_by_key(|(i, _)| self.scene.ancestor_depth(*i));
        self.playing = false;
        let count = layers.len();
        let poses: Vec<_> = layers
            .into_iter()
            .map(|(index, (x, y))| {
                let shape = &self.scene.shapes[index];
                let pose = self.scene.editable_pose(shape, self.time);
                let center = self.scene.render_pose(shape, self.time);
                (
                    index,
                    Pose {
                        x: pose.x + x - center.x,
                        y: pose.y + y - center.y,
                        ..pose
                    },
                )
            })
            .collect();
        for (index, pose) in poses {
            self.scene
                .set_property_pose(index, self.time, pose, Property::Position);
        }
        self.edited();
        self.notice = Some(format!("Arranged {count} layers at {:.2} s", self.time));
    }

    /// Electron's Arrange: with several layers selected, line them up or space them evenly at the playhead.
    fn arrange_panel(&mut self, ui: &mut Ui) {
        let layers = self.selection();
        if layers.len() < 2 {
            return;
        }
        ui.label(
            egui::RichText::new(format!("Arrange · {} layers", layers.len()))
                .font(font::bold(14.0))
                .color(color::INK),
        );
        let poses: Vec<Pose> = layers
            .iter()
            .map(|&i| self.scene.render_pose(&self.scene.shapes[i], self.time))
            .collect();
        let mut centers = None;
        ui.horizontal_wrapped(|ui| {
            for (label, edge) in [
                ("Left", model::Edge::Left),
                ("Center", model::Edge::Center),
                ("Right", model::Edge::Right),
                ("Top", model::Edge::Top),
                ("Middle", model::Edge::Middle),
                ("Bottom", model::Edge::Bottom),
            ] {
                if ui
                    .button(label)
                    .on_hover_text(format!("Align {}", label.to_lowercase()))
                    .clicked()
                {
                    centers = Some(model::align(&poses, edge));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (label, across) in [("Space evenly across", true), ("Space evenly down", false)] {
                if ui
                    .add_enabled(layers.len() > 2, egui::Button::new(label))
                    .on_disabled_hover_text("Select three layers or more")
                    .clicked()
                {
                    centers = Some(model::distribute(&poses, across));
                }
            }
        });
        ui.label(
            egui::RichText::new(
                "Shift-click layers on the canvas, timeline, or Layers list to select several. Position edits move them together.",
            )
            .font(font::ui(11.0))
            .color(color::INK_MUTED),
        );
        if let Some(centers) = centers {
            self.place_centers(layers.into_iter().zip(centers).collect());
        }
        ui.separator();
    }

    /// Electron's Enter from outside: the layer just outside one side of the canvas at the playhead.
    fn outside_panel(&mut self, ui: &mut Ui, index: usize) {
        ui.separator();
        section_label(ui, "Enter from outside");
        ui.horizontal_wrapped(|ui| {
            for (label, side) in [
                ("Left", model::Edge::Left),
                ("Right", model::Edge::Right),
                ("Above", model::Edge::Top),
                ("Below", model::Edge::Bottom),
            ] {
                if ui.button(label).clicked() {
                    self.playing = false;
                    let pose = self
                        .scene
                        .editable_pose(&self.scene.shapes[index], self.time);
                    let placed = model::outside(pose, side, self.scene.width, self.scene.height);
                    self.scene.set_pose(index, self.time, placed);
                    self.edited();
                    self.notice = Some(format!("Placed outside at {:.2} s", self.time));
                }
            }
        });
        ui.label(
            egui::RichText::new(
                "Places the layer just outside the canvas at the playhead. Move it inside at a later moment to make it walk in.",
            )
            .font(font::ui(11.0))
            .color(color::INK_MUTED),
        );
    }

    fn clip_fields(&mut self, ui: &mut Ui, index: usize) {
        let shape = &self.scene.shapes[index];
        // Shown again when another layer is picked, or when the clip changes elsewhere (dragged or trimmed on the
        // timeline, undone), but not while it's being typed in: typing doesn't change the clip until it's committed.
        let timing = (shape.clip_start(), shape.out_time);
        if self.clip_edit_id.as_deref() != Some(shape.id.as_str()) || self.clip_shown != timing {
            self.clip_edit_id = Some(shape.id.clone());
            self.clip_shown = timing;
            self.clip_in_edit = format!("{:.2}", shape.clip_start());
            self.clip_out_edit = shape.out_time.map_or(String::new(), |t| format!("{t:.2}"));
        }
        let mut start_commit = false;
        let mut end_commit = false;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("From (s)");
                let response =
                    ui.add(egui::TextEdit::singleline(&mut self.clip_in_edit).desired_width(82.0));
                start_commit = response.lost_focus()
                    || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
            ui.vertical(|ui| {
                ui.label("Until (s)");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.clip_out_edit)
                        .desired_width(82.0)
                        .hint_text(format!("End ({:.2})", self.scene.duration)),
                );
                end_commit = response.lost_focus()
                    || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
        });
        if start_commit {
            let end = self.scene.shapes[index]
                .out_time
                .unwrap_or(self.scene.duration);
            let start = self.clip_in_edit.trim().parse::<f64>().ok();
            if let Some(start) =
                start.filter(|s| s.is_finite() && *s >= 0.0 && *s <= end - model::MIN_CLIP)
            {
                let previous = self.scene.shapes[index].clip_start();
                if self
                    .scene
                    .trim_clip(index, start, self.scene.shapes[index].out_time)
                    .is_ok()
                {
                    let shape = &self.scene.shapes[index];
                    self.clip_in_edit = format!("{:.2}", shape.clip_start());
                    if !shape.visible(self.time, self.scene.duration) {
                        self.time = shape.clip_start();
                    }
                    if shape.clip_start() != previous {
                        self.edited();
                    }
                } else {
                    self.clip_in_edit = format!("{previous:.2}");
                    self.notice = Some("Start the clip at least 0.05 s before it ends".into());
                }
            } else {
                self.clip_in_edit = format!("{:.2}", self.scene.shapes[index].clip_start());
                self.notice = Some("Start the clip at least 0.05 s before it ends".into());
            }
        }
        if end_commit {
            let start = self.scene.shapes[index].clip_start();
            let raw = self.clip_out_edit.trim();
            let end = if raw.is_empty() {
                Some(None)
            } else {
                raw.parse::<f64>()
                    .ok()
                    .filter(|e| e.is_finite() && *e >= start + model::MIN_CLIP)
                    .map(Some)
            };
            if let Some(end) = end {
                let previous = self.scene.shapes[index].out_time;
                if self.scene.trim_clip(index, start, end).is_ok() {
                    self.clip_out_edit = self.scene.shapes[index]
                        .out_time
                        .map_or(String::new(), |t| format!("{t:.2}"));
                    if self.scene.shapes[index].out_time != previous {
                        self.edited();
                    }
                } else {
                    self.clip_out_edit = previous.map_or(String::new(), |t| format!("{t:.2}"));
                    self.notice = Some("End the clip at least 0.05 s after it starts".into());
                }
            } else {
                self.clip_out_edit = self.scene.shapes[index]
                    .out_time
                    .map_or(String::new(), |t| format!("{t:.2}"));
                self.notice = Some("End the clip at least 0.05 s after it starts".into());
            }
        }
    }

    /// The inspector, in Electron's three tabs: Layer (how it looks, when it shows, where it is), Animate (easing and
    /// every animation) and Scene (canvas size, background, length).
    fn inspector(&mut self, ui: &mut Ui) {
        if std::mem::take(&mut self.open_scene) {
            self.tab = Tab::Scene;
        }
        // Picking a move or a spin on the timeline brings up its settings.
        let picked = (self.selected_motion.clone(), self.selected_spin.clone());
        if picked != self.last_picked {
            if picked.0.is_some() || picked.1.is_some() {
                self.tab = Tab::Animate;
            }
            self.last_picked = picked;
        }
        ui.horizontal(|ui| {
            for (tab, label) in [
                (Tab::Layer, "Layer"),
                (Tab::Animate, "Animate"),
                (Tab::Scene, "Scene"),
            ] {
                let text = egui::RichText::new(label).font(if self.tab == tab {
                    font::bold(14.0)
                } else {
                    font::ui(14.0)
                });
                if ui
                    .add(egui::Button::selectable(self.tab == tab, text).min_size(vec2(0.0, 40.0)))
                    .clicked()
                {
                    self.tab = tab;
                }
            }
        });
        ui.separator();
        crate::panel_widgets::scroll(("forma-inspector", self.tab), ui, |ui| {
            self.inspector_content(ui)
        });
    }

    fn inspector_content(&mut self, ui: &mut Ui) {
        if self.tab == Tab::Scene {
            self.scene_settings(ui);
            return;
        }
        let Some(index) = self
            .scene
            .shapes
            .iter()
            .position(|s| Some(s.id.as_str()) == self.selected.as_deref())
        else {
            ui.label(
                egui::RichText::new(
                    "Select a layer on the canvas, in the timeline or in Layers to edit it.",
                )
                .color(color::INK_MUTED),
            );
            return;
        };
        if self.scene.shapes[index].is_camera() {
            if self.tab == Tab::Animate {
                if quiet_button(ui, "Show camera lanes", None).clicked() {
                    self.key_editor
                        .expanded
                        .insert(self.scene.shapes[index].id.clone());
                }
            } else {
                self.camera_panel(ui, index);
            }
            return;
        }
        if self.tab == Tab::Animate {
            self.animate_tab(ui, index);
            return;
        }
        ui.label("Layer name");
        let shape = &mut self.scene.shapes[index];
        let mut changed = ui.text_edit_singleline(&mut shape.name).changed();
        if shape.kind == "text" {
            changed |= text_panel::content(ui, shape);
            ui.separator();
        }
        let size = self
            .scene
            .layer_frame(&self.scene.shapes[index], self.time)
            .pose;
        let shape = &mut self.scene.shapes[index];
        let tool_before = self.vector_tools.tool;
        let has_paint = shape.is_vector() || shape.kind == "text";
        let show_solid = !matches!(shape.kind.as_str(), "adjustment" | "composition")
            && (!has_paint
                || shape_panel::fill_panel(
                    ui,
                    shape,
                    [size.w, size.h],
                    &mut self.vector_tools,
                    self.time,
                ));
        changed |= show_solid && solid_fill_color(ui, shape, self.time);
        if has_paint {
            shape_panel::stroke_and_trim(ui, shape, &mut self.vector_tools, self.time);
        }
        if self.vector_tools.tool != tool_before && self.vector_tools.tool.is_some() {
            self.mask_editor.stop();
            (self.motion_mode, self.motion_draft) = (None, None);
            self.gesture = None;
            self.playing = false;
        }
        self.follows_picker(ui, index, false);
        self.layer_3d_panel(ui, index);
        let was_masking = self.mask_editor.active();
        self.mask_editor
            .panel(ui, &mut self.scene, index, self.time, &mut self.playing);
        if self.mask_editor.active() {
            if !was_masking {
                self.vector_tools.set(None, None);
            }
            (self.motion_mode, self.motion_draft) = (None, None);
        }
        self.arrange_panel(ui);
        self.clip_fields(ui, index);
        let mut pose = self
            .scene
            .inspector_pose(&self.scene.shapes[index], self.time);
        let shape = &mut self.scene.shapes[index];
        composition_panel::layer_settings(ui, shape);
        if shape.kind == "text" {
            let linked = shape
                .extra
                .get("linkId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let text_id = shape.id.clone();
            let others: Vec<(usize, String, String)> = self
                .scene
                .shapes
                .iter()
                .enumerate()
                .filter(|(i, s)| {
                    s.kind != "text" && s.id != text_id && self.scene.can_follow(index, *i)
                })
                .map(|(i, s)| (i, s.id.clone(), s.name.clone()))
                .collect();
            let follows = others
                .iter()
                .find(|(_, id, _)| *id == linked)
                .map_or("Nothing".to_owned(), |(.., name)| name.clone());
            let mut relink = None;
            crate::panel_widgets::combo(ui, "text-linked-to", "Text linked to", follows).show_ui(
                ui,
                |ui| {
                    for (i, id, name) in &others {
                        if ui
                            .add(egui::Button::selectable(*id == linked, name).truncate())
                            .on_hover_text(name)
                            .clicked()
                            && *id != linked
                        {
                            relink = Some(*i);
                        }
                    }
                });
            if let Some(target) = relink {
                self.scene.link_text(index, target, self.time);
                changed = true;
            }
            let paths = self
                .scene
                .shapes
                .iter()
                .filter(|s| s.kind == "path")
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect::<Vec<_>>();
            let shape = &mut self.scene.shapes[index];
            if text_panel::layout(ui, shape, &paths, self.time) {
                self.key_editor.expanded.insert(shape.id.clone());
            }
        }
        let shape = &mut self.scene.shapes[index];
        if shape.kind == "svg"
            && ui
                .button("Edit SVG…")
                .on_hover_text("Change its markup or recolor it")
                .clicked()
        {
            let source = assets::svg_source(shape, &self.place)
                .ok()
                .flatten()
                .unwrap_or_default();
            self.svg_edit = Some(SvgEdit {
                id: shape.id.clone(),
                source,
                color: [0x1f, 0x3d, 0x2b],
                selected_part: None,
                status: String::new(),
            });
        }
        let shape = &mut self.scene.shapes[index];
        if crate::effect_panel::layer(
            ui,
            shape,
            self.time,
            self.key_editor.effects_expanded(&shape.id),
        ) {
            self.key_editor.reveal_effects(shape.id.clone());
        }
        self.scene.duration = shape.effect_times().fold(self.scene.duration, f64::max);
        let mut blur = shape
            .extra
            .get("blur")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        if shape.kind != "adjustment" {
            ui.label("Blur");
            if ui
                .add(
                    egui::Slider::new(&mut blur, 0.0..=100.0)
                        .clamping(egui::SliderClamping::Edits)
                        .step_by(1.0)
                        .suffix(" px"),
                )
                .changed()
            {
                if blur > 0.0 {
                    shape.extra.insert("blur".into(), serde_json::json!(blur));
                } else {
                    shape.extra.remove("blur");
                }
                changed = true;
            }
        }
        let mut posed = false;
        let before = pose;
        let lock_ratio = &mut self.lock_ratio;
        let old_anchor = self.scene.shapes[index].anchor.unwrap_or_default();
        let mut anchor = old_anchor;
        // Exact numbers, folded away as in Electron: dragging on the canvas does the same.
        crate::panel_widgets::section(
            ui,
            egui::CollapsingHeader::new("Position & size").id_salt("forma-position"),
            |ui| {
                ui.label(
                    egui::RichText::new("Measured on the canvas")
                        .small()
                        .color(color::INK_MUTED),
                );
                ui.checkbox(lock_ratio, "Lock size ratio");
                for (label, value, range) in [
                    ("X", &mut pose.x, -4000.0..=4000.0),
                    ("Y", &mut pose.y, -4000.0..=4000.0),
                    ("Width", &mut pose.w, 1.0..=4000.0),
                    ("Height", &mut pose.h, 1.0..=4000.0),
                    ("Angle", &mut pose.r, -3600.0..=3600.0),
                    ("Opacity", &mut pose.o, 0.0..=100.0),
                ] {
                    ui.label(label);
                    posed |= ui
                        .add(egui::Slider::new(value, range).clamping(egui::SliderClamping::Edits))
                        .changed();
                }
                Self::anchor_controls(ui, &mut anchor);
            });
        if *lock_ratio && posed {
            if pose.w != before.w {
                pose.h = (before.h * pose.w / before.w).max(1.0);
            } else if pose.h != before.h {
                pose.w = (before.w * pose.h / before.h).max(1.0);
            }
        }
        let added = ui
            .button("Add keyframe here")
            .on_hover_text("Remember the layer as it is now at the playhead")
            .clicked();
        if posed {
            self.apply_inspector_pose(index, before, pose);
            changed = true;
        }
        if anchor != old_anchor {
            self.scene.set_anchor(index, anchor, self.time);
            changed = true;
        }
        if added {
            for property in Property::ALL {
                self.scene.shapes[index].add_lane_key(Lane::Pose(property), self.time);
            }
        }
        if changed || added {
            self.edited();
        }
        ui.add_space(10.0);
        if ui
            .button(if self.targets().len() > 1 {
                "Delete selected layers"
            } else {
                "Delete layer"
            })
            .on_hover_text("Delete")
            .clicked()
        {
            self.clip_action(ClipAction::Delete);
        }
    }

    fn add_camera(&mut self) -> Option<String> {
        match self.scene.add_camera(new_id()) {
            Ok(id) => {
                self.select_layer(Some(id.clone()), false);
                self.key_editor.expanded.insert(id.clone());
                self.tab = Tab::Layer;
                self.drawer = None;
                self.edited();
                Some(id)
            }
            Err(error) => {
                self.notice = Some(error);
                None
            }
        }
    }

    fn layer_3d_panel(&mut self, ui: &mut Ui, index: usize) {
        use crate::spatial::{Property, Spatial};
        let shape = &mut self.scene.shapes[index];
        if shape.kind == "adjustment" {
            return;
        }
        let mut enabled = shape.is_3d();
        if ui.add_sized([40.0, 40.0], egui::Checkbox::new(&mut enabled, "3D"))
            .on_hover_text("Use camera perspective. Layers are sorted back to front by their centers. Intersecting layers are drawn whole.")
            .changed()
        {
            if enabled {
                shape.spatial.get_or_insert_with(Spatial::default).enabled = true;
                self.key_editor.expanded.insert(shape.id.clone());
            } else if let Some(spatial) = &mut shape.spatial {
                spatial.enabled = false;
                if spatial.tracks.is_empty() {
                    shape.spatial = None;
                }
            }
        }
        if enabled {
            crate::panel_widgets::section(ui, egui::CollapsingHeader::new("3D transform"), |ui| {
                for property in Property::LAYER {
                    spatial_field(ui, shape, property, self.time);
                }
            });
        }
    }

    fn camera_panel(&mut self, ui: &mut Ui, index: usize) {
        use crate::spatial::Property;
        ui.label("Camera name");
        ui.add(egui::TextEdit::singleline(
            &mut self.scene.shapes[index].name,
        ));
        self.follows_picker(ui, index, false);
        self.clip_fields(ui, index);
        let mut pose = self
            .scene
            .inspector_pose(&self.scene.shapes[index], self.time);
        let previous = pose;
        for (label, value) in [
            ("X position", &mut pose.x),
            ("Y position", &mut pose.y),
            ("Z rotation", &mut pose.r),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add_sized(
                    [100.0, 32.0],
                    egui::DragValue::new(value)
                        .speed(1.0)
                        .range(-1_000_000.0..=1_000_000.0),
                );
            });
        }
        if [pose.x, pose.y, pose.r].iter().all(|v| v.is_finite()) {
            self.apply_inspector_pose(index, previous, pose);
        }
        let shape = &mut self.scene.shapes[index];
        spatial_field(ui, shape, Property::Depth, self.time);
        spatial_field(ui, shape, Property::FieldOfView, self.time);
        let zoom_text = |shape: &Shape| {
            let fov = shape.spatial_value(Property::FieldOfView, self.time)[0];
            format!(
                "Zoom {:.0} px",
                crate::spatial::focal(self.scene.height, fov)
            )
        };
        ui.label(egui::RichText::new(zoom_text(shape)).color(color::INK_MUTED));
        crate::panel_widgets::section(ui, egui::CollapsingHeader::new("Aim and focus"), |ui| {
            let spatial = shape.spatial.get_or_insert_with(Default::default);
            ui.add_sized(
                [100.0, 32.0],
                egui::Checkbox::new(&mut spatial.one_node, "One-node camera"),
            )
            .on_hover_text("Aim using the camera rotation");
            let one_node = spatial.one_node;
            ui.add_sized(
                [100.0, 32.0],
                egui::Checkbox::new(&mut spatial.depth_of_field, "Depth of field"),
            )
            .on_hover_text("Blur 3D layers in front of or behind the focus distance");
            let depth_of_field = spatial.depth_of_field;
            if !one_node {
                spatial_field(ui, shape, Property::Interest, self.time);
            }
            for property in [
                Property::RotationX,
                Property::RotationY,
                Property::Orientation,
            ] {
                spatial_field(ui, shape, property, self.time);
            }
            if depth_of_field {
                spatial_field(ui, shape, Property::FocusDistance, self.time);
                spatial_field(ui, shape, Property::Aperture, self.time);
            }
        });
    }

    fn camera_drag(&mut self, delta: Vec2, pan: bool, scale: f32) {
        if delta == Vec2::ZERO {
            return;
        }
        let camera = self
            .scene
            .shapes
            .iter()
            .rev()
            .find(|shape| shape.is_camera() && shape.visible(self.time, self.scene.duration))
            .map(|shape| shape.id.clone());
        if let Some(id) = camera {
            self.scene
                .move_camera(&id, self.time, [delta.x, delta.y], pan, scale);
        }
    }

    fn apply_inspector_pose(&mut self, index: usize, old: Pose, pose: Pose) {
        let properties: Vec<_> = Property::ALL
            .into_iter()
            .filter(|p| p.values(old) != p.values(pose))
            .collect();
        if properties.is_empty() {
            return;
        }
        self.playing = false;
        let at = model::hundredths(self.time);
        self.time = if at <= self.scene.duration {
            at
        } else {
            (self.scene.duration * 100.0).floor() / 100.0
        };
        let mut layers: Vec<_> = self
            .selection()
            .into_iter()
            .filter(|&i| i != index)
            .map(|i| {
                let mut other = self.scene.inspector_pose(&self.scene.shapes[i], self.time);
                for &property in &properties {
                    let values = if property == Property::Position {
                        [other.x + pose.x - old.x, other.y + pose.y - old.y]
                    } else {
                        property.values(pose)
                    };
                    property.set(&mut other, values);
                }
                (i, other)
            })
            .collect();
        layers.push((index, pose));
        layers.sort_by_key(|(i, _)| self.scene.ancestor_depth(*i));
        for (i, pose) in layers {
            self.scene
                .set_inspector_pose(i, self.time, pose, &properties);
        }
    }

    fn follows_picker(&mut self, ui: &mut Ui, index: usize, menu: bool) {
        let shape = &self.scene.shapes[index];
        let parent = shape.parent.as_deref();
        let current = self
            .scene
            .shapes
            .iter()
            .find(|s| Some(s.id.as_str()) == parent)
            .map_or("None", |s| s.name.as_str());
        let mut pick = None;
        let choices = |ui: &mut Ui| {
            if ui.selectable_label(parent.is_none(), "None").clicked() {
                pick = Some(None);
                ui.close();
            }
            for (i, other) in self.scene.shapes.iter().enumerate() {
                if self.scene.can_follow(index, i)
                    && ui
                        .add(
                            egui::Button::selectable(parent == Some(&other.id), &other.name)
                                .truncate(),
                        )
                        .on_hover_text(&other.name)
                        .clicked()
                {
                    pick = Some(Some(i));
                    ui.close();
                }
            }
        };
        if menu {
            ui.menu_button("Follows", choices);
        } else {
            crate::panel_widgets::combo(ui, ("parent", &shape.id), "Follows", current)
                .show_ui(ui, choices);
        }
        if let Some(parent) = pick {
            match self.scene.set_parent(index, parent, self.time) {
                Ok(()) => self.edited(),
                Err(error) => self.notice = Some(error.into()),
            }
        }
    }

    fn anchor_controls(ui: &mut Ui, anchor: &mut Anchor) {
        ui.label("Anchor");
        ui.horizontal(|ui| {
            for (label, value) in [("X", &mut anchor.x), ("Y", &mut anchor.y)] {
                ui.label(label);
                ui.add_sized(
                    [64.0, 32.0],
                    egui::DragValue::new(value)
                        .speed(0.01)
                        .fixed_decimals(2)
                        .range(-10.0..=10.0),
                );
            }
        });
        egui::Grid::new("anchor-grid")
            .spacing(vec2(4.0, 4.0))
            .show(ui, |ui| {
                for (y, row) in ["Top", "Middle", "Bottom"].into_iter().enumerate() {
                    for (x, column) in ["left", "center", "right"].into_iter().enumerate() {
                        let target = Anchor {
                            x: x as f32 / 2.0,
                            y: y as f32 / 2.0,
                        };
                        let glyph = control_symbol(ANCHOR_GLYPHS[y][x]);
                        if ui
                            .add_sized(
                                [32.0, 32.0],
                                egui::Button::selectable(*anchor == target, glyph),
                            )
                            .on_hover_text(format!("{row} {column} anchor"))
                            .clicked()
                        {
                            *anchor = target;
                        }
                    }
                    ui.end_row();
                }
            });
        ui.label(egui::RichText::new("Alt-drag the anchor on the canvas").color(color::INK_MUTED));
    }

    /// The Animate tab: one-click animations, easing, and the moves, spins and fades on the layer.
    fn animate_tab(&mut self, ui: &mut Ui, index: usize) {
        let shape = &self.scene.shapes[index];
        if !shape.effects.is_empty() && quiet_button(ui, "Effect animation lanes", None).clicked() {
            self.key_editor.reveal_effects(shape.id.clone());
        }
        if self.scene.shapes[index].kind == "text" {
            let duration = self.scene.duration;
            let shape = &mut self.scene.shapes[index];
            let result = text_panel::animate(ui, shape, self.time, duration);
            if result.full {
                self.notice = Some("This layer has reached its limit of 64 text animators".into());
            }
            if let Some(id) = result.reveal {
                self.key_editor.reveal_animator(shape.id.clone(), id);
            }
            if let Some(end) = result.end {
                self.scene.duration = duration.max(end);
            }
            ui.separator();
        }
        let mut changed = false;
        let shape = &mut self.scene.shapes[index];
        let pose = shape.lane_pose(self.time);
        let mut animated = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Move →").clicked() {
                shape.key_property(self.time, Property::Position, pose);
                shape.key_property(
                    self.time + 1.0,
                    Property::Position,
                    Pose {
                        x: pose.x + 180.0,
                        ..pose
                    },
                );
                animated = true;
            }
            if ui.button("Fade out").clicked() {
                shape.key_property(self.time, Property::Opacity, pose);
                shape.key_property(self.time + 1.0, Property::Opacity, Pose { o: 0.0, ..pose });
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
        ui.scope(|ui| {
            crate::panel_widgets::combo(
                ui,
                ("ease", &shape.id),
                "Easing",
                EASES
                    .iter()
                    .find(|(_, kind)| *kind == shape.ease)
                    .map_or(shape.ease.as_str(), |(name, _)| name),
            )
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
        if changed || animated {
            self.edited();
        }
        self.motion_panel(ui, index);
        self.spin_panel(ui, index);
        self.fade_panel(ui, index);
        self.outside_panel(ui, index);
    }

    /// Electron's drop: the layer falls onto where it is from above the canvas, squashes and bounces, over the
    /// Seconds option (0.4-10 s) from the playhead, then the drop plays once.
    fn drop_onto_screen(&mut self, index: usize) {
        let start = (self.time * 100.0).round() / 100.0;
        let duration = self.motion_options.seconds.clamp(0.4, 10.0);
        let end = ((start + duration) * 100.0).round() / 100.0;
        let shape = &self.scene.shapes[index];
        let linked = shape.kind == "text"
            && shape
                .extra
                .get("linkId")
                .and_then(|v| v.as_str())
                .is_some_and(|id| !id.is_empty());
        let occupied = motion::motions(shape)
            .iter()
            .any(|m| m.start_time < end - 0.005 && m.start_time + m.duration > start + 0.005)
            || shape
                .keys
                .iter()
                .any(|k| k.t > start + 0.005 && k.t <= end + 0.005);
        let landing = shape.pose(start);
        let seen = self.scene.render_pose(shape, start);
        if linked {
            self.notice = Some("Text that follows a shape drops with its shape".into());
        } else if occupied {
            self.notice = Some("Move the playhead to a clear section of this layer first".into());
        } else if seen.x + seen.w / 2.0 < 0.0
            || seen.x - seen.w / 2.0 > self.scene.width
            || seen.y + seen.h / 2.0 < 0.0
            || seen.y - seen.h / 2.0 > self.scene.height
        {
            self.notice = Some("Place the item on the canvas where it should land first".into());
        } else {
            let space = self.scene.layer_frame(shape, start).space;
            let inverse = space.invert().unwrap_or_default();
            let above = -seen.y - seen.h / 2.0 - (self.scene.height * 0.08).max(40.0);
            let (dx, dy) = transform::vector(inverse, 0.0, above);
            let poses = [
                (
                    0.0,
                    Pose {
                        x: landing.x + dx,
                        y: landing.y + dy,
                        ..landing
                    },
                ),
                (
                    0.18,
                    Pose {
                        y: above + (landing.y - above) * 0.07,
                        ..landing
                    },
                ),
                (
                    0.6,
                    Pose {
                        y: landing.y + landing.h * 0.09,
                        w: landing.w * 1.13,
                        h: landing.h * 0.82,
                        ..landing
                    },
                ),
                (
                    0.71,
                    Pose {
                        y: landing.y - landing.h * 0.17,
                        w: landing.w * 0.94,
                        h: landing.h * 1.06,
                        ..landing
                    },
                ),
                (
                    0.8,
                    Pose {
                        y: landing.y - landing.h * 0.28,
                        ..landing
                    },
                ),
                (
                    0.91,
                    Pose {
                        y: landing.y + landing.h * 0.04,
                        w: landing.w * 1.06,
                        h: landing.h * 0.93,
                        ..landing
                    },
                ),
                (1.0, landing),
            ];
            let shape = &mut self.scene.shapes[index];
            for property in [Property::Position, Property::Size] {
                shape.exclude_keys(property, |k| k.t >= start - 0.005 && k.t <= end + 0.005);
            }
            for (fraction, pose) in poses {
                let t = model::hundredths(start + duration * fraction);
                shape.key_property(t, Property::Position, pose);
                shape.key_property(t, Property::Size, pose);
            }
            self.scene.duration = self.scene.duration.max(end);
            self.motion_mode = None;
            self.motion_draft = None;
            self.selected_motion = None;
            self.selected_spin = None;
            self.playing = false;
            self.edited();
            self.notice = Some("Drop animation added".into());
            self.preview(start, end);
        }
    }

    /// Plays from `start` and stops at `end`, as Electron previews a new animation.
    fn preview(&mut self, start: f64, end: f64) {
        self.motion_mode = None;
        (self.time, self.preview_end, self.playing, self.last_tick) = (
            start,
            Some(end.min(self.scene.duration)),
            true,
            Instant::now(),
        );
    }

    fn preview_fade(&mut self, index: usize) {
        if let Some((start, end)) = fade_preview_range(&self.scene, &self.scene.shapes[index]) {
            self.preview(start, end);
        }
    }

    fn queue_auto_preview(&mut self, target: PreviewTarget, ctx: &egui::Context) {
        if self.live_animation_preview {
            self.pending_auto_preview = Some((Instant::now() + Duration::from_millis(300), target));
            ctx.request_repaint_after(Duration::from_millis(310));
        }
    }

    fn run_auto_preview(&mut self, ctx: &egui::Context) {
        let Some((when, target)) = self.pending_auto_preview.clone() else {
            return;
        };
        if Instant::now() < when {
            ctx.request_repaint_after(when.saturating_duration_since(Instant::now()));
            return;
        }
        self.pending_auto_preview = None;
        match target {
            PreviewTarget::Motion(shape_id, motion_id)
                if self.selected.as_deref() == Some(&shape_id)
                    && self.selected_motion.as_deref() == Some(&motion_id) =>
            {
                let Some(index) = self.scene.shapes.iter().position(|s| s.id == shape_id) else {
                    return;
                };
                let (start, length, options) = self.motion_edit;
                match motion::edit(&mut self.scene, index, &motion_id, start, length, options) {
                    Ok(()) => {
                        self.motion_shown = motion::motions(&self.scene.shapes[index])
                            .into_iter()
                            .find(|m| m.id == motion_id)
                            .map(|m| (shape_id, m));
                        self.edited();
                        self.preview(start, start + length);
                    }
                    Err(error) => self.notice = Some(error),
                }
            }
            PreviewTarget::Spin(shape_id, spin_id)
                if self.selected.as_deref() == Some(&shape_id)
                    && self.selected_spin.as_deref() == Some(&spin_id) =>
            {
                if let Some(clip) = self
                    .scene
                    .shapes
                    .iter()
                    .find(|s| s.id == shape_id)
                    .and_then(|s| s.rotation_clips.iter().find(|c| c.id == spin_id))
                {
                    self.preview(clip.start_time, clip.start_time + clip.duration);
                }
            }
            PreviewTarget::Fade(shape_id) if self.selected.as_deref() == Some(&shape_id) => {
                if let Some(index) = self.scene.shapes.iter().position(|s| s.id == shape_id) {
                    self.preview_fade(index);
                }
            }
            _ => {}
        }
    }

    /// A 360° spin at the playhead, or over the chosen drawn move (Electron's Add rotation).
    fn add_spin(&mut self, index: usize) {
        let chosen = motion::motions(&self.scene.shapes[index])
            .into_iter()
            .find(|m| Some(&m.id) == self.selected_motion.as_ref());
        let (start, length) = chosen.map_or((self.time, self.motion_options.seconds), |m| {
            (m.start_time, m.duration)
        });
        let id = new_id();
        self.scene.shapes[index].rotation_clips.push(RotationClip {
            id: id.clone(),
            start_time: start,
            duration: length,
            degrees: 360.0,
            ease: "linear".into(),
            extra: Default::default(),
        });
        self.scene.duration = self.scene.duration.max(start + length);
        self.selected_spin = Some(id);
        self.edited();
    }

    /// A half-second fade out from the playhead that doesn't come back (Electron's Add disappearance).
    fn add_fade(&mut self, index: usize) {
        if self.scene.shapes[index].disappear.is_some() {
            return;
        }
        let length = 0.5_f64.min((self.scene.duration - self.time).max(0.0));
        self.scene.shapes[index].disappear = Some(model::Disappear {
            start_time: model::hundredths(self.time),
            duration: model::hundredths(length),
            reappear_mode: "never".into(),
            reappear_time: None,
            trigger_shape_id: None,
            trigger_clip_id: None,
            extra: Default::default(),
        });
        self.edited();
    }

    /// Electron's quick animate bar under the canvas: Move, Spin, Fade and Drop for the selected layer, while
    /// nothing plays or is being drawn.
    fn quick_bar(&mut self, ui: &mut Ui, area: Rect) {
        let Some(index) = self
            .selected
            .as_deref()
            .and_then(|id| self.scene.shapes.iter().position(|s| s.id == id))
        else {
            return;
        };
        if self.playing || self.motion_mode.is_some() || self.scene.shapes[index].is_camera() {
            return;
        }
        let bar = Rect::from_center_size(
            pos2(area.center().x, area.bottom() - 22.0),
            vec2(area.width().min(500.0), 40.0),
        );
        ui.scope_builder(UiBuilder::new().max_rect(bar), |ui| {
            // Darker than the canvas's surroundings, with an edge, so it reads as a bar over them.
            egui::Frame::new()
                .fill(Mood::Ember.pane_solid())
                .stroke(Stroke::new(1.0, color::HAIRLINE))
                .corner_radius(17.0)
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().interact_size.y = 32.0;
                        if bar.width() >= 440.0 {
                            let name: String =
                                self.scene.shapes[index].name.chars().take(18).collect();
                            ui.label(
                                egui::RichText::new(format!("Animate {name}"))
                                    .font(font::ui(12.0))
                                    .color(color::INK_MUTED),
                            );
                        }
                        if ui
                            .button("Move")
                            .on_hover_text("Draw where it should go")
                            .clicked()
                        {
                            (self.motion_mode, self.motion_draft) = (Some("arrow"), None);
                        }
                        if ui
                            .button("Spin")
                            .on_hover_text("A full turn from the playhead")
                            .clicked()
                        {
                            self.add_spin(index);
                        }
                        if ui
                            .button("Fade")
                            .on_hover_text("Fade out from the playhead")
                            .clicked()
                        {
                            self.add_fade(index);
                        }
                        if ui
                            .button("Drop")
                            .on_hover_text("Fall onto the screen from above")
                            .clicked()
                        {
                            self.drop_onto_screen(index);
                        }
                        if self.scene.shapes[index].is_vector() && ui.button("Draw on")
                            .on_hover_text("Reveal the outline from the playhead")
                            .clicked()
                        {
                            let shape = &mut self.scene.shapes[index];
                            let limit = shape.out_time.unwrap_or(self.scene.duration).min(self.scene.duration);
                            let start = model::hundredths(self.time.max(shape.clip_start())).min(limit);
                            let end = model::hundredths(start + 1.0).min(limit);
                            if shape.draw_on(start, end) {
                                self.key_editor.expanded.insert(shape.id.clone());
                                self.tab = Tab::Animate;
                            } else {
                                self.notice = Some("Move the playhead earlier in the layer to draw it on.".into());
                            }
                        }
                    });
                });
        });
    }

    /// Electron's rotation animations: spins added at the playhead (or over the chosen move), then retimed.
    fn spin_panel(&mut self, ui: &mut Ui, index: usize) {
        ui.separator();
        section_label(ui, "Spins");
        if ui
            .button("Add spin")
            .on_hover_text("360° from the playhead, or over the chosen drawn move")
            .clicked()
        {
            self.add_spin(index);
        }
        let mut changed = false;
        let shape = &mut self.scene.shapes[index];
        for (i, clip) in shape.rotation_clips.iter().enumerate() {
            let name = format!(
                "Spin {} · {}° · {:.2}–{:.2} s",
                i + 1,
                clip.degrees,
                clip.start_time,
                clip.start_time + clip.duration
            );
            if ui
                .selectable_label(self.selected_spin.as_ref() == Some(&clip.id), name)
                .clicked()
            {
                self.selected_spin = Some(clip.id.clone());
                (self.time, self.playing) = (clip.start_time, false);
            }
        }
        let Some(at) = shape
            .rotation_clips
            .iter()
            .position(|c| Some(&c.id) == self.selected_spin.as_ref())
        else {
            return;
        };
        let clip = &mut shape.rotation_clips[at];
        egui::Grid::new("forma-spin").num_columns(2).show(ui, |ui| {
            ui.label("Starts at (s)");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut clip.start_time)
                        .range(0.0..=3600.0)
                        .speed(0.01)
                        .fixed_decimals(2),
                )
                .changed();
            ui.end_row();
            ui.label("Length (s)");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut clip.duration)
                        .range(0.1..=3600.0)
                        .speed(0.01)
                        .fixed_decimals(2),
                )
                .changed();
            ui.end_row();
            ui.label("Degrees");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut clip.degrees)
                        .range(-10000.0..=10000.0)
                        .speed(5.0),
                )
                .changed();
            ui.end_row();
            ui.label("Easing");
            egui::ComboBox::from_id_salt("forma-spin-ease")
                .selected_text(if clip.ease == "smooth" {
                    "Smooth"
                } else {
                    "Steady"
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut clip.ease, "linear".into(), "Steady")
                        .changed();
                    changed |= ui
                        .selectable_value(&mut clip.ease, "smooth".into(), "Smooth")
                        .changed();
                });
            ui.end_row();
        });
        if ui
            .checkbox(
                &mut self.live_animation_preview,
                "Preview changes automatically",
            )
            .changed()
            && !self.live_animation_preview
        {
            self.pending_auto_preview = None;
        }
        if changed {
            (clip.start_time, clip.duration) = (
                model::hundredths(clip.start_time),
                model::hundredths(clip.duration),
            );
            let end = clip.start_time + clip.duration;
            self.scene.duration = self.scene.duration.max(end);
            self.edited();
            let target = PreviewTarget::Spin(
                self.scene.shapes[index].id.clone(),
                self.scene.shapes[index].rotation_clips[at].id.clone(),
            );
            self.queue_auto_preview(target, ui.ctx());
        }
        if ui.button("Preview").clicked() {
            self.pending_auto_preview = None;
            let clip = &self.scene.shapes[index].rotation_clips[at];
            self.preview(clip.start_time, clip.start_time + clip.duration);
        }
        if ui.button("Delete spin").clicked() {
            self.pending_auto_preview = None;
            let clip = self.scene.shapes[index].rotation_clips.remove(at);
            (self.selected_spin, self.time) = (None, clip.start_time);
            self.edited();
        }
    }

    /// Electron's disappear: fade the layer out, then bring it back never, at a time, or when another layer's
    /// animation ends.
    fn fade_panel(&mut self, ui: &mut Ui, index: usize) {
        ui.separator();
        section_label(ui, "Fade out and back");
        let Some(mut d) = self.scene.shapes[index].disappear.clone() else {
            if ui
                .button("Add fade out")
                .on_hover_text("Fades out from the playhead")
                .clicked()
            {
                self.add_fade(index);
            }
            return;
        };
        // Animations on other layers it can come back with: their keyframes, drawn moves and spins.
        let mut clips: Vec<(String, String, String, f64)> = Vec::new();
        for other in self
            .scene
            .shapes
            .iter()
            .filter(|s| s.id != self.scene.shapes[index].id)
        {
            if other.keys.len() > 1 {
                clips.push((
                    other.id.clone(),
                    "keys".into(),
                    format!("{} · Keyframes", other.name),
                    other.keys.last().unwrap().t,
                ));
            }
            for (i, m) in motion::motions(other).iter().enumerate() {
                clips.push((
                    other.id.clone(),
                    m.id.clone(),
                    format!(
                        "{} · {} {}",
                        other.name,
                        if m.kind == "path" { "Path" } else { "Move" },
                        i + 1
                    ),
                    m.start_time + m.duration,
                ));
            }
            for (i, c) in other.rotation_clips.iter().enumerate() {
                clips.push((
                    other.id.clone(),
                    c.id.clone(),
                    format!("{} · Rotation {}", other.name, i + 1),
                    c.start_time + c.duration,
                ));
            }
        }
        egui::Grid::new("forma-fade").num_columns(2).show(ui, |ui| {
            ui.label("Starts at (s)");
            ui.add(
                egui::DragValue::new(&mut d.start_time)
                    .range(0.0..=3600.0)
                    .speed(0.01)
                    .fixed_decimals(2),
            );
            ui.end_row();
            ui.label("Length (s)");
            ui.add(
                egui::DragValue::new(&mut d.duration)
                    .range(0.0..=3600.0)
                    .speed(0.01)
                    .fixed_decimals(2),
            );
            ui.end_row();
            ui.label("Comes back");
            let modes = [
                ("never", "Never"),
                ("time", "At a time"),
                ("clip", "With an animation"),
            ];
            egui::ComboBox::from_id_salt("forma-reappear")
                .selected_text(
                    modes
                        .iter()
                        .find(|(m, _)| *m == d.reappear_mode)
                        .map_or("Never", |(_, l)| *l),
                )
                .show_ui(ui, |ui| {
                    for (mode, label) in modes {
                        ui.selectable_value(&mut d.reappear_mode, mode.into(), label);
                    }
                });
            ui.end_row();
        });
        let fade_end = d.start_time + d.duration;
        match d.reappear_mode.as_str() {
            "time" => {
                let mut at = d
                    .reappear_time
                    .unwrap_or((fade_end + 1.0).min(self.scene.duration))
                    .max(fade_end);
                ui.horizontal(|ui| {
                    ui.label("At (s)");
                    ui.add(
                        egui::DragValue::new(&mut at)
                            .range(fade_end..=3600.0)
                            .speed(0.01)
                            .fixed_decimals(2),
                    );
                });
                d.reappear_time = Some(model::hundredths(at));
                (d.trigger_shape_id, d.trigger_clip_id) = (None, None);
            }
            "clip" => {
                let chosen = clips.iter().find(|(s, c, ..)| {
                    Some(s) == d.trigger_shape_id.as_ref() && Some(c) == d.trigger_clip_id.as_ref()
                });
                let text = chosen.map_or(
                    if clips.is_empty() {
                        "Animate another layer first".into()
                    } else {
                        "Choose an animation".to_owned()
                    },
                    |(.., label, end)| format!("{label} · ends {end:.2} s"),
                );
                egui::ComboBox::from_id_salt("forma-reappear-clip")
                    .selected_text(text)
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for (shape, clip, label, end) in &clips {
                            // It has to end after the fade out finishes (Electron refuses one that doesn't).
                            let ok = *end >= fade_end - 0.001;
                            if ui
                                .add_enabled(
                                    ok,
                                    egui::Button::selectable(
                                        false,
                                        format!("{label} · ends {end:.2} s"),
                                    ),
                                )
                                .clicked()
                            {
                                (d.trigger_shape_id, d.trigger_clip_id) =
                                    (Some(shape.clone()), Some(clip.clone()));
                            }
                        }
                    });
                d.reappear_time = None;
            }
            _ => (d.reappear_time, d.trigger_shape_id, d.trigger_clip_id) = (None, None, None),
        }
        (d.start_time, d.duration) = (
            model::hundredths(d.start_time),
            model::hundredths(d.duration),
        );
        if ui
            .checkbox(
                &mut self.live_animation_preview,
                "Preview changes automatically",
            )
            .changed()
            && !self.live_animation_preview
        {
            self.pending_auto_preview = None;
        }
        let shape = &mut self.scene.shapes[index];
        if shape.disappear.as_ref() != Some(&d) {
            self.scene.duration = self
                .scene
                .duration
                .max(d.start_time + d.duration)
                .max(d.reappear_time.unwrap_or(0.0));
            self.scene.shapes[index].disappear = Some(d);
            self.edited();
            self.queue_auto_preview(
                PreviewTarget::Fade(self.scene.shapes[index].id.clone()),
                ui.ctx(),
            );
        }
        if ui.button("Preview").clicked() {
            self.pending_auto_preview = None;
            self.preview_fade(index);
        }
        if ui.button("Delete fade out").clicked() {
            self.pending_auto_preview = None;
            self.scene.shapes[index].disappear = None;
            self.edited();
        }
    }

    /// Electron's drawn moves: draw an arrow or record a path on the canvas from the playhead, then retime, rescale
    /// or delete it.
    fn motion_panel(&mut self, ui: &mut Ui, index: usize) {
        ui.separator();
        section_label(ui, "Draw a move");
        ui.horizontal(|ui| {
            for (label, kind) in [("Draw arrow", "arrow"), ("Record path", "path")] {
                if ui
                    .selectable_label(self.motion_mode == Some(kind), label)
                    .clicked()
                {
                    self.mask_editor.stop();
                    self.motion_mode = if self.motion_mode == Some(kind) {
                        None
                    } else {
                        Some(kind)
                    };
                    self.motion_draft = None;
                    self.playing = false;
                }
            }
        });
        if ui.button("Drop onto the screen").clicked() {
            self.drop_onto_screen(index);
        }
        let help = match self.motion_mode {
            Some("arrow") => "Drag anywhere on the canvas to set the arrow destination.",
            Some(_) => "Hold and drag on the canvas. The selected shape will follow your mouse.",
            None => {
                "Choose a tool, then drag anywhere on the canvas. The motion begins at the playhead."
            }
        };
        ui.label(
            egui::RichText::new(help)
                .font(font::ui(11.0))
                .color(color::INK_MUTED),
        );
        let options = &mut self.motion_options;
        egui::Grid::new("forma-motion-options")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Seconds");
                ui.add(
                    egui::DragValue::new(&mut options.seconds)
                        .range(0.1..=120.0)
                        .speed(0.1)
                        .fixed_decimals(1),
                );
                ui.end_row();
                ui.label("Distance %");
                ui.add(
                    egui::DragValue::new(&mut options.distance)
                        .range(0.0..=500.0)
                        .speed(5.0),
                );
                ui.end_row();
                ui.label("Spin °");
                ui.add(
                    egui::DragValue::new(&mut options.spin)
                        .range(-10000.0..=10000.0)
                        .speed(5.0),
                );
                ui.end_row();
            });
        ui.checkbox(&mut options.face, "Turn to face where it's going");
        let list = motion::motions(&self.scene.shapes[index]);
        for (i, m) in list.iter().enumerate() {
            let name = format!(
                "{} {} · {:.1}–{:.1} s · {}%",
                if m.kind == "path" {
                    "Mouse path"
                } else {
                    "Arrow move"
                },
                i + 1,
                m.start_time,
                m.start_time + m.duration,
                m.distance
            );
            if ui
                .selectable_label(self.selected_motion.as_deref() == Some(&m.id), name)
                .clicked()
            {
                self.selected_motion = Some(m.id.clone());
                self.motion_edit = (
                    m.start_time,
                    m.duration,
                    motion::Options {
                        seconds: m.duration,
                        distance: m.distance,
                        spin: m.spin,
                        face: m.face,
                    },
                );
                self.motion_shown = Some((self.scene.shapes[index].id.clone(), m.clone()));
                (self.time, self.playing) = (m.start_time, false);
            }
        }
        let Some(id) = self
            .selected_motion
            .clone()
            .filter(|id| list.iter().any(|m| &m.id == id))
        else {
            self.motion_shown = None;
            return;
        };
        let current = list.iter().find(|m| m.id == id).unwrap();
        let shape_id = self.scene.shapes[index].id.clone();
        let waiting_on_this = self
            .pending_auto_preview
            .as_ref()
            .is_some_and(|(_, target)| {
                *target == PreviewTarget::Motion(shape_id.clone(), id.clone())
            });
        if !waiting_on_this
            && self.motion_shown.as_ref() != Some(&(shape_id.clone(), current.clone()))
        {
            self.motion_edit = (
                current.start_time,
                current.duration,
                motion::Options {
                    seconds: current.duration,
                    distance: current.distance,
                    spin: current.spin,
                    face: current.face,
                },
            );
            self.motion_shown = Some((shape_id.clone(), current.clone()));
        }
        let edit_before = self.motion_edit;
        let (start, length, options) = &mut self.motion_edit;
        egui::Grid::new("forma-motion-edit")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Starts at (s)");
                ui.add(
                    egui::DragValue::new(start)
                        .range(0.0..=3600.0)
                        .speed(0.01)
                        .fixed_decimals(2),
                );
                ui.end_row();
                ui.label("Length (s)");
                ui.add(
                    egui::DragValue::new(length)
                        .range(0.1..=3600.0)
                        .speed(0.01)
                        .fixed_decimals(2),
                );
                ui.end_row();
                ui.label("Distance %");
                ui.add(
                    egui::DragValue::new(&mut options.distance)
                        .range(0.0..=500.0)
                        .speed(5.0),
                );
                ui.end_row();
                ui.label("Spin °");
                ui.add(
                    egui::DragValue::new(&mut options.spin)
                        .range(-10000.0..=10000.0)
                        .speed(5.0),
                );
                ui.end_row();
            });
        ui.checkbox(&mut options.face, "Turn to face where it's going");
        if ui
            .checkbox(
                &mut self.live_animation_preview,
                "Preview changes automatically",
            )
            .changed()
            && !self.live_animation_preview
        {
            self.pending_auto_preview = None;
        }
        if self.motion_edit != edit_before {
            self.queue_auto_preview(
                PreviewTarget::Motion(shape_id.clone(), id.clone()),
                ui.ctx(),
            );
        }
        let (start, length, options) = self.motion_edit;
        ui.horizontal(|ui| {
            if ui.button("Update").clicked() {
                self.pending_auto_preview = None;
                match motion::edit(&mut self.scene, index, &id, start, length, options) {
                    Ok(()) => {
                        (self.time, self.playing) = (start, false);
                        self.motion_shown = motion::motions(&self.scene.shapes[index])
                            .into_iter()
                            .find(|m| m.id == id)
                            .map(|m| (shape_id.clone(), m));
                        self.edited();
                        if self.live_animation_preview {
                            self.preview(start, start + length);
                        }
                    }
                    Err(error) => self.notice = Some(error),
                }
            }
            if ui.button("Preview").clicked() {
                self.pending_auto_preview = None;
                if let Some(m) = motion::motions(&self.scene.shapes[index])
                    .into_iter()
                    .find(|m| m.id == id)
                {
                    self.preview(m.start_time, m.start_time + m.duration);
                }
            }
            if ui.button("Delete").clicked() {
                motion::remove(&mut self.scene, index, &id);
                self.selected_motion = None;
                self.motion_shown = None;
                self.pending_auto_preview = None;
                self.time = start;
                self.edited();
            }
        });
    }

    /// Zooms the canvas by `factor` (20 %-500 %), keeping the scene point under `at` where it is (Electron's
    /// `setWorkspaceZoom`).
    fn zoom_by(&mut self, factor: f32, at: Pos2, viewport: Rect) {
        let old = self.zoom;
        self.zoom = (self.zoom * factor).clamp(0.2, 5.0);
        let from_center = at - viewport.center();
        self.pan = from_center - (from_center - self.pan) * (self.zoom / old);
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let viewport = ui.max_rect();
        self.vector_tools.sync(self.selected.as_deref(), self.time);
        self.canvas_view(ui);
        self.canvas_bar(ui, viewport);
        if !self.composition_path.is_empty() {
            let rect = Rect::from_min_size(viewport.min + vec2(8.0, 41.0), vec2(viewport.width() - 16.0, 32.0));
            ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                egui::Frame::new().fill(Mood::Ember.pane_solid()).corner_radius(8)
                    .inner_margin(4).show(ui, |ui| self.composition_breadcrumbs(ui));
            });
        }
        if let Some(tool) = self.vector_tools.tool.filter(|_| !self.playing) {
            shape_panel::draw_hint(ui, viewport, tool);
        }
    }

    fn vector_canvas(
        &mut self, ui: &Ui, painter: &egui::Painter, response: &egui::Response,
        view: tiny_skia::Transform, tool: VectorTool,
    ) {
        let (finish, escape, delete) = if self.vector_keyboard {
            ui.input(|i| (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
            ))
        } else {
            (false, false, false)
        };
        let selected = self.selected.as_deref().and_then(|id| {
            let index = self.scene.shapes.iter().position(|shape| shape.id == id)?;
            let shape = &self.scene.shapes[index];
            shape.visible(self.time, self.scene.duration)
                .then(|| (index, self.scene.layer_frame(shape, self.time)))
        });
        if tool == VectorTool::Pen || tool == VectorTool::Subpath {
            let space = if tool == VectorTool::Subpath {
                let Some((_, frame)) = selected else {
                    self.vector_tools.set(None, None);
                    return;
                };
                frame.box_transform().post_concat(view)
            } else {
                view
            };
            let contour = if finish || escape {
                self.vector_tools.pen.finish(false)
            } else {
                self.vector_tools.pen.canvas(ui, painter, response, space)
            };
            if let Some(contour) = contour {
                if tool == VectorTool::Subpath {
                    if let Some((index, frame)) = selected {
                        let shape = &mut self.scene.shapes[index];
                        let at = model::hundredths(self.time).min(self.scene.duration);
                        if let Some(mut path) = shape.path_at(at, [frame.pose.w, frame.pose.h]) {
                            path.contours.push(contour);
                            if path.valid() && let Some(vector) = &mut shape.vector {
                                vector.edit(VectorProperty::Path, VectorValue::Path(path), at);
                            } else {
                                self.notice = Some("This layer has reached the path point limit.".into());
                            }
                        }
                        self.vector_tools.set(Some(VectorTool::Points), self.selected.as_deref());
                    }
                } else {
                    self.add_pen_layer(contour);
                }
            } else if escape {
                self.vector_tools.set(None, None);
            }
            return;
        }
        if escape {
            self.vector_tools.set(None, None);
            return;
        }
        let Some((index, frame)) = selected else {
            self.vector_tools.set(None, None);
            return;
        };
        let shape = &mut self.scene.shapes[index];
        let local = frame.box_transform().post_concat(view);
        let at = model::hundredths(self.time).min(self.scene.duration);
        match tool {
            VectorTool::Points => {
                if let Some(path) = shape.path_at(at, [frame.pose.w, frame.pose.h]) {
                    self.vector_tools.edit_points(ui, painter, response, shape, path, local, at, delete);
                }
            }
            VectorTool::Gradient(target) => {
                self.vector_tools.gradient_canvas(ui, painter, response, shape, target, local, at);
            }
            _ => {}
        }
    }

    fn add_pen_layer(&mut self, contour: crate::paths::Contour) {
        if self.scene.shapes.len() >= 500 {
            self.notice = Some("This scene already has 500 layers.".into());
            return;
        }
        let Some(bounds) = crate::path_editor::shape_bounds(&contour) else { return; };
        let closed = contour.closed;
        let path = crate::paths::PathData { contours: vec![crate::path_editor::normalize(contour, bounds)] };
        if !path.valid() {
            return;
        }
        let id = self.scene.add_shape(new_id(), "path");
        if let Some(shape) = self.scene.shapes.last_mut() {
            shape.name = "Path".into();
            if let Some(key) = shape.keys.first_mut() {
                key.x = bounds.center().x.clamp(-10_000.0, 10_000.0);
                key.y = bounds.center().y.clamp(-10_000.0, 10_000.0);
                key.w = bounds.width().clamp(1.0, 10_000.0);
                key.h = bounds.height().clamp(1.0, 10_000.0);
            }
            let color = crate::render::rgb(&shape.color).map(f32::from);
            shape.vector = Some(VectorLayer {
                path: Some(path),
                fill: (!closed).then(|| PaintStyle::new(PaintMode::None)),
                stroke: (!closed).then(|| StrokeStyle {
                    rgba: [color[0], color[1], color[2], 100.0],
                    ..StrokeStyle::default()
                }),
                ..VectorLayer::default()
            });
        }
        self.select_layer(Some(id.clone()), false);
        self.tab = Tab::Layer;
        self.vector_tools.set(Some(VectorTool::Points), Some(&id));
    }

    /// Electron's canvas bar, over the top of the canvas: the scene's shape (opens Scene settings), a hint, and zoom.
    fn canvas_bar(&mut self, ui: &mut Ui, viewport: Rect) {
        let bar = Rect::from_min_size(
            viewport.min + vec2(8.0, 8.0),
            vec2(viewport.width() - 16.0, 26.0),
        );
        // The buttons get a dark backing, drawn under them once their places are known: zoomed in, the canvas lies
        // behind the bar, and their light text would vanish on a light scene.
        let backing = [ui.painter().add(egui::Shape::Noop), ui.painter().add(egui::Shape::Noop)];
        let back = |ui: &Ui, slot: egui::layers::ShapeIdx, rect: Rect| {
            ui.painter().set(slot, egui::epaint::RectShape::filled(rect.expand(2.0), 8, Color32::from_rgba_unmultiplied(24, 22, 16, 235)));
        };
        ui.scope_builder(UiBuilder::new().max_rect(bar), |ui| {
            ui.horizontal(|ui| {
                let (w, h) = (self.scene.width, self.scene.height);
                let shape = if w < h {
                    "Portrait"
                } else if w > h {
                    "Landscape"
                } else {
                    "Square"
                };
                let playing = if self.playing { " · Playing" } else { "" };
                // A narrow canvas (a split pane) keeps just the size, so the zoom buttons still fit beside it.
                let label = if bar.width() < 330.0 {
                    format!("{w} \u{00d7} {h}")
                } else {
                    format!("{shape} \u{00b7} {w} \u{00d7} {h}{playing}")
                };
                let size = ui
                    .add(egui::Button::new(label).wrap_mode(egui::TextWrapMode::Extend))
                    .on_hover_text("Canvas size, length, and background");
                let mut background = size.rect;
                if self.scene.shapes.iter().any(Shape::is_3d) || self.camera_tool {
                    let camera = ui
                        .add(
                            egui::Button::selectable(self.camera_tool, "Camera")
                                .min_size(vec2(56.0, 32.0)),
                        )
                        .on_hover_text(
                            "Drag to orbit the camera. Shift-drag or middle-drag to pan.",
                        );
                    background = background.union(camera.rect);
                    if camera.clicked() {
                        self.camera_tool = !self.camera_tool;
                        if self.camera_tool {
                            if !self.scene.shapes.iter().any(Shape::is_camera) {
                                self.add_camera();
                            }
                            self.mask_editor.stop();
                            self.vector_tools.set(None, None);
                            (self.motion_mode, self.motion_draft) = (None, None);
                            self.gesture = None;
                            self.playing = false;
                        }
                    }
                }
                back(ui, backing[0], background);
                if size.clicked() {
                    self.open_scene = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let more = ui.small_button("+").on_hover_text("Zoom in");
                    if more.clicked() {
                        self.zoom_by(1.25, viewport.center(), viewport);
                    }
                    if ui
                        .add(
                            egui::Button::new(format!("{:.0}%", self.zoom * 100.0))
                                .small()
                                .wrap_mode(egui::TextWrapMode::Extend),
                        )
                        .on_hover_text("Reset zoom and position (Ctrl+0)")
                        .clicked()
                    {
                        (self.zoom, self.pan) = (1.0, Vec2::ZERO);
                    }
                    let less = ui.small_button("−").on_hover_text("Zoom out");
                    if less.clicked() {
                        self.zoom_by(0.8, viewport.center(), viewport);
                    }
                    back(ui, backing[1], more.rect.union(less.rect));
                    if ui.available_width() > 330.0 {
                        ui.label(
                            egui::RichText::new(
                                "Scroll to zoom · middle-drag to pan · only the canvas is exported",
                            )
                            .font(font::ui(11.0))
                            .color(color::INK_MUTED),
                        );
                    }
                });
            });
        });
    }

    fn canvas_view(&mut self, ui: &mut Ui) {
        let viewport = ui.max_rect();
        let area = viewport.shrink(14.0);
        // As Electron sizes its artboard: at 100 % it fits the space, but never past one point a scene pixel.
        let fit = self
            .scene
            .width
            .min((viewport.width() - 24.0).max(200.0) * 0.92)
            .min(
                (viewport.height() - 24.0).max(180.0) * 0.92 * self.scene.width / self.scene.height,
            )
            / self.scene.width;
        // Scroll to zoom, the zoom buttons, middle-drag to pan.
        // Only over the canvas itself: not through a menu or dialog lying on top of it.
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| viewport.contains(*p) && ui.ctx().layer_id_at(*p) == Some(ui.layer_id()));
        if let Some(p) = pointer {
            let factor = ui.input(|i| i.zoom_delta() * (i.smooth_scroll_delta.y * 0.00375).exp());
            if (factor - 1.0).abs() > 1e-4 {
                self.zoom_by(factor, p, viewport);
            }
        }
        let scale = (fit * self.zoom).max(0.01);
        let canvas = Rect::from_center_size(
            viewport.center() + self.pan,
            vec2(self.scene.width * scale, self.scene.height * scale),
        );
        if self
            .renderer
            .as_ref()
            .is_none_or(|r| !r.matches(&self.scene))
        {
            self.frame_cache.invalidate();
            self.renderer = Some(render::Renderer::new(&self.scene, &self.place));
        }
        let Some(renderer) = self.renderer.as_ref() else {
            return;
        };
        // The frame and its surroundings (Electron's pasteboard: 55 % of the longer side around it), as far as they're
        // in view, drawn at the screen's pixel size, again only when the scene, the time or the view changes.
        let pixels = ui.ctx().pixels_per_point();
        let margin = (self.scene.width.max(self.scene.height) * 0.55).round() * scale;
        let board = canvas.expand(margin);
        let shown = board.intersect(viewport);
        let snapped = Rect::from_min_max(
            (shown.min.to_vec2() * pixels).round().to_pos2() / pixels,
            (shown.max.to_vec2() * pixels).round().to_pos2() / pixels,
        );
        let pixel_scale = scale * pixels;
        let full_view = (
            (
                (snapped.left() - canvas.left()) / scale,
                (snapped.top() - canvas.top()) / scale,
            ),
            pixel_scale,
            (
                (snapped.width() * pixels).round().max(1.0) as u32,
                (snapped.height() * pixels).round().max(1.0) as u32,
            ),
        );
        let clip = self.playing;
        if !clip {
            self.playing_at_half = false;
        }
        let quality = if self.playing_at_half && self.quality == crate::preview_cache::Quality::Full
        {
            crate::preview_cache::Quality::Half
        } else {
            self.quality
        };
        let view = quality.view(full_view);
        let time = if clip {
            ((self.time * 30.0 + 1e-8).floor() / 30.0).min(self.scene.duration)
        } else {
            self.time
        };
        if self
            .videos
            .update(&self.scene, renderer, time, clip, view.1)
        {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        let generation = self.videos.generation;
        // While a clip on a track is being edited, the clip before it shows faintly beneath everything.
        let ghost = self
            .selected
            .as_deref()
            .filter(|_| self.onion_skin && !self.playing)
            .and_then(|id| self.scene.shapes.iter().position(|s| s.id == id))
            .and_then(|index| self.scene.onion_skin(index, self.time));
        let display = match ghost {
            Some(ghost) => {
                let mut scene = self.scene.clone();
                scene.shapes.insert(0, ghost);
                std::borrow::Cow::Owned(scene)
            }
            None => std::borrow::Cow::Borrowed(&self.scene),
        };
        let scene_changed = self
            .preview
            .as_ref()
            .is_some_and(|(_, scene, ..)| *scene != *display);
        if scene_changed {
            self.frame_cache.invalidate();
            renderer.invalidate_preview_pixels();
        }
        let stale = scene_changed
            || renderer.preview_is_waiting()
            || self
                .preview
                .as_ref()
                .is_none_or(|(_, _, drawn_time, drawn, shown)| {
                    *drawn != (view, clip) || *drawn_time != time || *shown != generation
                });
        if stale {
            let start = Instant::now();
            let key = (time, view, clip);
            let cacheable = clip
                && !renderer.has_nested_video()
                && !(display.motion_blur_preview
                    && display
                        .shapes
                        .iter()
                        .any(|shape| shape.kind == "video" && shape.motion_blur));
            let cached = if cacheable {
                self.frame_cache.get(&key)
            } else {
                None
            };
            let pixels = if let Some((pixels, hits)) = cached {
                renderer.restore_text_hits(hits);
                Some(pixels)
            } else {
                let pixels = renderer
                    .view_with_samples(&display, time, view, clip, &self.videos.frames, |sample| {
                        self.videos.sample(sample)
                    })
                    .map(std::sync::Arc::new);
                self.frame_time_ms = start.elapsed().as_secs_f64() * 1000.0;
                if let Some(pixels) = pixels.as_ref().filter(|_| {
                    cacheable
                        && !renderer.preview_is_waiting()
                        && self.videos.have_playback_frames(&self.scene, time)
                }) {
                    let hits = renderer.text_hits_snapshot();
                    let bytes = pixels.data().len()
                        + hits
                            .iter()
                            .map(|(key, rects)| {
                                key.len() + rects.len() * std::mem::size_of::<tiny_skia::Rect>()
                            })
                            .sum::<usize>();
                    self.frame_cache.insert(key, (pixels.clone(), hits), bytes);
                }
                if clip
                    && quality == crate::preview_cache::Quality::Full
                    && self.frame_time_ms > 1000.0 / 30.0
                {
                    self.playing_at_half = true;
                    ui.ctx().request_repaint();
                }
                pixels
            };
            if let Some(pixmap) = pixels {
                let image = egui::ColorImage::from_rgba_premultiplied(
                    [pixmap.width() as usize, pixmap.height() as usize],
                    pixmap.data(),
                );
                if let Some((texture, scene, drawn_time, drawn, shown)) = &mut self.preview {
                    texture.set(image, TextureOptions::LINEAR);
                    if scene_changed {
                        *scene = display.into_owned();
                    }
                    (*drawn_time, *drawn, *shown) = (time, (view, clip), generation);
                } else {
                    let texture =
                        ui.ctx()
                            .load_texture("forma-preview", image, TextureOptions::LINEAR);
                    self.preview = Some((
                        texture,
                        display.into_owned(),
                        time,
                        (view, clip),
                        generation,
                    ));
                }
            }
        }
        if renderer.preview_is_waiting() {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        // Selection handles can reach past the canvas, but not over the toolbar or the panels.
        let painter = ui.painter().with_clip_rect(viewport);
        painter.add(pane_frame(Mood::Ember).paint(viewport));
        painter.rect_filled(board, 6.0, color::VEIL);
        painter.rect_filled(canvas.expand(1.0), 2.0, Color32::from_black_alpha(90));
        match &self.preview {
            Some((texture, ..)) => painter.image(
                texture.id(),
                snapped,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            ),
            None => painter.rect_filled(canvas, 3.0, hex(&self.scene.bg, 255)),
        };
        let to_screen = |x: f32, y: f32| canvas.min + vec2(x, y) * scale;
        let to_scene = |p: Pos2| (p - canvas.min) / scale;
        let from_layer = |frame: LayerFrame, x, y| {
            let (x, y) = transform::point(frame.transform, x, y);
            to_screen(x, y)
        };
        let corners = |frame: LayerFrame, height: f32| {
            [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
                .map(|(x, y)| from_layer(frame, x * frame.pose.w, y * height))
        };
        let overlays = if self.playing {
            std::borrow::Cow::Borrowed(&self.scene)
        } else {
            crate::expressions::at_time(&self.scene, time)
        };
        let height_factors: Vec<_> = overlays
            .shapes
            .iter()
            .map(|shape| crate::text_render::selection_height_factor(shape, time))
            .collect();
        let camera = crate::spatial::Camera::at(&overlays, time);
        let mut hits = Vec::new();
        let mut depths = Vec::new();
        let mut projected = std::collections::HashMap::new();
        let mattes = MatteLinks::new(&overlays);
        for (index, shape) in overlays.shapes.iter().enumerate() {
            if shape.is_camera() || !shape.visible(time, overlays.duration) {
                continue;
            }
            if mattes.as_ref().is_some_and(|mattes| mattes.hidden[index]) {
                continue;
            }
            let frame = overlays.layer_frame(shape, time);
            if shape.is_3d() {
                let plane = crate::spatial::Plane {
                    scene: &overlays,
                    shape,
                    frame,
                    time,
                };
                let map = |x, y, z| {
                    let (x, y) = transform::point(frame.transform, x, y);
                    camera
                        .project(plane.world([x, y, z]))
                        .map(|[x, y]| to_screen(x, y))
                };
                let quad: Option<Vec<_>> = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
                    .into_iter()
                    .map(|(x, y)| {
                        map(
                            x * frame.pose.w,
                            y * frame.pose.h * height_factors[index],
                            0.0,
                        )
                    })
                    .collect();
                if let Some(quad) = quad {
                    if !self.playing && self.multi_selected.contains(&shape.id) {
                        painter.add(egui::Shape::closed_line(
                            quad.clone(),
                            Stroke::new(1.5, color::SUN),
                        ));
                    }
                    projected.insert(index, quad);
                }
                if !self.playing && self.selected.as_deref() == Some(shape.id.as_str()) {
                    let (x, y) = frame.anchor();
                    if let Some([x, y]) = camera.project(plane.world([x, y, 0.0])) {
                        let origin = to_screen(x, y);
                        let length = 44.0 / scale;
                        let (a, b) = shape.anchor.unwrap_or_default().offset_in(frame.pose);
                        for (axis, endpoint, tint) in [
                            (
                                "X",
                                map(a + length, b, 0.0),
                                canopy_ui::Accent::Clay.color(),
                            ),
                            ("Y", map(a, b + length, 0.0), color::LEAF),
                            ("Z", map(a, b, length), canopy_ui::Accent::Sky.color()),
                        ] {
                            if let Some(point) = endpoint {
                                if point.distance(origin) < 4.0 {
                                    painter.circle_stroke(origin, 6.0, Stroke::new(1.5, tint));
                                    painter.text(
                                        origin + vec2(9.0, -9.0),
                                        Align2::CENTER_CENTER,
                                        axis,
                                        font::ui(11.0),
                                        tint,
                                    );
                                } else {
                                    painter.line_segment([origin, point], Stroke::new(2.0, tint));
                                    painter.text(
                                        point,
                                        Align2::CENTER_CENTER,
                                        axis,
                                        font::ui(11.0),
                                        tint,
                                    );
                                }
                            }
                        }
                        painter.circle_filled(origin, 3.5, color::SUN);
                    }
                }
                depths.push(Some(camera.depth(plane.center())));
            } else {
                depths.push(None);
            }
            if renderer.leaves_out(shape) {
                painter.add(egui::Shape::convex_polygon(
                    corners(frame, frame.pose.h * height_factors[index]).to_vec(),
                    hex(
                        &shape.color_at(self.time),
                        (frame.pose.o * 2.55).round() as u8,
                    ),
                    Stroke::NONE,
                ));
                painter.text(
                    from_layer(frame, 0.0, 0.0),
                    Align2::CENTER_CENTER,
                    format!("{} layer", shape.kind),
                    font::ui(12.0),
                    Color32::BLACK,
                );
            }
            hits.push((index, frame));
            if !self.playing
                && !shape.is_3d()
                && self.multi_selected.contains(&shape.id)
                && self.selected.as_deref() != Some(&shape.id)
            {
                painter.add(egui::Shape::closed_line(
                    corners(frame, frame.pose.h * height_factors[index]).to_vec(),
                    Stroke::new(1.5, color::SUN),
                ));
            }
        }
        let order = crate::spatial::depth_order(&depths);
        let hits: Vec<_> = order
            .into_iter()
            .filter_map(|index| hits.get(index).copied())
            .collect();
        let selected = self.selected.as_deref().and_then(|id| {
            let index = overlays.shapes.iter().position(|shape| shape.id == id)?;
            let shape = &overlays.shapes[index];
            (!shape.is_3d() && shape.visible(time, overlays.duration))
                .then(|| (index, overlays.layer_frame(shape, time)))
        });
        let on_path = |shape: &Shape| {
            shape
                .text_layout
                .as_ref()
                .is_some_and(|text| text.path.is_some())
        };
        let selected_path = selected.filter(|&(index, _)| on_path(&self.scene.shapes[index]));
        let mut handles = Vec::new();
        let (mut rotate_at, mut anchor_at) = (None, None);
        if let (Some((index, _)), false, false, None) = (
            selected_path,
            self.playing,
            self.mask_editor.active(),
            self.vector_tools.tool,
        ) {
            if let Some(bounds) = renderer.text_bounds(&self.scene.shapes[index].id) {
                let min = snapped.min + vec2(bounds.left(), bounds.top()) / (view.1 / scale);
                let max = snapped.min + vec2(bounds.right(), bounds.bottom()) / (view.1 / scale);
                painter.rect_stroke(
                    Rect::from_min_max(min, max),
                    0.0,
                    Stroke::new(1.5, color::SUN),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if let (Some((index, frame)), false, false, None) = (
            selected.filter(|_| selected_path.is_none()),
            self.playing,
            self.mask_editor.active(),
            self.vector_tools.tool,
        ) {
            let w = frame.pose.w;
            let h = frame.pose.h * height_factors[index];
            let center = from_layer(frame, 0.0, 0.0);
            let x_axis = (from_layer(frame, w, 0.0) - center).normalized();
            let y_axis = (from_layer(frame, 0.0, h) - center).normalized();
            let size = vec2(
                center.distance(from_layer(frame, w, 0.0)),
                center.distance(from_layer(frame, 0.0, h)),
            );
            let knob = (size.x.min(size.y) * 0.28).clamp(8.0, 12.0);
            let reach = (size.x.min(size.y) * 0.5).clamp(12.0, 28.0);
            painter.add(egui::Shape::closed_line(
                corners(frame, h).to_vec(),
                Stroke::new(1.5, color::SUN),
            ));
            for (sx, sy) in HANDLES {
                if (sx == 0 && size.x < 48.0) || (sy == 0 && size.y < 48.0) {
                    continue;
                }
                let at = from_layer(frame, f32::from(sx) * w / 2.0, f32::from(sy) * h / 2.0);
                let square = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .map(|(x, y)| at + (x_axis * x + y_axis * y) * knob / 2.0);
                painter.add(egui::Shape::convex_polygon(
                    square.to_vec(),
                    Color32::WHITE,
                    Stroke::new(1.0, color::SUN),
                ));
                handles.push(((sx, sy), at, reach));
            }
            let top = from_layer(frame, 0.0, -h / 2.0);
            let knob_at = top - y_axis * 34.0;
            painter.line_segment([top, knob_at], Stroke::new(1.5, color::SUN));
            painter.circle(knob_at, 10.0, Color32::WHITE, Stroke::new(1.5, color::SUN));
            painter.circle_stroke(knob_at, 4.5, Stroke::new(1.5, color::SUN));
            rotate_at = Some(knob_at);
            let (x, y) = frame.anchor();
            let at = to_screen(x, y);
            let outline = Stroke::new(1.5, color::SUN);
            painter.circle(at, 5.0, Mood::Ember.ground(), outline);
            for axis in [vec2(1.0, 0.0), vec2(0.0, 1.0)] {
                painter.line_segment([at - axis * 9.0, at + axis * 9.0], outline);
            }
            anchor_at = Some(at);
        }
        // All of the view takes clicks and drags: layers can be picked up outside the frame.
        let response = ui.interact(
            viewport,
            ui.scope_id().with("forma-canvas"),
            Sense::click_and_drag(),
        );
        if self.camera_tool {
            let middle = response.dragged_by(egui::PointerButton::Middle);
            let primary = response.dragged_by(egui::PointerButton::Primary);
            if !self.playing && (middle || primary) {
                let pan = middle || ui.input(|input| input.modifiers.shift);
                let delta = ui.input(|input| input.pointer.delta());
                self.camera_drag(delta, pan, scale);
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            return;
        }
        // The middle button pans, whatever it's over (Electron's workspace pan).
        if ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle)) {
            if response.dragged_by(egui::PointerButton::Middle) {
                self.pan += response.drag_delta();
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            return;
        }
        if !self.playing && let Some(tool) = self.vector_tools.tool {
            if response.clicked() || response.drag_started() {
                self.key_editor.clear_selection();
            }
            let view = tiny_skia::Transform::from_scale(scale, scale)
                .post_translate(canvas.left(), canvas.top());
            self.vector_canvas(ui, &painter, &response, view, tool);
            return;
        }
        if !self.playing
            && let Some((index, frame)) = selected
        {
            if self.mask_editor.active() && (response.clicked() || response.drag_started()) {
                self.key_editor.clear_selection();
            }
            let view = tiny_skia::Transform::from_scale(scale, scale)
                .post_translate(canvas.left(), canvas.top());
            self.mask_editor.canvas(
                ui,
                &painter,
                &response,
                &mut self.scene.shapes[index],
                frame,
                view,
                self.time,
                self.vector_keyboard,
            );
        } else {
            self.mask_editor.cancel_gesture();
        }
        if self.mask_editor.active() {
            return;
        }
        let motion_space = selected.map_or(tiny_skia::Transform::identity(), |(index, _)| {
            self.scene
                .motion_space(&self.scene.shapes[index], self.time)
        });
        let motion_to_screen = |p: motion::Point| {
            let (x, y) = transform::point(motion_space, p.x, p.y);
            to_screen(x, y)
        };
        // The selected move's path, and one being drawn.
        if let Some(m) = self
            .selected
            .as_deref()
            .and_then(|id| self.scene.shapes.iter().find(|s| s.id == id))
            .and_then(|s| {
                motion::motions(s)
                    .into_iter()
                    .find(|m| Some(&m.id) == self.selected_motion.as_ref())
            })
        {
            let points: Vec<Pos2> = motion::scaled_points(&m)
                .into_iter()
                .map(motion_to_screen)
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(2.0, color::SUN.gamma_multiply(0.7)),
            ));
        }
        if let Some((start, points)) = &self.motion_draft {
            let mut line = vec![motion_to_screen(motion::Point {
                x: start.x,
                y: start.y,
            })];
            line.extend(points.iter().copied().map(motion_to_screen));
            if self.motion_mode == Some("arrow") {
                line = vec![line[0], *line.last().unwrap()];
            }
            painter.add(egui::Shape::line(
                line.clone(),
                Stroke::new(2.5, color::SUN),
            ));
            if let [.., a, b] = line[..] {
                let back = (a - b).normalized() * 12.0;
                let side = vec2(-back.y, back.x) * 0.5;
                painter.add(egui::Shape::convex_polygon(
                    vec![b, b + back + side, b + back - side],
                    color::SUN,
                    Stroke::NONE,
                ));
            }
        }
        if let Some(kind) = self.motion_mode {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            let Some(inverse) = motion_space.invert() else {
                return;
            };
            let motion_point = |p: Pos2| {
                let p = to_scene(p);
                let (x, y) = transform::point(inverse, p.x, p.y);
                motion::Point { x, y }
            };
            let index = self
                .selected
                .as_deref()
                .and_then(|id| self.scene.shapes.iter().position(|s| s.id == id));
            if let (true, Some(index), Some(p)) = (
                response.drag_started(),
                index,
                ui.input(|i| i.pointer.press_origin()),
            ) {
                self.motion_draft = Some((
                    self.scene.shapes[index].lane_pose(self.time),
                    vec![motion_point(p)],
                ));
            }
            if let (true, Some((_, points)), Some(p)) = (
                response.dragged(),
                self.motion_draft.as_mut(),
                response.interact_pointer_pos(),
            ) {
                let p = motion_point(p);
                let last = *points.last().unwrap();
                if kind == "arrow" {
                    *points.last_mut().unwrap() = p;
                } else if (p.x - last.x).hypot(p.y - last.y) > 3.0 {
                    points.push(p);
                }
            }
            if response.drag_stopped() {
                if let (Some((start, mut points)), Some(index)) = (self.motion_draft.take(), index)
                {
                    if kind == "path" {
                        points.insert(
                            0,
                            motion::Point {
                                x: start.x,
                                y: start.y,
                            },
                        );
                    }
                    match motion::draw(
                        &mut self.scene,
                        index,
                        kind,
                        start,
                        &points,
                        self.time,
                        self.motion_options,
                        new_id(),
                    ) {
                        Ok(id) => {
                            let m = motion::motions(&self.scene.shapes[index])
                                .into_iter()
                                .find(|m| m.id == id)
                                .unwrap();
                            self.motion_edit = (
                                m.start_time,
                                m.duration,
                                motion::Options {
                                    seconds: m.duration,
                                    ..self.motion_options
                                },
                            );
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
            handles
                .iter()
                .find(|(_, at, reach)| at.distance(p) <= reach / 2.0)
                .map(|(handle, ..)| *handle)
        };
        let on_rotate = |p: Pos2| rotate_at.is_some_and(|at| at.distance(p) <= 12.0);
        let on_anchor = |p: Pos2| anchor_at.is_some_and(|at| at.distance(p) <= 11.0);
        let selected_id = self.selected.clone();
        let topmost =
            |scene: &Scene, time: f64, screen: Pos2, renderer: Option<&render::Renderer>| {
                let p = to_scene(screen);
                hits.iter()
                    .rev()
                    .find(|(index, frame)| {
                        let shape = &scene.shapes[*index];
                        if shape.is_3d() {
                            return projected
                                .get(index)
                                .is_some_and(|quad| point_in_quad(screen, quad));
                        }
                        if shape.kind == "adjustment"
                            && selected_id.as_deref() != Some(shape.id.as_str())
                        {
                            return false;
                        }
                        let pixel = (screen - snapped.min) * (view.1 / scale);
                        if shape.kind == "text"
                            && renderer.is_some_and(|r| r.text_hit(&shape.id, pixel.x, pixel.y))
                        {
                            return true;
                        }
                        if on_path(shape) {
                            return false;
                        }
                        if shape.kind == "path"
                            && let Some(path) = shape.path_at(time, [frame.pose.w, frame.pose.h])
                            && let Some(vector) = &shape.vector
                        {
                            let view = tiny_skia::Transform::from_scale(scale, scale)
                                .post_translate(canvas.left(), canvas.top());
                            let fill = vector
                                .fill
                                .as_ref()
                                .is_none_or(|fill| fill.mode != PaintMode::None);
                            let place = frame.transform.post_concat(view);
                            let width = vector.stroke_at(time, &shape.ease).map_or(0.0, |stroke| {
                                stroke.width * transform::max_scale(place) / 2.0
                            });
                            return crate::path_editor::hits(
                                &path,
                                frame.box_transform().post_concat(view),
                                screen,
                                fill,
                                width,
                            );
                        }
                        frame.transform.invert().is_some_and(|inverse| {
                            let (x, y) = transform::point(inverse, p.x, p.y);
                            x.abs() <= frame.pose.w / 2.0
                                && y.abs() <= frame.pose.h * height_factors[*index] / 2.0
                        })
                    })
                    .map(|(index, _)| *index)
            };
        let menu_target = response
            .interact_pointer_pos()
            .and_then(|p| topmost(&self.scene, self.time, p, self.renderer.as_ref()));
        if response.double_clicked() && let Some(index) = menu_target {
            let id = self.scene.shapes[index].id.clone();
            self.queue_composition(&id);
        }
        if response.secondary_clicked() {
            let id = menu_target.map(|i| self.scene.shapes[i].id.clone());
            self.select_layer(id, false);
        }
        if let Some(index) = self
            .selected
            .as_deref()
            .and_then(|id| self.scene.shapes.iter().position(|s| s.id == id))
        {
            response.context_menu(|ui| self.follows_picker(ui, index, true));
        }
        let alt = ui.input(|i| i.modifiers.alt);
        if let Some(p) = response.hover_pos() {
            if alt && on_anchor(p) {
                response.clone().on_hover_text("Drag the anchor");
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            } else if on_rotate(p) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            } else if let (Some((sx, sy)), Some((_, frame))) = (near_handle(p), selected) {
                let (dx, dy) = transform::vector(frame.transform, f32::from(sx), f32::from(sy));
                let angle = dy.atan2(dx).to_degrees().rem_euclid(180.0);
                ui.ctx().set_cursor_icon(
                    [
                        egui::CursorIcon::ResizeHorizontal,
                        egui::CursorIcon::ResizeNwSe,
                        egui::CursorIcon::ResizeVertical,
                        egui::CursorIcon::ResizeNeSw,
                    ][(angle / 45.0).round() as usize % 4],
                );
            } else if let Some(index) = topmost(&self.scene, self.time, p, self.renderer.as_ref()) {
                if on_path(&self.scene.shapes[index]) {
                    response
                        .clone()
                        .on_hover_text("Change Path offset in the Layer tab to move this text.");
                } else {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                }
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary) || response.clicked() {
            if let Some(p) = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos())
            {
                let point = to_scene(p).to_pos2();
                self.gesture = None;
                if let Some((index, frame)) = selected.filter(|_| alt && on_anchor(p)) {
                    if let Some(inverse) = frame.transform.invert() {
                        let shape = self.scene.shapes[index].clone();
                        self.gesture = Some(Gesture::Anchor {
                            start: point,
                            initial: shape.anchor.unwrap_or_default(),
                            inverse,
                            size: vec2(frame.pose.w, frame.pose.h),
                            shape: Box::new(shape),
                        });
                    }
                } else if let Some((index, frame)) = selected.filter(|_| on_rotate(p)) {
                    if let Some(inverse) = frame.space.invert() {
                        let center = pos2(frame.pose.x, frame.pose.y);
                        let (x, y) = transform::point(inverse, point.x, point.y);
                        self.gesture = Some(Gesture::Rotate {
                            id: self.scene.shapes[index].id.clone(),
                            center,
                            inverse,
                            last: (y - center.y).atan2(x - center.x).to_degrees(),
                        });
                    }
                } else if let (Some(handle), Some((index, frame))) = (near_handle(p), selected) {
                    if let Some(inverse) = frame.transform.invert() {
                        let shape = &self.scene.shapes[index];
                        self.gesture = Some(Gesture::Resize {
                            id: shape.id.clone(),
                            handle,
                            start: point,
                            pose: self.scene.editable_pose(shape, self.time),
                            inverse,
                            anchor: shape.anchor.unwrap_or_default(),
                            height_factor: height_factors[index],
                        });
                    }
                } else {
                    let hit = topmost(&self.scene, self.time, p, self.renderer.as_ref())
                        .map(|i| self.scene.shapes[i].id.clone());
                    let extend = ui.input(|i| i.modifiers.shift);
                    if extend && response.clicked() {
                        self.select_layer(hit, true);
                    } else if !hit
                        .as_ref()
                        .is_some_and(|id| self.multi_selected.contains(id))
                        || response.clicked()
                    {
                        self.select_layer(hit, false);
                    }
                    if let Some(i) = self
                        .selected
                        .as_deref()
                        .and_then(|id| self.scene.shapes.iter().position(|s| s.id == id))
                    {
                        let grouped = self
                            .selected
                            .as_ref()
                            .is_some_and(|id| self.multi_selected.contains(id));
                        let mut poses: Vec<_> = self
                            .scene
                            .shapes
                            .iter()
                            .enumerate()
                            .filter(|(j, shape)| {
                                !on_path(shape)
                                    && !shape.is_3d()
                                    && (*j == i
                                        || (grouped && self.multi_selected.contains(&shape.id)))
                            })
                            .map(|(j, shape)| {
                                (
                                    self.scene.ancestor_depth(j),
                                    shape.id.clone(),
                                    self.scene.editable_pose(shape, self.time),
                                )
                            })
                            .collect();
                        poses.sort_by_key(|(depth, ..)| *depth);
                        if !poses.is_empty() {
                            self.gesture = Some(Gesture::Move {
                                start: point,
                                poses: poses.into_iter().map(|(_, id, pose)| (id, pose)).collect(),
                            });
                        }
                    }
                }
                if response.clicked() {
                    self.gesture = None;
                }
            }
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            if let (Some(gesture), Some(p)) =
                (self.gesture.clone(), response.interact_pointer_pos())
            {
                let point = to_scene(p).to_pos2();
                let lock_ratio = self.lock_ratio && !ui.input(|i| i.modifiers.shift);
                match gesture {
                    Gesture::Move { start, poses } => {
                        for (id, pose) in poses {
                            let Some(index) =
                                self.scene.shapes.iter().position(|shape| shape.id == id)
                            else {
                                continue;
                            };
                            let moved = Pose {
                                x: pose.x + point.x - start.x,
                                y: pose.y + point.y - start.y,
                                ..pose
                            };
                            self.scene.set_property_pose(
                                index,
                                self.time,
                                moved,
                                Property::Position,
                            );
                        }
                    }
                    Gesture::Resize {
                        id,
                        handle,
                        start,
                        pose,
                        inverse,
                        anchor,
                        height_factor,
                    } => {
                        let Some(index) = self.scene.shapes.iter().position(|shape| shape.id == id)
                        else {
                            self.gesture = None;
                            return;
                        };
                        let delta = point - start;
                        let (dx, dy) = transform::vector(inverse, delta.x, delta.y);
                        let (w, h) = transform::resize_about_anchor(
                            pose,
                            anchor,
                            handle,
                            (dx, dy),
                            height_factor,
                            lock_ratio,
                        );
                        self.scene.set_property_pose(
                            index,
                            self.time,
                            Pose { w, h, ..pose },
                            Property::Size,
                        );
                    }
                    Gesture::Anchor {
                        start,
                        initial,
                        inverse,
                        size,
                        shape,
                    } => {
                        let Some(index) = self
                            .scene
                            .shapes
                            .iter()
                            .position(|live| live.id == shape.id)
                        else {
                            self.gesture = None;
                            return;
                        };
                        let delta = point - start;
                        let (dx, dy) = transform::vector(inverse, delta.x, delta.y);
                        let anchor = Anchor {
                            x: (initial.x + dx / size.x).clamp(-10.0, 10.0),
                            y: (initial.y + dy / size.y).clamp(-10.0, 10.0),
                        };
                        self.scene.shapes[index] = *shape;
                        self.scene.set_anchor(index, anchor, self.time);
                    }
                    Gesture::Rotate {
                        id,
                        center,
                        inverse,
                        last,
                    } => {
                        let Some(index) = self.scene.shapes.iter().position(|shape| shape.id == id)
                        else {
                            self.gesture = None;
                            return;
                        };
                        let (x, y) = transform::point(inverse, point.x, point.y);
                        let angle = (y - center.y).atan2(x - center.x).to_degrees();
                        let delta = (angle - last + 540.0).rem_euclid(360.0) - 180.0;
                        let pose = self
                            .scene
                            .editable_pose(&self.scene.shapes[index], self.time);
                        let r = (pose.r + delta).clamp(-10000.0, 10000.0);
                        self.scene.set_property_pose(
                            index,
                            self.time,
                            Pose { r, ..pose },
                            Property::Rotation,
                        );
                        self.gesture = Some(Gesture::Rotate {
                            id,
                            center,
                            inverse,
                            last: angle,
                        });
                    }
                }
                self.edited();
            }
        }
        if response.drag_stopped() {
            self.gesture = None;
        }
        self.quick_bar(ui, area);
    }

    /// Electron Forma's timeline rows: one per layer (clips sharing a track side by side), each followed by its
    /// fade-out, drawn moves and spins. On a track of clips only the selected clip shows those.
    fn rows(&self) -> Vec<Row> {
        let mut groups: Vec<(String, Vec<Row>)> = Vec::new();
        for (index, shape) in self.scene.shapes.iter().enumerate() {
            let track = shape
                .extra
                .get("trackId")
                .and_then(|v| v.as_str())
                .filter(|t| !t.is_empty());
            let key = track.unwrap_or(&shape.id).to_owned();
            let group = match groups.iter().position(|(k, _)| *k == key) {
                Some(group) => group,
                None => {
                    let name = if track.is_some() {
                        shape
                            .name
                            .trim_end_matches(|c: char| c.is_ascii_digit())
                            .trim_end()
                            .to_owned()
                    } else {
                        shape.name.clone()
                    };
                    groups.push((
                        key,
                        vec![Row::Layer {
                            name,
                            clips: Vec::new(),
                        }],
                    ));
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
            if self.key_editor.expanded.contains(&shape.id) {
                rows.extend(
                    Lane::ALL
                        .into_iter()
                        .filter(|lane| {
                            !shape.is_camera()
                                || matches!(
                                    lane,
                                    Lane::Pose(Property::Position | Property::Rotation)
                                )
                        })
                        .chain(shape.spatial_lanes())
                        .map(|lane| Row::Property(index, lane)),
                );
                if let Some(vector) = &shape.vector {
                    rows.extend(vector.properties().into_iter().map(|p| Row::Property(index, Lane::Vector(p))));
                }
                if let Some(text) = &shape.text_layout {
                    rows.extend(text.style_lanes().map(|lane| Row::Property(index, lane)));
                    for animator in &text.animators {
                        rows.push(Row::TextAnimator(index, animator.id));
                        if self.key_editor.animator_expanded(&shape.id, animator.id) {
                            rows.extend(
                                text.animator_lanes(animator.id)
                                    .map(|lane| Row::Property(index, lane)),
                            );
                        }
                    }
                }
                if !shape.effects.is_empty() {
                    rows.push(Row::Effects(index));
                    if self.key_editor.effects_expanded(&shape.id) {
                        for effect in &shape.effects {
                            rows.push(Row::Effect(index, effect.id));
                            rows.extend(effect.kind.controls().iter().map(|control| {
                                Row::Property(index, Lane::Effect(effect.id, control.parameter))
                            }));
                        }
                    }
                }
                for (number, mask) in shape.masks.iter().enumerate() {
                    rows.push(Row::Mask(index, mask.id, number + 1));
                    if self.key_editor.mask_expanded(&shape.id, mask.id) {
                        rows.extend(
                            MaskProperty::ALL.map(|property| {
                                Row::Property(index, Lane::Mask(mask.id, property))
                            }),
                        );
                    }
                }
            }
            if shape.disappear.is_some() {
                rows.push(Row::Fade(index));
            }
            rows.extend((0..model::motions(shape).count()).map(|m| Row::Motion(index, m)));
            rows.extend((0..shape.rotation_clips.len()).map(|r| Row::Spin(index, r)));
        }
        groups.into_iter().flat_map(|(_, rows)| rows).collect()
    }

    fn workspace(&mut self, ui: &mut Ui, rect: Rect) {
        let rail = Rect::from_min_size(rect.min, vec2(88.0, rect.height()));
        let inspector_w = if rect.width() < 1150.0 { 250.0 } else { 310.0 };
        let right = Rect::from_min_max(pos2(rect.right() - inspector_w, rect.top()), rect.max);
        let drawer = Rect::from_min_size(
            pos2(rail.right() + 5.0, rect.top()),
            vec2(220.0, rect.height()),
        );
        let can_dock = right.left() - drawer.right() >= 530.0;
        if !self.drawer_decided {
            self.drawer_decided = true;
            self.drawer = can_dock.then_some(Drawer::Add);
        }
        let docked = can_dock && self.drawer.is_some();
        let left = if docked { drawer.right() } else { rail.right() };
        let center = Rect::from_min_max(
            pos2(left + 5.0, rect.top()),
            pos2(right.left() - 5.0, rect.bottom()),
        );
        let (canvas, timeline) = self.split_workspace(ui, center);

        for panel in [rail, right] {
            ui.painter().add(pane_frame(Mood::Ember).paint(panel));
        }
        ui.scope_builder(
            UiBuilder::new().id_salt("rail").max_rect(rail.shrink(8.0)),
            |ui| {
                crate::panel_widgets::scroll("tools", ui, |ui| self.rail(ui));
            },
        );
        ui.scope_builder(
            UiBuilder::new()
                .id_salt("inspector")
                .max_rect(right.shrink(12.0)),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(right));
                self.inspector(ui);
            },
        );
        ui.scope_builder(UiBuilder::new().id_salt("canvas").max_rect(canvas), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(canvas));
            self.canvas(ui)
        });
        let reveal_mask = self.mask_editor.reveal_lanes.take();
        if let Some((id, mask)) = &reveal_mask {
            self.key_editor.reveal_mask(id.clone(), *mask);
        }
        ui.scope_builder(
            UiBuilder::new().id_salt("timeline").max_rect(timeline),
            |ui| self.timeline(ui, reveal_mask.as_ref()),
        );

        if docked {
            ui.painter().add(pane_frame(Mood::Ember).paint(drawer));
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt("drawer")
                    .max_rect(drawer.shrink(10.0)),
                |ui| {
                    self.drawer(ui);
                },
            );
        } else if self.drawer.is_some() {
            // The floating drawer owns its hit region above the canvas and timeline.
            egui::Area::new(ui.scope_id().with("forma-drawer-float"))
                .order(egui::Order::Foreground)
                .fixed_pos(drawer.min)
                .constrain_to(rect)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_width(drawer.width() - 16.0);
                        ui.set_height((drawer.height() - 16.0).max(0.0));
                        self.drawer(ui);
                    });
                });
        }
    }

    fn split_workspace(&mut self, ui: &mut Ui, rect: Rect) -> (Rect, Rect) {
        const DIVIDER: f32 = 8.0;
        let usable = (rect.height() - DIVIDER).max(0.0);
        let max = (usable - 180.0).max(usable * 0.5);
        let min = 160.0_f32.min(max);
        // Electron budgets 26% of the viewport for tracks, plus the transport, ruler and footer.
        let default = (ui.ctx().content_rect().height() * 0.26).clamp(124.0, 300.0) + 100.0;
        let mut height = self.timeline_height.unwrap_or(default).clamp(min, max);
        let divider = |height| {
            Rect::from_min_size(
                pos2(rect.left(), rect.bottom() - height - DIVIDER),
                vec2(rect.width(), DIVIDER),
            )
        };
        let response = ui
            .interact(
                divider(height),
                ui.scope_id().with("timeline-divider"),
                Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeVertical)
            .on_hover_text(
                "Drag to resize the timeline. Double-click to reset. Use Up or Down when focused.",
            );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Splitter, true, "Timeline height")
        });
        if response.drag_started() {
            self.timeline_at_drag_start = Some(height);
        }
        if let (Some(start), Some(delta)) =
            (self.timeline_at_drag_start, response.total_drag_delta())
        {
            height = (start - delta.y).clamp(min, max);
            self.timeline_height = Some(height);
        }
        let mut remember = response.drag_stopped() && self.timeline_at_drag_start.take().is_some();
        if response.has_focus() {
            let step = ui.input(|input| {
                f32::from(input.key_pressed(egui::Key::ArrowUp))
                    - f32::from(input.key_pressed(egui::Key::ArrowDown))
            });
            if step != 0.0 {
                height = (height + step * 24.0).clamp(min, max);
                self.timeline_height = Some(height);
                remember = true;
            }
        }
        if response.double_clicked() {
            self.timeline_height = None;
            height = default.clamp(min, max);
            remember = true;
        }
        if remember
            && let Err(error) = storage::save_timeline_height(&self.place, self.timeline_height)
        {
            self.notice = Some(format!("Forma couldn't remember timeline height: {error}"));
        }
        let divider = divider(height);
        let ink = if response.hovered() || response.dragged() || response.has_focus() {
            color::SUN
        } else {
            color::HAIRLINE_STRONG
        };
        ui.painter()
            .hline(divider.x_range(), divider.center().y, Stroke::new(1.0, ink));
        ui.painter().rect_filled(
            Rect::from_center_size(divider.center(), vec2(40.0, 3.0)),
            2.0,
            ink,
        );
        (
            Rect::from_min_max(rect.min, pos2(rect.right(), divider.top())),
            Rect::from_min_max(pos2(rect.left(), divider.bottom()), rect.max),
        )
    }

    /// The timeline toolbar stays visible while the tracks scroll.
    fn transport(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if icon_button(ui, Icon::SkipBack, "Back to start (Home)", false).clicked() {
                self.playing = false;
                self.preview_end = None;
                self.time = 0.0;
            }
            let play = if self.playing { Icon::Pause } else { Icon::Play };
            let label = if self.playing { "Pause (Space)" } else { "Play (Space)" };
            if icon_button(ui, play, label, self.playing).clicked() {
                if !self.playing && self.time >= self.scene.duration {
                    self.time = 0.0;
                }
                self.playing = !self.playing;
                self.preview_end = None;
                self.last_tick = Instant::now();
            }
            if pill(ui, "Loop", None, self.loop_playback)
                .on_hover_text("Loop playback")
                .clicked()
            {
                self.loop_playback = !self.loop_playback;
            }
            ui.label(
                egui::RichText::new(format!("{:.2} / {:.2} s", self.time, self.scene.duration))
                    .font(font::ui(13.0))
                    .color(color::INK_SOFT),
            );
            let quality_label = if self.playing_at_half { "Half (auto)" } else { self.quality.label() };
            egui::ComboBox::from_id_salt("forma-preview-quality")
                .selected_text(quality_label)
                .show_ui(ui, |ui| {
                    for quality in crate::preview_cache::Quality::ALL {
                        if ui.selectable_value(&mut self.quality, quality, quality.label()).clicked() {
                            self.playing_at_half = false;
                        }
                    }
                }).response.on_hover_text("Preview quality. Full switches to Half during playback if rendering exceeds 33 ms.");
            if ui.available_width() > 370.0 {
                let used = 100 * self.frame_cache.bytes() / crate::preview_cache::FRAME_BYTES;
                ui.label(egui::RichText::new(format!("{:.0} ms · RAM {used}%", self.frame_time_ms))
                    .font(font::ui(11.0)).color(color::INK_MUTED))
                    .on_hover_text("Last frame render time and playback cache fill");
            }
            self.key_editor.toolbar(ui, &mut self.scene, self.selected.as_deref());
            if pill(ui, "Onion skin", None, self.onion_skin)
                .on_hover_text("While editing a clip, faintly show the clip before it on the same track")
                .clicked()
            {
                self.onion_skin = !self.onion_skin;
            }
        });
    }

    fn timeline(&mut self, ui: &mut Ui, reveal_mask: Option<&(String, u64)>) {
        let full = ui.max_rect();
        ui.painter().add(pane_frame(Mood::Ember).paint(full));
        ui.set_clip_rect(ui.clip_rect().intersect(full));
        let bar = ui
            .scope_builder(
                UiBuilder::new()
                    .id_salt("transport")
                    .max_rect(full.shrink(8.0)),
                |ui| self.transport(ui),
            )
            .response
            .rect;
        let ruler_top = (bar.bottom() + 4.0).min(full.bottom());
        let rect = Rect::from_min_max(pos2(full.left(), ruler_top), full.max);
        self.key_editor.begin_frame(&self.scene);
        let reveal_animator = self.key_editor.animator_to_reveal.take();
        let reveal_effects = self.key_editor.effects_to_reveal.take();
        let plot = Rect::from_min_max(
            pos2(rect.left() + LABEL_WIDTH, rect.top()),
            pos2(
                (rect.right() - TIME_MARGIN).max(rect.left() + LABEL_WIDTH + 1.0),
                rect.bottom(),
            ),
        );
        let duration = self.scene.duration;
        let view = self.key_editor.time_view(ui, plot, duration);
        let x = |t: f64| view.x(plot, t).clamp(plot.left(), plot.right());
        let width = plot.width() * (duration / view.span) as f32;
        let step = [
            0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0,
            1800.0, 3600.0,
        ]
        .into_iter()
        .find(|s| plot.width() as f64 * s / view.span >= 60.0)
        .unwrap_or(3600.0);
        let first = (view.start / step).ceil() as usize;
        let last = ((view.start + view.span) / step).floor() as usize;
        for n in first..=last {
            let tick = n as f64 * step;
            let pos = x(tick);
            ui.painter().line_segment(
                [pos2(pos, rect.top() + 6.0), pos2(pos, rect.bottom())],
                Stroke::new(0.5, color::HAIRLINE),
            );
            let label = if tick >= 60.0 {
                format!("{}:{:02}", tick as u64 / 60, tick as u64 % 60)
            } else {
                format!("{:.2}s", model::hundredths(tick)).replace(".00s", "s")
            };
            let galley = ui
                .painter()
                .layout_no_wrap(label, font::ui(10.0), color::INK_MUTED);
            if pos + 3.0 + galley.size().x <= plot.right() {
                ui.painter()
                    .galley(pos2(pos + 3.0, rect.top() + 4.0), galley, color::INK_MUTED);
            }
        }
        let ruler = Rect::from_min_max(plot.min, pos2(plot.right(), rect.top() + 25.0));
        let response = ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
        if let (true, Some(p)) = (
            response.clicked() || response.dragged(),
            response.interact_pointer_pos(),
        ) {
            self.time = view.time(plot, p.x).clamp(0.0, duration);
            self.playing = false;
        }
        let rows = self.rows();
        let area = Rect::from_min_max(pos2(rect.left(), rect.top() + 27.0), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            crate::panel_widgets::scroll("forma-timeline", ui, |ui| {
                // Each row advances the cursor once. Its controls use absolute positions.
                for row in &rows {
                    if let Row::Property(index, lane) = *row {
                        let id = self.scene.shapes[index].id.clone();
                        let height = self.key_editor.row_height(&id, lane);
                        let (mut line, _) = ui.allocate_exact_size(
                            vec2(ui.available_width(), height),
                            Sense::hover(),
                        );
                        line.max.x = rect.right();
                        if ui.is_rect_visible(line)
                            && self.key_editor.row(
                                ui,
                                &mut self.scene,
                                index,
                                lane,
                                line,
                                view,
                                &mut self.time,
                                &mut self.playing,
                            )
                        {
                            self.selected = Some(id.clone());
                            if let Lane::Mask(mask, _) = lane {
                                self.mask_editor.select(&id, mask);
                            }
                            self.multi_selected = vec![id];
                            self.tab = Tab::Animate;
                        }
                    } else {
                        let (line, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
                        if let (Row::Mask(index, mask, _), Some((id, reveal))) = (row, reveal_mask)
                            && mask == reveal
                            && self.scene.shapes[*index].id == *id
                        {
                            ui.scroll_to_rect(line, Some(egui::Align::TOP));
                        }
                        if let (Row::Effects(index), Some(id)) = (row, reveal_effects.as_ref())
                            && self
                                .scene
                                .shapes
                                .get(*index)
                                .is_some_and(|shape| &shape.id == id)
                        {
                            ui.scroll_to_rect(line, Some(egui::Align::TOP));
                        }
                        if let (Row::TextAnimator(index, animator), Some((id, reveal))) =
                            (row, reveal_animator.as_ref())
                            && animator == reveal
                            && self.scene.shapes[*index].id == *id
                        {
                            ui.scroll_to_rect(line, Some(egui::Align::TOP));
                        }
                        if ui.is_rect_visible(line) {
                            self.timeline_row(ui, row, line, &x, width, view);
                        }
                    }
                }
            });
        });
        self.key_editor
            .finish_frame(ui, &mut self.scene, area, self.time, self.lock_ratio);
        if let Some((id, action)) = self.pending_clip.take() {
            if self.scene.shapes.iter().any(|s| s.id == id) {
                // Right-clicking one of several selected layers acts on all of them, as Delete or Ctrl+C do.
                if self.multi_selected.contains(&id) {
                    self.selected = Some(id);
                } else {
                    self.select_layer(Some(id), false);
                }
                self.clip_action(action);
            }
        }
        if view.contains(self.time) {
            let play_x = x(self.time);
            ui.painter().line_segment(
                [pos2(play_x, rect.top()), pos2(play_x, rect.bottom())],
                Stroke::new(2.0, color::SUN),
            );
        }
        if ui.input(|i| !i.pointer.any_down()) && self.timeline_drag.take().is_some() {
            // A clip that moved away from the playhead brings the playhead along, as in Electron.
            if let Some(shape) = self
                .selected
                .as_deref()
                .and_then(|id| self.scene.shapes.iter().find(|s| s.id == id))
            {
                if !shape.visible(self.time, self.scene.duration) {
                    self.time = shape.clip_start();
                }
            }
        }
    }

    fn timeline_row(
        &mut self,
        ui: &mut Ui,
        row: &Row,
        line: Rect,
        x: &dyn Fn(f64) -> f32,
        width: f32,
        view: TimeView,
    ) {
        let bar = |from: f64, to: f64| {
            Rect::from_min_max(
                pos2(x(from), line.top() + 3.0),
                pos2(x(to).max(x(from) + 3.0), line.bottom() - 3.0),
            )
        };
        let label = |painter: &egui::Painter, text: &str, swatch: Option<Color32>, sub: bool| {
            let mut at = line.left() + if sub { 18.0 } else { 38.0 };
            if let Some(fill) = swatch {
                painter.rect_filled(
                    Rect::from_center_size(pos2(at + 5.0, line.center().y), vec2(10.0, 10.0)),
                    2.0,
                    fill,
                );
                at += 15.0;
            }
            let ink = if sub { color::INK_MUTED } else { color::INK };
            let mut job =
                egui::text::LayoutJob::simple_singleline(text.to_owned(), font::ui(11.0), ink);
            job.wrap =
                egui::text::TextWrapping::truncate_at_width(line.left() + LABEL_WIDTH - 10.0 - at);
            let galley = painter.layout_job(job);
            painter.galley(
                pos2(at, line.center().y - galley.size().y * 0.5),
                galley,
                ink,
            );
        };
        ui.painter().rect_filled(
            Rect::from_min_max(
                pos2(x(0.0), line.top() + 3.0),
                pos2(x(self.scene.duration), line.bottom() - 3.0),
            ),
            4.0,
            color::VEIL,
        );
        match row {
            Row::Layer { name, clips } => {
                let Some(&first) = clips.first() else {
                    return;
                };
                let pick = *clips
                    .iter()
                    .find(|&&i| self.selected.as_deref() == Some(&self.scene.shapes[i].id))
                    .or_else(|| {
                        clips.iter().find(|&&i| {
                            self.scene.shapes[i].visible(self.time, self.scene.duration)
                        })
                    })
                    .unwrap_or(&first);
                let id = self.scene.shapes[pick].id.clone();
                let open = self.key_editor.expanded.contains(&id);
                let disclosure = Rect::from_min_size(line.min + vec2(3.0, 0.0), Vec2::splat(32.0));
                if ui
                    .place(
                        disclosure,
                        egui::Button::selectable(open, disclosure_symbol(open)),
                    )
                    .on_hover_text(if open {
                        "Hide property lanes"
                    } else {
                        "Show property lanes"
                    })
                    .clicked()
                {
                    self.key_editor.toggle_layer(&id);
                    self.select_clip(pick);
                }
                let label_rect = Rect::from_min_max(
                    line.min + vec2(36.0, 0.0),
                    pos2(line.left() + LABEL_WIDTH - 8.0, line.bottom()),
                );
                label(
                    &ui.painter().with_clip_rect(label_rect),
                    name,
                    Some(hex(&self.scene.shapes[first].color, 255)),
                    false,
                );
                let head = ui
                    .interact(
                        label_rect,
                        ui.scope_id().with(("track", &id)),
                        Sense::click(),
                    )
                    .on_hover_text(name);
                if head.double_clicked() {
                    self.queue_composition(&id);
                }
                if head.clicked() {
                    self.select_clip(pick);
                }
                for &index in clips {
                    self.clip_bar(ui, index, line, x, width, view);
                }
            }
            Row::Property(..) => {}
            Row::TextAnimator(index, animator) => {
                let shape = &self.scene.shapes[*index];
                let Some(animation) = shape
                    .text_layout
                    .as_ref()
                    .and_then(|text| text.animator(*animator))
                else {
                    return;
                };
                let id = shape.id.clone();
                let open = self.key_editor.animator_expanded(&id, *animator);
                let label = Rect::from_min_size(line.min + vec2(14.0, 0.0), vec2(140.0, 32.0));
                if ui
                    .place(
                        label,
                        egui::Button::selectable(
                            open,
                            (disclosure_symbol(open), animation.name.as_str()),
                        )
                        .truncate(),
                    )
                    .on_hover_text(format!("Show or hide lanes for {}", animation.name))
                    .clicked()
                {
                    self.key_editor.toggle_animator(id.clone(), *animator);
                    self.select_layer(Some(id), false);
                    self.tab = Tab::Animate;
                }
            }
            Row::Effects(index) => {
                let Some(shape) = self.scene.shapes.get(*index) else {
                    return;
                };
                let id = shape.id.clone();
                let open = self.key_editor.effects_expanded(&id);
                let label = Rect::from_min_size(line.min + vec2(14.0, 0.0), vec2(140.0, 32.0));
                if ui
                    .place(
                        label,
                        egui::Button::selectable(open, (disclosure_symbol(open), "Effects")),
                    )
                    .on_hover_text("Show or hide effect parameter lanes")
                    .clicked()
                {
                    self.key_editor.toggle_effects(id.clone());
                    self.select_layer(Some(id), false);
                    self.tab = Tab::Animate;
                }
            }
            Row::Effect(index, effect) => {
                if let Some(effect) = self.scene.shapes[*index].effect(*effect) {
                    label(ui.painter(), effect.kind.label(), None, true);
                }
            }
            Row::Mask(index, mask, number) => {
                let id = self.scene.shapes[*index].id.clone();
                let open = self.key_editor.mask_expanded(&id, *mask);
                let label = Rect::from_min_size(line.min + vec2(14.0, 0.0), vec2(140.0, 32.0));
                if ui
                    .place(
                        label,
                        egui::Button::selectable(
                            open,
                            (disclosure_symbol(open), format!("Mask {number}")),
                        ),
                    )
                    .on_hover_text("Show or hide this mask's animation lanes")
                    .clicked()
                {
                    self.key_editor.toggle_mask(&id, *mask);
                    self.select_layer(Some(id.clone()), false);
                    self.mask_editor.select(&id, *mask);
                }
            }
            Row::Fade(index) => {
                let shape = &self.scene.shapes[*index];
                let Some(d) = &shape.disappear else {
                    return;
                };
                label(ui.painter(), "Fade out", None, true);
                let end = self.scene.reappear_at(shape).min(self.scene.duration);
                let bar_end = end.max(d.start_time + self.scene.duration * 0.015);
                if !view.includes_span(d.start_time, bar_end) {
                    return;
                }
                let span = bar(d.start_time, bar_end);
                ui.painter().rect_filled(span, 4.0, Accent::Berry.color());
                paint_clip_label(ui, span, "Hidden".into(), color::INK);
                let timed = d.reappear_mode == "time";
                let tip = format!(
                    "Fade out {:.2}–{:.2} s{}. Drag to move it{}.",
                    d.start_time,
                    d.start_time + d.duration,
                    if end.is_finite() && end < self.scene.duration {
                        format!("; reappear {end:.2} s")
                    } else {
                        "; stay hidden".into()
                    },
                    if timed {
                        ", drag its end to change when it comes back"
                    } else {
                        ""
                    }
                );
                let (id, start) = (shape.id.clone(), d.start_time);
                let response = ui
                    .interact(
                        span,
                        ui.scope_id().with(("fade", &id)),
                        Sense::click_and_drag(),
                    )
                    .on_hover_text(tip);
                edge_cursor(ui, &response, span, false, timed);
                self.timeline_gesture(
                    ui,
                    &response,
                    *index,
                    span,
                    Part::Fade,
                    width,
                    (false, view.contains(bar_end)),
                );
                if response.clicked() {
                    (self.selected, self.time, self.playing) = (Some(id), start, false);
                }
            }
            Row::Motion(index, m) => {
                let shape = &self.scene.shapes[*index];
                let Some((motion, start, length)) = model::motions(shape).nth(*m) else {
                    return;
                };
                let path = motion.get("type").and_then(|t| t.as_str()) == Some("path");
                label(
                    ui.painter(),
                    &format!("{} {}", if path { "Path" } else { "Move" }, m + 1),
                    None,
                    true,
                );
                if !view.includes_span(start, start + length) {
                    return;
                }
                let span = bar(start, start + length);
                ui.painter().rect_filled(span, 4.0, Accent::Sky.color());
                paint_clip_label(
                    ui,
                    span,
                    if path { "Mouse path" } else { "Arrow move" }.into(),
                    color::INK,
                );
                let id = shape.id.clone();
                let motion_id = motion.get("id").and_then(|v| v.as_str()).map(str::to_owned);
                let dragging = self
                    .timeline_drag
                    .as_ref()
                    .is_some_and(|d| d.shape.id == id && matches!(d.part, Part::Motion(i) | Part::MotionStart(i) | Part::MotionEnd(i) if i == *m));
                if dragging {
                    let after = Rect::from_min_max(
                        span.right_top(),
                        pos2(line.right() - TIME_MARGIN, span.bottom()),
                    );
                    paint_clip_label(
                        ui,
                        after,
                        format!("{start:.2}\u{2013}{:.2} s", start + length),
                        color::INK,
                    );
                }
                let response = ui
                    .interact(
                        span,
                        ui.scope_id().with(("motion", &id, *m)),
                        Sense::click_and_drag(),
                    )
                    .on_hover_text(format!(
                        "{}: {start:.2}–{:.2} s. Drag to move it, drag an end to make it longer or shorter.",
                        shape.name,
                        start + length
                    ));
                edge_cursor(ui, &response, span, true, true);
                self.timeline_gesture(
                    ui,
                    &response,
                    *index,
                    span,
                    Part::Motion(*m),
                    width,
                    (view.contains(start), view.contains(start + length)),
                );
                if response.clicked() || response.drag_started() {
                    self.selected_motion = motion_id;
                }
                if response.clicked() {
                    (self.selected, self.time, self.playing) = (Some(id), start, false);
                }
            }
            Row::Spin(index, r) => {
                let clip = &self.scene.shapes[*index].rotation_clips[*r];
                let id = self.scene.shapes[*index].id.clone();
                label(ui.painter(), &format!("Spin {}", r + 1), None, true);
                if !view.includes_span(clip.start_time, clip.start_time + clip.duration) {
                    return;
                }
                let span = bar(clip.start_time, clip.start_time + clip.duration);
                ui.painter().rect_filled(span, 4.0, Accent::Clay.color());
                paint_clip_label(
                    ui,
                    span,
                    format!("Spin {}\u{00b0}", clip.degrees),
                    color::INK,
                );
                let tip = format!(
                    "Rotation {}° · {:.2}–{:.2} s. Drag to move, drag an end to change its length.",
                    clip.degrees,
                    clip.start_time,
                    clip.start_time + clip.duration
                );
                let start = clip.start_time;
                let response = ui
                    .interact(
                        span,
                        ui.scope_id().with(("spin", &id, *r)),
                        Sense::click_and_drag(),
                    )
                    .on_hover_text(tip);
                edge_cursor(ui, &response, span, true, true);
                self.timeline_gesture(
                    ui,
                    &response,
                    *index,
                    span,
                    Part::Spin(*r),
                    width,
                    (view.contains(start), view.contains(start + clip.duration)),
                );
                if response.clicked() {
                    (self.selected, self.time, self.playing) = (Some(id), start, false);
                }
            }
        }
    }

    /// A layer's clip bar, its keyframes and its color keyframes.
    fn clip_bar(
        &mut self,
        ui: &mut Ui,
        index: usize,
        line: Rect,
        x: &dyn Fn(f64) -> f32,
        width: f32,
        view: TimeView,
    ) {
        let shape = &self.scene.shapes[index];
        let (start, end) = (
            shape.clip_start(),
            shape
                .out_time
                .unwrap_or(self.scene.duration)
                .min(self.scene.duration),
        );
        if !view.includes_span(start, end) {
            return;
        }
        let span = Rect::from_min_max(
            pos2(x(start), line.top() + 3.0),
            pos2(x(end).max(x(start) + 3.0), line.bottom() - 3.0),
        );
        let selected = self.selected.as_deref() == Some(&shape.id);
        // Canopy's colors: the selected clip in sun, the rest in moss.
        ui.painter().rect_filled(
            span,
            4.0,
            if selected {
                color::SUN
            } else {
                Accent::Moss.color()
            },
        );
        let dragging = self.timeline_drag.as_ref().is_some_and(|d| {
            d.shape.id == shape.id && matches!(d.part, Part::Clip | Part::Start | Part::End)
        });
        let track = shape
            .extra
            .get("trackId")
            .and_then(|v| v.as_str())
            .is_some_and(|t| !t.is_empty());
        if dragging || track {
            let text = if dragging {
                format!("{start:.2}–{end:.2} s")
            } else {
                shape.name.clone()
            };
            let ink = if selected {
                Mood::Ember.ground()
            } else {
                color::INK
            };
            paint_clip_label(ui, span, text, ink);
        }
        let tip = format!(
            "{}: visible {start:.2}–{end:.2} s. Drag to move, drag an edge to trim.",
            shape.name
        );
        let id = shape.id.clone();
        let opens_composition = shape.composition.is_some();
        let response = ui
            .interact(
                span,
                ui.scope_id().with(("clip", &id)),
                Sense::click_and_drag(),
            )
            .on_hover_text(tip);
        if opens_composition && response.double_clicked() {
            self.queue_composition(&id);
        }
        if let Some(p) = response.hover_pos() {
            let edge = (p.x - span.left()).abs() <= 6.0 || (p.x - span.right()).abs() <= 6.0;
            ui.ctx().set_cursor_icon(if edge {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::Grab
            });
        }
        self.timeline_gesture(
            ui,
            &response,
            index,
            span,
            Part::Clip,
            width,
            (view.contains(start), view.contains(end)),
        );
        if response.clicked() || response.secondary_clicked() {
            self.select_clip(index);
        }
        response.context_menu(|ui| {
            self.follows_picker(ui, index, true);
            ui.separator();
            for (label, action, keys) in [
                ("Next pose after this", ClipAction::Next, ""),
                (
                    "Paste copied layer after",
                    ClipAction::PasteAfter,
                    "Ctrl+Shift+V",
                ),
                ("Split at playhead", ClipAction::Split, "S"),
                ("Duplicate", ClipAction::Duplicate, "Ctrl+D"),
                ("Copy", ClipAction::Copy, "Ctrl+C"),
                ("Delete clip", ClipAction::Delete, "Delete"),
            ] {
                let enabled = action != ClipAction::PasteAfter || !self.clipboard.is_empty();
                if ui
                    .add_enabled(enabled, egui::Button::new(label).shortcut_text(keys))
                    .clicked()
                {
                    self.select_clip(index);
                    self.pending_clip = Some((id.clone(), action));
                    ui.close();
                }
            }
        });
        let mut times: Vec<_> = self.scene.shapes[index]
            .keys
            .iter()
            .map(|k| k.t)
            .chain(self.scene.shapes[index].color_keys.iter().map(|k| k.t))
            .chain(self.scene.shapes[index].mask_times())
            .collect();
        times.sort_by(f64::total_cmp);
        times.dedup_by(|a, b| crate::keyframes::same_time(*a, *b));
        for t in times.into_iter().filter(|t| (start..=end).contains(t)) {
            let at = pos2(x(t), span.center().y);
            if at.x <= x(0.0) || at.x >= x(self.scene.duration) {
                continue;
            }
            ui.painter().line_segment(
                [at - vec2(0.0, 3.0), at + vec2(0.0, 3.0)],
                Stroke::new(1.0, color::INK_SOFT),
            );
        }
    }

    /// The layers an action works on: every selected layer when several are (Shift-click, Ctrl+A), else the one.
    /// In the scene's order.
    fn targets(&self) -> Vec<String> {
        let Some(selected) = self.selected.as_ref() else {
            return Vec::new();
        };
        let several = self.multi_selected.len() > 1 && self.multi_selected.contains(selected);
        self.scene
            .shapes
            .iter()
            .filter(|s| if several { self.multi_selected.contains(&s.id) } else { &s.id == selected })
            .map(|s| s.id.clone())
            .collect()
    }

    /// Selects these layers together, the first one in the inspector.
    fn select_all_of(&mut self, ids: Vec<String>) {
        self.selected = ids.first().cloned();
        self.multi_selected = ids;
    }

    /// Electron's clip actions, on the selected layer (Copy, Duplicate and Delete on every selected layer): they
    /// report problems as a toast.
    fn clip_action(&mut self, action: ClipAction) {
        let plural = |n: usize, one: &str| if n == 1 { one.to_owned() } else { format!("{n} layers") };
        match action {
            ClipAction::Paste => {
                if self.clipboard.is_empty() {
                    return;
                }
                if self.clipboard.iter().any(|layer| !self.can_paste_composition(layer)) {
                    self.notice = Some("Can't paste a composition whose source is missing here or would form a loop.".into());
                    return;
                }
                let ids: Vec<_> = self
                    .clipboard
                    .iter()
                    .map(|layer| (layer.id.clone(), new_id()))
                    .collect();
                let ids = self.scene.paste_layers(&self.clipboard, &ids);
                self.select_all_of(ids);
                self.edited();
                return;
            }
            ClipAction::Copy | ClipAction::Duplicate | ClipAction::Delete => {
                let ids = self.targets();
                if ids.is_empty() {
                    self.notice = Some("Select a layer first".into());
                    return;
                }
                let name = |me: &Self, id: &String| {
                    me.scene.shapes.iter().find(|s| &s.id == id).map(|s| s.name.clone()).unwrap_or_default()
                };
                let first = name(self, &ids[0]);
                self.playing = false;
                match action {
                    ClipAction::Copy => {
                        self.clipboard = self
                            .scene
                            .shapes
                            .iter()
                            .filter(|s| ids.contains(&s.id))
                            .cloned()
                            .collect();
                        self.notice = Some(format!("{} copied", plural(ids.len(), &first)));
                    }
                    ClipAction::Duplicate => {
                        let ids: Vec<_> = ids.into_iter().map(|id| (id, new_id())).collect();
                        let copies = self.scene.duplicate_layers(&ids);
                        if let Some(shape) = self
                            .scene
                            .shapes
                            .iter()
                            .find(|s| Some(&s.id) == copies.first())
                        {
                            if !shape.visible(self.time, self.scene.duration) {
                                self.time = shape.clip_start();
                            }
                        }
                        self.notice = Some(if copies.len() == 1 {
                            format!("{} added", name(self, &copies[0]))
                        } else {
                            format!("{} layers duplicated", copies.len())
                        });
                        self.select_all_of(copies);
                        self.edited();
                    }
                    _ => {
                        self.scene.delete_layers(&ids, self.time);
                        self.select_all_of(Vec::new());
                        (self.selected_motion, self.selected_spin, self.motion_shown) = (None, None, None);
                        self.notice = Some(format!("{} deleted", plural(ids.len(), &first)));
                        self.edited();
                    }
                }
                return;
            }
            _ => {}
        }
        let Some(index) = self
            .selected
            .as_deref()
            .and_then(|id| self.scene.shapes.iter().position(|s| s.id == id))
        else {
            self.notice = Some("Select a clip first".into());
            return;
        };
        self.playing = false;
        let result = match action {
            ClipAction::Next => self
                .scene
                .continue_after(index, None, self.time, new_id(), new_id())
                .map(|i| (i, "added — drag it to its next position")),
            ClipAction::PasteAfter => match self.clipboard.first().cloned() {
                Some(template) if self.can_paste_composition(&template) => self
                    .scene
                    .continue_after(index, Some(&template), self.time, new_id(), new_id())
                    .map(|i| (i, "added — drag it to its next position")),
                Some(_) => Err("Can't paste a composition whose source is missing here or would form a loop.".into()),
                None => Err("Copy a layer first (Ctrl+C)".into()),
            },
            ClipAction::Split => self
                .scene
                .split_at(index, self.time, new_id(), new_id())
                .map(|i| (i, "split off")),
            ClipAction::Paste | ClipAction::Copy | ClipAction::Duplicate | ClipAction::Delete => {
                unreachable!("handled above")
            }
        };
        match result {
            Ok((index, done)) => {
                let shape = &self.scene.shapes[index];
                self.notice = Some(format!(
                    "{} {done} at {:.2} s",
                    shape.name,
                    shape.clip_start()
                ));
                (self.selected, self.time) = (Some(shape.id.clone()), shape.clip_start());
                self.edited();
            }
            Err(error) => self.notice = Some(error),
        }
    }

    /// Selects a clip and brings the playhead into it (Electron's `selectClip`).
    fn select_clip(&mut self, index: usize) {
        self.key_editor.clear_selection();
        let shape = &self.scene.shapes[index];
        self.selected = Some(shape.id.clone());
        self.playing = false;
        if !shape.visible(self.time, self.scene.duration) {
            self.time = shape.clip_start();
        }
    }

    /// Drags on the timeline, applied to the layer as it was when the drag began, so every frame starts afresh.
    fn timeline_gesture(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        index: usize,
        span: Rect,
        part: Part,
        width: f32,
        trim: (bool, bool),
    ) {
        if response.drag_started() {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(span.center());
            let (at_start, at_end) = (
                trim.0 && (origin.x - span.left()).abs() <= EDGE,
                trim.1 && (origin.x - span.right()).abs() <= EDGE,
            );
            let part = match part {
                Part::Clip if at_start => Part::Start,
                Part::Clip if at_end => Part::End,
                Part::Spin(r) if at_end => Part::SpinLength(r),
                Part::Spin(r) if at_start => Part::SpinStart(r),
                Part::Motion(m) if at_end => Part::MotionEnd(m),
                Part::Motion(m) if at_start => Part::MotionStart(m),
                Part::Fade if at_end && span.width() > 2.0 * EDGE => Part::FadeEnd,
                part => part,
            };
            self.timeline_drag = Some(TimelineDrag {
                part,
                origin: origin.x,
                seconds_per_pixel: self.scene.duration / f64::from(width),
                shape: self.scene.shapes[index].clone(),
                duration: self.scene.duration,
            });
            self.selected = Some(self.scene.shapes[index].id.clone());
            self.playing = false;
        }
        let Some(drag) = self
            .timeline_drag
            .clone()
            .filter(|d| d.shape.id == self.scene.shapes[index].id && response.dragged())
        else {
            return;
        };
        let Some(pointer) = response.interact_pointer_pos() else {
            return;
        };
        let duration = drag.duration;
        let delta = f64::from(pointer.x - drag.origin) * drag.seconds_per_pixel;
        // Snap to the playhead, the scene's ends and other clips' edges within 8 px, else to tenths; Alt is free.
        let mut targets = vec![0.0, self.time, duration];
        for (_, other) in self
            .scene
            .shapes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
        {
            targets.push(other.clip_start());
            targets.extend(other.out_time);
        }
        let tolerance = 8.0 * drag.seconds_per_pixel;
        let free = ui.input(|i| i.modifiers.alt);
        let snap = |t: f64| -> (f64, bool) {
            if free {
                return (model::hundredths(t), false);
            }
            let nearest = targets.iter().copied().fold(f64::INFINITY, |best, x| {
                if (x - t).abs() < (best - t).abs() {
                    x
                } else {
                    best
                }
            });
            if (nearest - t).abs() <= tolerance {
                (nearest, true)
            } else {
                ((t * 10.0).round() / 10.0, false)
            }
        };
        self.scene.shapes[index] = drag.shape.clone();
        self.scene.duration = duration;
        let (start0, end0) = (
            drag.shape.clip_start(),
            drag.shape.out_time.unwrap_or(duration),
        );
        match drag.part {
            Part::Clip => {
                let (a, b) = (snap(start0 + delta), snap(end0 + delta));
                let use_end =
                    b.1 && (!a.1 || (b.0 - end0 - delta).abs() < (a.0 - start0 - delta).abs());
                self.scene
                    .shift_layer(index, if use_end { b.0 - end0 } else { a.0 - start0 });
            }
            Part::Start => {
                let start = snap(start0 + delta).0.min(end0 - model::MIN_CLIP).max(0.0);
                let _ = self
                    .scene
                    .trim_clip(index, start, drag.shape.out_time.map(|_| end0));
            }
            Part::End => {
                let end = snap(end0 + delta).0.max(start0 + model::MIN_CLIP);
                let _ = self.scene.trim_clip(index, start0, Some(end));
            }
            Part::Spin(r) | Part::SpinStart(r) | Part::SpinLength(r) => {
                let clip = &drag.shape.rotation_clips[r];
                let target = &mut self.scene.shapes[index].rotation_clips[r];
                let end = clip.start_time + clip.duration;
                match drag.part {
                    // A spin can run past the scene's end: the scene grows to fit, as it does for clips.
                    Part::SpinLength(_) => {
                        target.duration = (snap(end + delta).0 - clip.start_time).max(0.1);
                        self.scene.duration =
                            self.scene.duration.max(clip.start_time + target.duration);
                    }
                    Part::SpinStart(_) => {
                        let start = snap(clip.start_time + delta).0.clamp(0.0, end - 0.1);
                        (target.start_time, target.duration) =
                            (start, model::hundredths(end - start));
                    }
                    _ => {
                        let (a, b) = (snap(clip.start_time + delta), snap(end + delta));
                        let start = if b.1 && !a.1 {
                            b.0 - clip.duration
                        } else {
                            a.0
                        };
                        target.start_time = model::hundredths(start.max(0.0));
                        self.scene.duration =
                            self.scene.duration.max(target.start_time + clip.duration);
                    }
                }
            }
            Part::Motion(m) | Part::MotionStart(m) | Part::MotionEnd(m) => {
                let list = motion::motions(&drag.shape);
                let Some(mo) = list.get(m) else { return };
                let (s0, l0) = (mo.start_time, mo.duration);
                // A move can't overlap another of the layer's moves: it stops at its neighbours.
                let before = list
                    .iter()
                    .filter(|o| o.id != mo.id && o.start_time + o.duration <= s0 + 1e-3)
                    .map(|o| o.start_time + o.duration)
                    .fold(0.0, f64::max);
                let after = list
                    .iter()
                    .filter(|o| o.id != mo.id && o.start_time >= s0 + l0 - 1e-3)
                    .map(|o| o.start_time)
                    .fold(f64::INFINITY, f64::min);
                let (start, length) = match drag.part {
                    Part::MotionStart(_) => {
                        let start = snap(s0 + delta).0.clamp(before, s0 + l0 - 0.1);
                        (start, s0 + l0 - start)
                    }
                    Part::MotionEnd(_) => (s0, (snap(s0 + l0 + delta).0.min(after) - s0).max(0.1)),
                    _ => {
                        let (a, b) = (snap(s0 + delta), snap(s0 + l0 + delta));
                        let start = if b.1 && !a.1 { b.0 - l0 } else { a.0 };
                        (start.max(before).min(after - l0).max(0.0), l0)
                    }
                };
                let options = motion::Options {
                    seconds: length,
                    distance: mo.distance,
                    spin: mo.spin,
                    face: mo.face,
                };
                let id = mo.id.clone();
                let _ = motion::edit(&mut self.scene, index, &id, start, length, options);
            }
            Part::Fade | Part::FadeEnd => {
                let Some(d) = drag.shape.disappear.clone() else {
                    return;
                };
                let target = self.scene.shapes[index].disappear.as_mut().unwrap();
                if drag.part == Part::FadeEnd {
                    if let Some(back) = d.reappear_time {
                        target.reappear_time = Some(model::hundredths(
                            snap(back + delta).0.max(d.start_time + d.duration),
                        ));
                    }
                } else {
                    let start = model::hundredths(snap(d.start_time + delta).0.max(0.0));
                    target.start_time = start;
                    if d.reappear_mode == "time" {
                        target.reappear_time = d
                            .reappear_time
                            .map(|t| model::hundredths(t + start - d.start_time));
                    }
                    self.scene.duration = self.scene.duration.max(start + d.duration);
                }
            }
        }
        self.edited();
    }

    /// Electron Forma's export dialog: what to make, the video's options, and what it will contain.
    fn export_window(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.show_export {
            return;
        }
        let mut run = false;
        let pane = self.pane;
        let modal = pane_dialog(ui.ctx(), pane, ui.scope_id().with("forma-export"), |ui| {
            ui.set_width(400.0_f32.min(ui.available_width()));
            ui.label(
                egui::RichText::new("Export")
                    .font(font::bold(18.0))
                    .color(color::INK),
            );
            ui.separator();
            dialog_body(ui, pane, |ui| self.export_options(ui));
            ui.separator();
            let label = match self.export_kind {
                ExportKind::Video => "Export video",
                ExportKind::Svg => "Export SVG",
                ExportKind::Picture => "Save picture",
                ExportKind::Project => "Save project file",
                ExportKind::Lottie => "Export Lottie",
                ExportKind::StreamCuts => "Send to StreamCuts as template",
            };
            run = primary_button(ui, label, Mood::Ember).clicked();
        });
        if run {
            self.show_export = false;
            self.export(host, ui.ctx());
        } else if modal.close {
            self.show_export = false;
        }
    }

    fn export_options(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        let old_kind = self.export_kind;
        for (kind, title, detail) in [
            (
                ExportKind::Video,
                "Video",
                "MP4, WebM, MOV, GIF or PNG frames.",
            ),
            (
                ExportKind::Svg,
                "Animated SVG",
                "A small looping file for websites.",
            ),
            (
                ExportKind::Picture,
                "Picture",
                "A PNG of the frame at the playhead.",
            ),
            (
                ExportKind::Project,
                "Project file",
                "Everything, to open in Forma elsewhere.",
            ),
            (
                ExportKind::Lottie,
                "Lottie",
                "An animation in Bodymovin JSON.",
            ),
            (
                ExportKind::StreamCuts,
                "StreamCuts template",
                "An animated title with editable fields.",
            ),
        ] {
            ui.radio_value(
                &mut self.export_kind,
                kind,
                egui::RichText::new(format!("{title}  ·  {detail}")).font(font::ui(13.0)),
            );
        }
        if self.export_kind != old_kind {
            self.export_error = None;
        }
        ui.separator();
        let scene = &self.scene;
        if self.export_kind == ExportKind::Video {
            let settings = &mut self.video_settings;
            egui::Grid::new("forma-video-options")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Format");
                    let formats = [
                        (video::Format::Mp4, "MP4"),
                        (video::Format::WebM, "WebM (VP9)"),
                        (video::Format::Mov, "MOV (ProRes 4444)"),
                        (video::Format::PngSequence, "PNG sequence (ZIP)"),
                        (video::Format::Gif, "GIF"),
                    ];
                    egui::ComboBox::from_id_salt("video-format")
                        .selected_text(
                            formats
                                .iter()
                                .find(|(format, _)| *format == settings.format)
                                .map_or("MP4", |(_, name)| *name),
                        )
                        .show_ui(ui, |ui| {
                            for (format, name) in formats {
                                ui.selectable_value(&mut settings.format, format, name);
                            }
                        });
                    ui.end_row();
                    if settings.format == video::Format::Mp4 {
                        settings.transparent = false;
                    }
                    ui.label("Background");
                    ui.add_enabled(
                        settings.format != video::Format::Mp4,
                        egui::Checkbox::new(&mut settings.transparent, "Transparent"),
                    );
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
                        ui.label(
                            egui::RichText::new(format!("{w} × {h} pixels"))
                                .color(color::INK_MUTED),
                        );
                    });
                    ui.end_row();
                });
            ui.separator();
        }
        let summary = match self.export_kind {
            ExportKind::Video => {
                let sound = if !matches!(
                    self.video_settings.format,
                    video::Format::Gif | video::Format::PngSequence
                ) && scene.shapes.iter().any(|s| s.kind == "video")
                {
                    "  ·  Includes sound"
                } else {
                    ""
                };
                format!(
                    "{:.1} seconds · {} frames{sound}",
                    scene.duration,
                    self.video_settings.frames(scene)
                )
            }
            ExportKind::Svg => {
                let video = if scene.shapes.iter().any(|s| s.kind == "video") {
                    "  ·  Video layers show their first frame."
                } else {
                    ""
                };
                format!("{:.1} seconds, looping{video}", scene.duration)
            }
            ExportKind::Picture => format!(
                "The frame at {:.2} s · {} × {}",
                self.time, scene.width, scene.height
            ),
            ExportKind::Project => {
                let n = scene.shapes.len();
                format!(
                    "{n} layer{} with their pictures and video",
                    if n == 1 { "" } else { "s" }
                )
            }
            ExportKind::Lottie => format!("{:.1} seconds at 30 frames per second", scene.duration),
            ExportKind::StreamCuts => "Saves the open composition and its editable fields.".into(),
        };
        ui.label(egui::RichText::new(summary).font(font::bold(13.0)));
        if let Some(error) = &self.export_error {
            ui.label(error);
        }
        ui.add_space(6.0);
    }

    /// Makes the export chosen in the dialog, named after the project, into Files (a video on its own thread).
    fn export(&mut self, host: &mut dyn Host, ctx: &egui::Context) {
        let extension = match self.export_kind {
            ExportKind::Video => self.video_settings.extension(),
            ExportKind::Svg => "svg",
            ExportKind::Picture => "png",
            ExportKind::Project => "forma",
            ExportKind::Lottie => "lottie.json",
            ExportKind::StreamCuts => crate::template::EXTENSION,
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
            ExportKind::Video => match video::start(
                &self.scene,
                &self.place,
                path,
                self.video_settings,
                ctx.clone(),
            ) {
                Ok(export) => {
                    self.export = Some(export);
                    return;
                }
                Err(error) => Err(error),
            },
            ExportKind::Svg => animated_svg::animated(&self.scene, &self.place)
                .and_then(|svg| std::fs::write(&path, svg).map_err(|e| e.to_string()))
                .map(|_| path),
            ExportKind::Picture => {
                render::save_png(&self.scene, &self.place, self.time, &path).map(|_| path)
            }
            ExportKind::Project => archive::pack(&self.project_snapshot(), &self.place, &path).map(|_| path),
            ExportKind::Lottie => crate::lottie::export(&self.scene)
                .and_then(|data| std::fs::write(&path, data).map_err(|e| e.to_string())).map(|_| path),
            ExportKind::StreamCuts => {
                match crate::template::save(&self.scene, &self.place, &path) {
                    Ok(()) => {
                        if let Ok(request) = serde_json::to_string(&path) {
                            host.open_in("streamcuts", &format!("{}{request}", crate::template::OPEN_PREFIX));
                        }
                        Ok(path)
                    }
                    Err(error) => Err(error),
                }
            }
        };
        match done {
            Ok(path) => host.exported(path),
            Err(error) => {
                self.export_error = Some(error);
                self.show_export = true;
            }
        }
    }

    /// Electron's SVG editor: the markup, a preview, recoloring the whole artwork, and applying it to the layer.
    fn svg_window(&mut self, ui: &mut Ui) {
        let Some(edit) = &mut self.svg_edit else {
            return;
        };
        let mut apply = false;
        let pane = self.pane;
        let modal = pane_dialog(ui.ctx(), pane, ui.scope_id().with("forma-svg"), |ui| {
            ui.set_width(520.0_f32.min(ui.available_width()));
            ui.label(
                egui::RichText::new("Edit SVG")
                    .font(font::bold(18.0))
                    .color(color::INK),
            );
            let tree = svg::parse(&edit.source);
            dialog_body(ui, pane, |ui| {
                // The preview is drawn again only when the markup changes.
                let hash = {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    edit.source.hash(&mut hasher);
                    hasher.finish()
                };
                if self.svg_preview.as_ref().is_none_or(|(h, _)| *h != hash) {
                    self.svg_preview = tree.as_ref().ok().and_then(|tree| {
                        let (w, h) = svg::size(tree);
                        let fit = (480.0 / w).min(180.0 / h);
                        let pixmap = svg::rasterize(
                            tree,
                            (w * fit).max(1.0) as u32,
                            (h * fit).max(1.0) as u32,
                        )?;
                        let image = egui::ColorImage::from_rgba_premultiplied(
                            [pixmap.width() as usize, pixmap.height() as usize],
                            pixmap.data(),
                        );
                        Some((
                            hash,
                            ui.ctx().load_texture(
                                "forma-svg-preview",
                                image,
                                TextureOptions::LINEAR,
                            ),
                        ))
                    });
                }
                match (&tree, &self.svg_preview) {
                    (Ok(_), Some((_, texture))) => {
                        let response = ui.add(
                            egui::Image::new(texture)
                                .max_size(vec2(ui.available_width().min(480.0), 180.0))
                                .sense(egui::Sense::click()),
                        );
                        if let Some(pos) = response
                            .interact_pointer_pos()
                            .filter(|_| response.clicked())
                        {
                            let uv = (pos - response.rect.min) / response.rect.size();
                            let size = texture.size();
                            edit.selected_part = svg::hit_part(
                                &edit.source,
                                (uv.x * size[0] as f32) as u32,
                                (uv.y * size[1] as f32) as u32,
                                size[0] as u32,
                                size[1] as u32,
                            );
                            if let Some(index) = edit.selected_part {
                                if let Some(hex) = svg::part_color(&edit.source, index, "fill") {
                                    if hex.len() == 7 && hex.starts_with('#') {
                                        if let Ok(rgb) = u32::from_str_radix(&hex[1..], 16) {
                                            edit.color =
                                                [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        ui.label(
                            egui::RichText::new("Fix the SVG markup to preview it.")
                                .color(color::INK_MUTED),
                        );
                    }
                }
                ui.spacing_mut().scroll.floating = false;
                egui::ScrollArea::vertical()
                    .id_salt("forma-svg-source")
                    .max_height(200.0)
                    .show(ui, |ui| {
                        if ui
                            .add(
                                egui::TextEdit::multiline(&mut edit.source)
                                    .code_editor()
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(8),
                            )
                            .changed()
                        {
                            edit.selected_part = None;
                        }
                    });
                ui.horizontal_wrapped(|ui| {
                    egui::color_picker::color_edit_button_srgb(ui, &mut edit.color);
                    if ui
                        .button("Recolor")
                        .on_hover_text("Paint every fill and stroke this color")
                        .clicked()
                    {
                        let color = format!(
                            "#{:02x}{:02x}{:02x}",
                            edit.color[0], edit.color[1], edit.color[2]
                        );
                        match svg::recolor(&edit.source, &color) {
                            Ok(source) => {
                                (edit.source, edit.status) = (
                                    source,
                                    "Artwork recolored. Apply the changes to update the layer."
                                        .into(),
                                )
                            }
                            Err(error) => edit.status = error,
                        }
                    }
                });
                if let Some(index) = edit.selected_part {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "Selected: {}",
                            svg::part_name(&edit.source, index).unwrap_or_default()
                        ));
                        let color = format!(
                            "#{:02x}{:02x}{:02x}",
                            edit.color[0], edit.color[1], edit.color[2]
                        );
                        for (label, paint) in [("Set fill", "fill"), ("Set stroke", "stroke")] {
                            if ui.button(label).clicked() {
                                match svg::set_part_paint(&edit.source, index, paint, &color) {
                                    Ok(source) => {
                                        edit.source = source;
                                        edit.status = format!("{label} updated. Apply to save.");
                                    }
                                    Err(error) => edit.status = error,
                                }
                            }
                        }
                        if ui.button("Delete part").clicked() {
                            match svg::delete_part(&edit.source, index) {
                                Ok(source) => {
                                    edit.source = source;
                                    edit.selected_part = None;
                                    edit.status = "Part deleted. Apply to save.".into();
                                }
                                Err(error) => edit.status = error,
                            }
                        }
                    });
                }
                if !edit.status.is_empty() {
                    ui.label(egui::RichText::new(&edit.status).color(color::INK_MUTED));
                }
            });
            ui.separator();
            apply = ui
                .add_enabled(tree.is_ok(), egui::Button::new("Apply"))
                .clicked();
        });
        if apply && let Some(edit) = self.svg_edit.take() {
            if let Some(shape) = self.scene.shapes.iter_mut().find(|s| s.id == edit.id) {
                shape.extra.remove("svgSourceRef");
                shape.extra.insert("svgSource".into(), edit.source.into());
                self.edited();
            }
        } else if modal.close {
            self.svg_edit = None;
        }
    }

    /// A new project as Electron's start screen makes one: the example shapes, or an empty canvas, in the shape
    /// picked there (the example is scaled into portrait the way changing the format scales a scene).
    fn new_project(&mut self, example: bool) {
        let id = new_id();
        let mut scene = if example {
            Scene::example(id)
        } else {
            Scene::empty(id)
        };
        if self.start_portrait {
            scene.resize(1080.0, 1920.0);
        }
        self.open(scene);
        self.edited();
    }

    /// Electron's start screen ("What would you like to make?"): the canvas shape, three ways to begin, the projects
    /// saved here, and Open a file. It opens on a first launch and from Projects.
    fn projects_window(&mut self, ui: &mut Ui) {
        if !self.show_projects {
            return;
        }
        enum Choice {
            Shapes,
            StopMotion,
            Empty,
            Open(String),
            File,
            Back,
        }
        let mut choice = None;
        let current = self.main_id().to_owned();
        let modal = pane_modal(
            ui.ctx(),
            self.pane,
            ui.scope_id().with("forma-projects"),
            |ui| {
                ui.set_width(620.0_f32.min(ui.available_width()));
                ui.horizontal_wrapped(|ui| {
                    ui.heading("What would you like to make?");
                    if ui.button("Back to the editor").clicked() {
                        choice = Some(Choice::Back);
                    }
                });
                ui.label(egui::RichText::new(
                "Pick a starting point. You can mix shapes, text, pictures, and video in any project.",
            ).color(color::INK_MUTED));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.start_portrait, false, "Landscape");
                    ui.selectable_value(&mut self.start_portrait, true, "Portrait for phones");
                });
                ui.add_space(8.0);
                let wide = ui.available_width() >= 540.0;
                let cards = [
                    (
                        "Animate shapes",
                        "Move, spin, and fade shapes and text. Forma fills in the motion between the moments you set.",
                        Choice::Shapes,
                    ),
                    (
                        "Stop-motion from pictures",
                        "Pick your drawings in order. Each becomes a pose that shows for half a second, like a flip book.",
                        Choice::StopMotion,
                    ),
                    (
                        "Start from an empty canvas",
                        "Add your own shapes, text, pictures, or video.",
                        Choice::Empty,
                    ),
                ];
                let mut draw_card = |column: &mut Ui, (title, about, pick)| {
                    let card = egui::Frame::new()
                        .fill(color::VEIL)
                        .corner_radius(10.0)
                        .inner_margin(12.0)
                        .show(column, |ui| {
                            ui.set_min_height(96.0);
                            // Left-aligned: the columns' justified layout would spread the words out.
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.label(
                                    egui::RichText::new(title)
                                        .font(font::bold(14.0))
                                        .color(color::INK),
                                );
                                ui.label(
                                    egui::RichText::new(about)
                                        .font(font::ui(12.0))
                                        .color(color::INK_MUTED),
                                );
                            });
                        })
                        .response
                        .interact(Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if card.hovered() {
                        column.painter().rect_stroke(
                            card.rect,
                            10.0,
                            Stroke::new(1.5, color::SUN),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if card.clicked() {
                        choice = Some(pick);
                    }
                };
                if wide {
                    ui.columns(3, |uis| {
                        for (column, card) in uis.iter_mut().zip(cards) {
                            draw_card(column, card);
                        }
                    });
                } else {
                    for card in cards {
                        draw_card(ui, card);
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Your projects")
                            .font(font::bold(14.0))
                            .color(color::INK),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Open a file")
                            .on_hover_text("A .forma file or an editable JSON project")
                            .clicked()
                        {
                            choice = Some(Choice::File);
                        }
                    });
                });
                let projects = storage::list(&self.place);
                if projects.is_empty() {
                    ui.label(
                        egui::RichText::new("Nothing saved yet. Projects appear here as you work.")
                            .color(color::INK_MUTED),
                    );
                }
                egui::ScrollArea::vertical()
                    .id_salt("forma-start-projects")
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for doc in projects {
                            let open_now = if doc.id == current {
                                " · open now"
                            } else {
                                ""
                            };
                            let text = format!(
                                "{}{open_now}  ·  {}  ·  {}",
                                doc.name,
                                doc.detail,
                                saved_ago(doc.saved_at)
                            );
                            if ui
                                .add(
                                    egui::Button::new(&text)
                                        .min_size(vec2(ui.available_width(), 32.0))
                                        .truncate(),
                                )
                                .on_hover_text(text)
                                .clicked()
                            {
                                choice = Some(Choice::Open(doc.id));
                            }
                        }
                    });
            },
        );
        let close = match choice {
            Some(Choice::Shapes) => {
                self.new_project(true);
                self.notice = Some("New project created".into());
                true
            }
            Some(Choice::Empty) => {
                self.new_project(false);
                self.notice =
                    Some("Empty project created. Add something with the tools on the left.".into());
                true
            }
            Some(Choice::StopMotion) => {
                let picked = rfd::FileDialog::new()
                    .set_title("Pick the poses in order")
                    .add_filter("Pictures and SVG", &["png", "jpg", "jpeg", "webp", "svg"])
                    .pick_files();
                match picked {
                    Some(files) if !files.is_empty() => {
                        self.new_project(false);
                        self.pending_poses.extend(files);
                        true
                    }
                    _ => false,
                }
            }
            Some(Choice::Open(id)) => {
                if id != current {
                    match storage::load_document(&self.place, &id) {
                        Ok(scene) => {
                            self.open(scene);
                            self.notice = Some("Project opened".into());
                        }
                        Err(error) => self.notice = Some(error),
                    }
                }
                true
            }
            Some(Choice::File) => {
                let picked = rfd::FileDialog::new()
                    .set_title("Open a Forma project")
                    .add_filter("Forma projects", &["forma", "json"])
                    .pick_file();
                match picked {
                    Some(file) => {
                        self.pending_import.push(file);
                        true
                    }
                    None => false,
                }
            }
            Some(Choice::Back) => true,
            None => modal.close,
        };
        if close {
            self.show_projects = false;
        }
    }

    /// Space plays; Home and End go to the ends; arrows nudge the selected layers (Shift: 10 px) or, with none
    /// selected, step the playhead; [ and ] move the selected layer back and forward; Ctrl + and Ctrl - zoom the
    /// canvas and Ctrl+0 resets it.
    fn more_keys(&mut self, ui: &Ui) {
        use egui::{Key, Modifiers};
        let (play, home, end, arrow, big, back, forward, zoom, reset) = ui.input_mut(|i| {
            let big = i.modifiers.shift;
            let mut arrow = Vec2::ZERO;
            if !i.modifiers.command && !i.modifiers.alt {
                for (key, by) in [(Key::ArrowLeft, vec2(-1.0, 0.0)), (Key::ArrowRight, vec2(1.0, 0.0)), (Key::ArrowUp, vec2(0.0, -1.0)), (Key::ArrowDown, vec2(0.0, 1.0))] {
                    if i.key_pressed(key) {
                        arrow += by;
                    }
                }
            }
            let closer = i.consume_key(Modifiers::COMMAND, Key::Plus) | i.consume_key(Modifiers::COMMAND, Key::Equals);
            let zoom = if closer { 1.25 } else if i.consume_key(Modifiers::COMMAND, Key::Minus) { 0.8 } else { 1.0 };
            (
                i.consume_key(Modifiers::NONE, Key::Space),
                i.consume_key(Modifiers::NONE, Key::Home),
                i.consume_key(Modifiers::NONE, Key::End),
                arrow,
                big,
                i.consume_key(Modifiers::NONE, Key::OpenBracket),
                i.consume_key(Modifiers::NONE, Key::CloseBracket),
                zoom,
                i.consume_key(Modifiers::COMMAND, Key::Num0),
            )
        });
        if play {
            if !self.playing && self.time >= self.scene.duration {
                self.time = 0.0;
            }
            self.playing = !self.playing;
            self.preview_end = None;
            self.last_tick = Instant::now();
        }
        if home || end {
            self.playing = false;
            self.preview_end = None;
            self.time = if home { 0.0 } else { self.scene.duration };
        }
        if arrow != Vec2::ZERO {
            let mut layers = self.selection();
            if layers.is_empty() {
                // Nothing selected: Left and Right step the playhead (Shift: half a second).
                let step = if big { 0.5 } else { 0.05 };
                self.playing = false;
                self.time = (self.time + arrow.x as f64 * step).clamp(0.0, self.scene.duration);
            } else {
                // Text that follows a layer goes after the layers, so it keeps up with where they went.
                layers.sort_by_key(|i| self.scene.ancestor_depth(*i));
                let by = arrow * if big { 10.0 } else { 1.0 };
                self.playing = false;
                let poses: Vec<_> = layers
                    .into_iter()
                    .map(|index| {
                        let pose = self
                            .scene
                            .editable_pose(&self.scene.shapes[index], self.time);
                        (
                            index,
                            Pose {
                                x: pose.x + by.x,
                                y: pose.y + by.y,
                                ..pose
                            },
                        )
                    })
                    .collect();
                for (index, pose) in poses {
                    self.scene
                        .set_property_pose(index, self.time, pose, Property::Position);
                }
                self.edited();
            }
        }
        if back || forward {
            let at = self.selected.as_ref().and_then(|id| self.scene.shapes.iter().position(|s| &s.id == id));
            let to = at.and_then(|at| if forward { (at + 1 < self.scene.shapes.len()).then_some(at + 1) } else { at.checked_sub(1) });
            if let (Some(at), Some(to)) = (at, to) {
                self.scene.shapes.swap(at, to);
                self.edited();
                self.notice = Some(if forward { "Moved the layer forward".into() } else { "Moved the layer back".into() });
            }
        }
        if zoom != 1.0 {
            // Around the middle of the canvas, where the pan is measured from.
            let old = self.zoom;
            self.zoom = (self.zoom * zoom).clamp(0.2, 5.0);
            self.pan *= self.zoom / old;
        }
        if reset {
            (self.zoom, self.pan) = (1.0, Vec2::ZERO);
        }
    }

    fn help_window(&mut self, ui: &mut Ui) {
        if !self.show_help {
            return;
        }
        let mut close = false;
        let pane = self.pane;
        let modal = pane_dialog(ui.ctx(), pane, ui.scope_id().with("forma-help"), |ui| {
            ui.set_width(470.0_f32.min(ui.available_width()));
            ui.heading("Make your first animation");
            dialog_body(ui, pane, |ui| {
                ui.add_space(8.0);
                for step in [
                    "1. Add a layer. Use + Circle, + Text, or + Picture, SVG or video on the left.",
                    "2. Set a moment. Drag the playhead in the timeline, then drag, resize, or recolor the layer. Forma remembers it as a keyframe.",
                    "3. Make it move. Use Move, Spin, Fade, or Drop under the canvas, or set another moment.",
                    "4. Stop-motion. Give a picture an end time, press Next pose after this, and drag the copy.",
                ] {
                    ui.label(step);
                    ui.add_space(6.0);
                }
                ui.separator();
                ui.label("Press Ctrl + Z to undo and S to split a clip at the playhead. Press play to preview, then Export.");
                ui.add_space(4.0);
                ui.label("Keys: Space plays. Arrows nudge the selected layers (Shift: 10 px), or step the playhead with none selected. [ and ] move a layer back and forward. Home and End go to the ends. Ctrl + and Ctrl - zoom, Ctrl+0 resets.");
                ui.add_space(8.0);
            });
            ui.separator();
            close = ui.button("Got it").clicked();
        });
        if close || modal.close {
            self.show_help = false;
        }
    }
}

impl CanopyApp for Forma {
    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.pane = ui.max_rect();
        if !self.pending_poses.is_empty() {
            // As Electron's importPoses: the pictures come in, then line up as poses from the start.
            let before = self.scene.shapes.len();
            let files = std::mem::take(&mut self.pending_poses)
                .into_iter()
                .map(SharedFile::new)
                .collect();
            self.receive(files, host);
            let poses: Vec<usize> = (before..self.scene.shapes.len())
                .filter(|&i| self.scene.shapes[i].kind != "video")
                .collect();
            if !poses.is_empty() {
                self.scene.arrange_as_poses(&poses, 0.5, new_id());
                (self.selected, self.time) = (Some(self.scene.shapes[poses[0]].id.clone()), 0.0);
                self.edited();
                host.toast(format!(
                    "{} pose{} added. Press play, or drag a pose to move it.",
                    poses.len(),
                    if poses.len() == 1 { "" } else { "s" }
                ));
            }
        }
        if !self.pending_import.is_empty() {
            let files = std::mem::take(&mut self.pending_import)
                .into_iter()
                .map(SharedFile::new)
                .collect();
            let receipt = self.receive(files, host);
            if let Some(note) = receipt.note {
                host.toast(note);
            } else if !receipt.skipped.is_empty() {
                host.toast(format!("Forma can't open {}", receipt.skipped.join(", ")));
            }
        }
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
        let before = self.project_snapshot();
        self.run_auto_preview(ui.ctx());
        self.automation(ui.ctx(), host);
        // With a dialog up, the editor's keys wait (Delete would otherwise remove a layer behind the Export dialog).
        let dialog = self.show_projects || self.show_compositions || self.show_help || self.show_export || self.svg_edit.is_some();
        self.vector_keyboard = host.focused() && !dialog && !ui.ctx().text_edit_focused();
        if host.focused() && !dialog {
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
            let mask_shortcut = self.mask_editor.editing_points() && !ui.ctx().text_edit_focused()
                && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
            let vector_shortcut = self.vector_tools.tool.is_some() && !ui.ctx().text_edit_focused()
                && ui.input(|i| [egui::Key::Escape, egui::Key::Enter, egui::Key::Delete, egui::Key::Backspace]
                    .iter().any(|&key| i.key_pressed(key)));
            let key_shortcut = !ui.ctx().text_edit_focused() && {
                let (copy, paste, delete) = ui.input(|i| {
                    let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy))
                        || (i.modifiers.command && i.key_pressed(egui::Key::C));
                    let paste = !i.modifiers.shift
                        && (i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))
                            || (i.modifiers.command && i.key_pressed(egui::Key::V)));
                    let delete =
                        i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace);
                    (copy, paste, delete)
                });
                self.key_editor.begin_frame(&self.scene);
                self.key_editor
                    .shortcut(&mut self.scene, self.time, copy, paste, delete)
            };
            if key_shortcut {
                self.vector_keyboard = false;
            }
            if !key_shortcut && !mask_shortcut && !vector_shortcut && !ui.ctx().egui_wants_keyboard_input() {
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) {
                    self.multi_selected = self
                        .scene
                        .shapes
                        .iter()
                        .map(|shape| shape.id.clone())
                        .collect();
                    self.selected = self.multi_selected.last().cloned();
                }
                // Escape stops drawing a move, or goes back from several selected layers to one (Electron's clips.js).
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    if self.camera_tool {
                        self.camera_tool = false;
                    } else if self.mask_editor.active() {
                        self.mask_editor.escape();
                    } else if self.motion_mode.is_some() {
                        (self.motion_mode, self.motion_draft) = (None, None);
                    } else if self.multi_selected.len() > 1 {
                        self.multi_selected = self.selected.clone().into_iter().collect();
                    }
                }
                let action = ui.input(|i| {
                    let command = i.modifiers.command;
                    let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy))
                        || (command && i.key_pressed(egui::Key::C));
                    let paste = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))
                        || (command && i.key_pressed(egui::Key::V));
                    if copy {
                        Some(ClipAction::Copy)
                    } else if paste && i.modifiers.shift {
                        Some(ClipAction::PasteAfter)
                    } else if paste {
                        Some(ClipAction::Paste)
                    } else if command && !i.modifiers.shift && i.key_pressed(egui::Key::D) {
                        Some(ClipAction::Duplicate)
                    } else if !command
                        && !i.modifiers.alt
                        && !i.modifiers.shift
                        && i.key_pressed(egui::Key::S)
                    {
                        Some(ClipAction::Split)
                    } else if i.key_pressed(egui::Key::Delete)
                        || i.key_pressed(egui::Key::Backspace)
                    {
                        Some(ClipAction::Delete)
                    } else {
                        None
                    }
                });
                if let Some(action) = action {
                    self.clip_action(action);
                }
                self.more_keys(ui);
            }
        }
        if self.playing {
            let now = Instant::now();
            self.time += now.duration_since(self.last_tick).as_secs_f64();
            self.last_tick = now;
            if let Some(end) = self.preview_end.filter(|end| self.time >= *end) {
                (self.time, self.playing, self.preview_end) = (end, false, None);
            } else if self.time >= self.scene.duration {
                if self.loop_playback {
                    self.time %= self.scene.duration;
                } else {
                    self.time = self.scene.duration;
                    self.playing = false;
                }
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
        self.workspace(
            ui,
            Rect::from_min_max(pos2(rect.left(), top.bottom() + 4.0), rect.max),
        );
        if let Some(notice) = self.notice.take() {
            host.toast(notice);
        }
        self.projects_window(ui);
        self.compositions_window(ui);
        self.help_window(ui);
        self.export_window(ui, host);
        self.svg_window(ui);
        if !self.suppress_history && before.project_id == self.main_id() && self.project_differs(&before) {
            let dragging = ui.input(|i| i.pointer.any_down());
            if !self.recording_drag {
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
        if !self
            .sound
            .update(&self.scene, self.renderer.as_ref(), self.time, self.playing)
        {
            self.playing = false;
            self.preview_end = None;
        }
        if let Some(notice) = self.sound.take_notice() {
            self.notice = Some(notice);
        }
        if let Some(notice) = self.notice.take() {
            host.toast(notice);
        }
    }

    fn receive(&mut self, files: Vec<SharedFile>, host: &mut dyn Host) -> Receipt {
        let mut receipt = Receipt::default();
        for file in files {
            let lower = file.name.to_lowercase();
            if lower.ends_with(".json") || lower.ends_with(".forma") {
                let read = if lower.ends_with(".forma") {
                    archive::unpack(&file.path, &self.place)
                } else {
                    storage::load(&file.path)
                };
                match read {
                    // Like Electron Forma, an opened file becomes a project of its own: saving it never replaces another.
                    Ok(mut scene) => {
                        scene.project_id = new_id();
                        scene.project_name = match scene.project_name.trim() {
                            "" => file
                                .name
                                .rsplit_once('.')
                                .map_or(file.name.as_str(), |(stem, _)| stem)
                                .to_owned(),
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
                    assets::import_svg(&file.path)
                        .map(|(source, w, h)| ("svg", "svgSource", source, w, h, None))
                } else if [".mp4", ".webm", ".ogv"]
                    .iter()
                    .any(|ext| lower.ends_with(ext))
                {
                    assets::import_video(&file.path, &self.place).map(|(url, poster, info)| {
                        (
                            "video",
                            "assetData",
                            url,
                            info.width as f32,
                            info.height as f32,
                            Some((poster, info.duration)),
                        )
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
        let result = storage::save(&self.place, &self.project_snapshot());
        self.edited_at = None;
        self.save_failed = result.is_err();
        result
    }

    fn title(&self) -> String {
        self.scene.project_name.clone()
    }
}

/// What a dialog over Forma's pane returns: its contents' result, and whether it should close (its backdrop was
/// clicked, or Escape pressed).
struct PaneModal<R> {
    #[allow(dead_code)]
    inner: R,
    close: bool,
}

/// A dialog over Forma's own pane: egui's Modal dims and blocks the whole window, which inside Canopy froze the
/// sidebar and the other pane too. This one dims only `pane`, sits in its middle and scrolls when the pane is short.
fn pane_modal<R>(
    ctx: &egui::Context,
    pane: Rect,
    id: egui::Id,
    content: impl FnOnce(&mut Ui) -> R,
) -> PaneModal<R> {
    pane_dialog(ctx, pane, id, |ui| {
        ui.spacing_mut().scroll.floating = false;
        egui::ScrollArea::vertical()
            .max_height((pane.height() - 60.0).max(0.0))
            .show(ui, content)
            .inner
    })
}

fn dialog_body(ui: &mut Ui, pane: Rect, content: impl FnOnce(&mut Ui)) {
    ui.spacing_mut().scroll.floating = false;
    egui::ScrollArea::vertical()
        .max_height((pane.height() - 134.0).max(0.0))
        .show(ui, content);
}

fn pane_dialog<R>(
    ctx: &egui::Context,
    pane: Rect,
    id: egui::Id,
    content: impl FnOnce(&mut Ui) -> R,
) -> PaneModal<R> {
    let pane = if pane.is_positive() {
        pane
    } else {
        ctx.content_rect()
    };
    let backdrop = egui::Area::new(id.with("backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(pane.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.painter().rect_filled(pane, 10.0, Color32::from_black_alpha(110));
            // Takes the clicks meant for the editor underneath.
            ui.allocate_response(pane.size(), Sense::click())
        })
        .inner;
    let inner = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .sense(Sense::hover())
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(pane.center())
        .constrain_to(pane.shrink(8.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .show(ui, |ui| {
                    ui.set_max_width((pane.width() - 48.0).max(200.0));
                    ui.set_clip_rect(ui.clip_rect().intersect(pane));
                    ui.spacing_mut().interact_size.y = 32.0;
                    content(ui)
                })
                .inner
        })
        .inner;
    let close = backdrop.clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape));
    PaneModal { inner, close }
}

fn paint_clip_label(ui: &Ui, rect: Rect, label: String, ink: Color32) {
    if rect.width() < 24.0 {
        return;
    }
    let rect = rect.shrink2(vec2(5.0, 0.0));
    let mut job = egui::text::LayoutJob::simple_singleline(label, font::ui(10.0), ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width());
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        rect.left_center() - vec2(0.0, galley.size().y / 2.0),
        galley,
        ink,
    );
}

fn point_in_quad(point: Pos2, quad: &[Pos2]) -> bool {
    let mut sides = quad
        .iter()
        .zip(quad.iter().cycle().skip(1))
        .map(|(&a, &b)| {
            let edge = b - a;
            let offset = point - a;
            edge.x * offset.y - edge.y * offset.x
        });
    let Some(first) = sides.find(|side| side.abs() > 1e-4) else {
        return false;
    };
    sides.all(|side| side.abs() <= 1e-4 || side.signum() == first.signum())
}

fn spatial_field(ui: &mut Ui, shape: &mut Shape, property: crate::spatial::Property, time: f64) {
    let mut values = shape.spatial_value(property, time);
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(property.label());
        for (i, channel) in property.channels().iter().enumerate() {
            if property.channels().len() > 1 {
                ui.label(*channel);
            }
            let range = property.clamp(-f32::MAX)..=property.clamp(f32::MAX);
            changed |= ui
                .add_sized(
                    [63.0, 32.0],
                    egui::DragValue::new(&mut values[i])
                        .speed(0.5)
                        .range(range)
                        .max_decimals(2),
                )
                .changed();
        }
    });
    if changed {
        shape.set_spatial(property, values, time);
    }
}

/// When a project was saved, said the way people say it ("just now", "5 minutes ago", "3 days ago").
fn saved_ago(saved_at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let minutes = now.saturating_sub(saved_at) / 60_000;
    let (amount, unit) = match minutes {
        0 => return "just now".into(),
        m if m < 60 => (m, "minute"),
        m if m < 60 * 24 => (m / 60, "hour"),
        m => (m / (60 * 24), "day"),
    };
    format!("{amount} {unit}{} ago", if amount == 1 { "" } else { "s" })
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

#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_forma() -> Forma {
        let root = std::env::temp_dir().join(format!("forma-preview-{}", new_id()));
        Forma::new(Place {
            space: None,
            projects_dir: root.clone(),
            data_dir: root.clone(),
            exports_dir: root,
        })
    }

    fn layout_at_default_width(height: f32, layout: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let ctx = egui::Context::default();
        canopy_ui::install(&ctx);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1456.0, height))),
                ..Default::default()
            },
            layout,
        );
        output.textures_delta.clear();
        output
    }

    #[test]
    fn timeline_rows_stay_separate_with_one_expanded_layer() {
        let mut forma = test_forma();
        forma.scene.add_layer("rect".into(), "rect");
        forma.scene.add_text("text".into());
        forma.scene.add_layer("circle".into(), "circle");
        forma.key_editor.toggle_layer("text");
        let text = &mut forma.scene.shapes[1];
        text.text_layout
            .as_mut()
            .unwrap()
            .add(crate::text_animator::Animator::default());
        text.effects.push(crate::effects::Effect::new(
            1,
            crate::effects::Kind::GaussianBlur,
        ));
        text.masks.push(crate::compositing::LayerMask {
            id: 1,
            shape: crate::compositing::MaskShape::Rectangle,
            mode: crate::compositing::MaskMode::Add,
            inverted: false,
            reference_size: [100.0, 100.0],
            rest: crate::compositing::MaskPose::new([0.0, 0.0], [100.0, 100.0]),
            keys: Vec::new(),
        });

        let output = layout_at_default_width(900.0, |ui| forma.timeline(ui, None));
        // Each row paints a wide veil behind the clip, keys or subgroup. Its vertical inset is 3 px.
        let rows: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|painted| match &painted.shape {
                egui::Shape::Rect(rect) if rect.fill == color::VEIL && rect.rect.width() > 1000.0 => {
                    Some(rect.rect.expand2(vec2(0.0, 3.0)))
                }
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), forma.rows().len());
        assert!(rows.iter().all(|row| row.height() >= 32.0));
        for pair in rows.windows(2) {
            assert!(
                pair[1].top() >= pair[0].bottom(),
                "Overlapping rows: {pair:?}"
            );
        }
    }

    #[test]
    fn the_tool_rail_and_canvas_numbers_fit_on_one_line() {
        let mut forma = test_forma();
        forma.selected = Some(forma.scene.add_layer("rect".into(), "rect"));
        forma.zoom = 0.9;
        let rail = Rect::from_min_size(Pos2::ZERO, vec2(88.0, 600.0));
        let canvas = Rect::from_min_size(pos2(270.0, 0.0), vec2(922.0, 600.0));
        let output = layout_at_default_width(920.0, |ui| {
            let mut tools = ui.new_child(UiBuilder::new().max_rect(rail.shrink2(vec2(4.0, 8.0))));
            forma.rail(&mut tools);
            forma.canvas_bar(ui, canvas);
        });
        let text: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|painted| match &painted.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        let size = format!(
            "Landscape \u{00b7} {} \u{00d7} {}",
            forma.scene.width, forma.scene.height
        );
        for (label, bounds) in [
            ("Pen", rail),
            ("Mask", rail),
            (size.as_str(), canvas),
            ("90%", canvas),
        ] {
            let found = text
                .iter()
                .find(|shape| shape.galley.text() == label)
                .unwrap_or_else(|| panic!("Missing {label:?}"));
            assert_eq!(found.galley.rows.len(), 1, "{label:?} wrapped");
            assert!(bounds.contains_rect(found.galley.rect.translate(found.pos.to_vec2())));
        }
    }

    #[test]
    fn bundled_ui_fonts_cover_anchor_disclosure_and_shutter_symbols() {
        layout_at_default_width(900.0, |ui| {
            ui.fonts_mut(|fonts| {
                let symbols = egui::FontId::monospace(14.0);
                for glyph in ANCHOR_GLYPHS
                    .into_iter()
                    .flatten()
                    .chain([DISCLOSURE_OPEN, DISCLOSURE_CLOSED])
                {
                    assert!(fonts.has_glyphs(&symbols, glyph), "Missing {glyph:?}");
                }
                assert!(fonts.has_glyph(&font::ui(14.0), '\u{00b0}'));
            });
        });
    }

    #[test]
    fn project_edit_invalidates_cached_playback_frames() {
        let mut forma = test_forma();
        let key = (0.0, ((0.0, 0.0), 1.0, (2, 2)), true);
        let pixels = std::sync::Arc::new(tiny_skia::Pixmap::new(2, 2).unwrap());
        forma
            .frame_cache
            .insert(key, (pixels, render::TextHits::new()), 16);
        assert!(forma.frame_cache.get(&key).is_some());
        forma.scene.bg = "#334861".into();
        forma.edited();
        assert!(forma.frame_cache.get(&key).is_none());
        assert_eq!(forma.frame_cache.bytes(), 0);
    }

    #[test]
    fn fade_preview_reaches_reappearance_and_stops_at_scene_end() {
        let mut scene = Scene::example("test".into());
        let fade = model::Disappear {
            start_time: 1.0,
            duration: 0.5,
            reappear_mode: "time".into(),
            reappear_time: Some(2.5),
            trigger_shape_id: None,
            trigger_clip_id: None,
            extra: Default::default(),
        };
        scene.shapes[0].disappear = Some(fade);
        assert_eq!(
            fade_preview_range(&scene, &scene.shapes[0]),
            Some((0.75, 2.75))
        );
        scene.shapes[0].disappear.as_mut().unwrap().reappear_time = Some(4.0);
        assert_eq!(
            fade_preview_range(&scene, &scene.shapes[0]),
            Some((0.75, 4.0))
        );
    }
}
