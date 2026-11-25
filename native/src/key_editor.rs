use crate::compositing::MaskProperty;
use crate::keyframes::{EASINGS, Lane, LaneKey, Property, same_time};
use crate::model::{Scene, Shape, ease_at, hundredths};
use canopy_ui::{Accent, color, font};
use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;

pub(crate) const LABEL_WIDTH: f32 = 166.0;
pub(crate) const TIME_MARGIN: f32 = 14.0;

#[derive(Clone, Copy)]
pub struct TimeView {
    pub start: f64,
    pub span: f64,
}

impl TimeView {
    pub fn contains(self, time: f64) -> bool {
        (self.start..=self.start + self.span).contains(&time)
    }

    pub fn includes_span(self, start: f64, end: f64) -> bool {
        end >= self.start && start <= self.start + self.span
    }

    pub fn x(self, plot: Rect, t: f64) -> f32 {
        plot.left() + plot.width() * ((t - self.start) / self.span) as f32
    }

    pub fn time(self, plot: Rect, x: f32) -> f64 {
        self.start + (x - plot.left()) as f64 / plot.width().max(1.0) as f64 * self.span
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Track {
    layer: String,
    lane: Lane,
}

#[derive(Clone, Debug)]
struct Pick {
    track: Track,
    time: f64,
}

impl Pick {
    fn matches(&self, other: &Self) -> bool {
        self.track == other.track && same_time(self.time, other.time)
    }
}

#[derive(Clone)]
struct Plot {
    rect: Rect,
    view: TimeView,
    values: RangeInclusive<f32>,
}

impl Plot {
    fn point(&self, t: f64, value: f32) -> Pos2 {
        let fraction = (value - self.values.start()) / (self.values.end() - self.values.start());
        pos2(
            self.view.x(self.rect, t),
            self.rect.bottom() - fraction * self.rect.height(),
        )
    }

    fn value(&self, y: f32) -> f32 {
        let fraction = (self.rect.bottom() - y) / self.rect.height();
        self.values.start() + fraction * (self.values.end() - self.values.start())
    }
}

struct PointHit {
    at: Pos2,
    time: f64,
    channel: usize,
}

#[derive(Clone)]
struct Handle {
    at: Pos2,
    from: (f64, f32),
    to: (f64, f32),
    index: usize,
    offset: egui::Vec2,
}

struct KeyDrag {
    source: Pick,
    origin: Pos2,
    plot: Plot,
    graph_key: Option<(usize, LaneKey)>,
    picks: Vec<Pick>,
    delta: f64,
}

struct HandleDrag {
    track: Track,
    handle: Handle,
    plot: Plot,
}

struct BoxDrag {
    origin: Pos2,
    area: Rect,
    extend: Vec<Pick>,
}

enum Drag {
    Keys(KeyDrag),
    Handle(HandleDrag),
    Box(BoxDrag),
}

#[derive(Default)]
pub(crate) struct KeyEditor {
    pub expanded: HashSet<String>,
    expanded_masks: HashMap<String, HashSet<u64>>,
    graph: bool,
    lane_focus: bool,
    graph_tracks: HashSet<Track>,
    channels: HashMap<Track, usize>,
    timeline_view: Option<TimeView>,
    selected: Vec<Pick>,
    clipboard: Vec<(Track, LaneKey)>,
    menu: Option<Pick>,
    drag: Option<Drag>,
    frame_points: Vec<(Pick, Pos2)>,
    view: Option<TimeView>,
}

impl KeyEditor {
    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.drag = None;
        self.lane_focus = false;
    }

    pub fn begin_frame(&mut self, scene: &Scene) {
        self.frame_points.clear();
        self.selected.retain(|pick| key_at(scene, pick).is_some());
        self.graph_tracks
            .retain(|track| track_shape(scene, track).is_some());
        self.expanded_masks.retain(|id, masks| {
            if let Some(shape) = scene.shapes.iter().find(|s| s.id == *id) {
                masks.retain(|id| shape.mask(*id).is_some());
            } else {
                masks.clear();
            }
            !masks.is_empty()
        });
    }

    pub fn time_view(&mut self, ui: &Ui, plot: Rect, duration: f64) -> TimeView {
        let full = TimeView {
            start: 0.0,
            span: duration,
        };
        let mut view = self.view.unwrap_or(full);
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|&p| plot.contains(p));
        if let Some(pointer) = pointer.filter(|_| self.drag.is_none()) {
            let (zoom, pan) = ui.input_mut(|i| {
                let scroll = i.smooth_scroll_delta();
                let pan = if i.modifiers.shift {
                    i.smooth_scroll_delta.y = 0.0;
                    scroll.x + scroll.y
                } else {
                    scroll.x
                };
                (i.zoom_delta(), pan)
            });
            if zoom != 1.0 {
                let at = view.time(plot, pointer.x);
                let span = (view.span / f64::from(zoom)).clamp(0.12, duration);
                view.start = at - (at - view.start) * span / view.span;
                view.span = span;
            } else if pan != 0.0 {
                view.start -= f64::from(pan / plot.width().max(1.0)) * view.span;
            }
            if zoom != 1.0 || pan != 0.0 {
                self.view = Some(view);
            }
        }
        view.span = view.span.clamp(0.12, duration);
        view.start = view.start.clamp(0.0, (duration - view.span).max(0.0));
        if self.view.is_some() {
            self.view = Some(view);
        }
        view
    }

    pub fn toolbar(&mut self, ui: &mut Ui, scene: &mut Scene, selected_layer: Option<&str>) {
        let toggle = egui::Button::selectable(self.graph, "Graph").min_size(vec2(0.0, 32.0));
        if ui
            .add(toggle)
            .on_hover_text("Show the curves of selected property lanes")
            .clicked()
        {
            self.graph = !self.graph;
            if self.graph && self.graph_tracks.is_empty() {
                if let Some(id) =
                    selected_layer.or_else(|| scene.shapes.first().map(|s| s.id.as_str()))
                {
                    self.expanded.insert(id.to_owned());
                    self.graph_tracks.insert(Track {
                        layer: id.to_owned(),
                        lane: Lane::ALL[0],
                    });
                }
            }
            if self.graph {
                self.timeline_view = self.view;
                self.expanded
                    .extend(self.graph_tracks.iter().map(|track| track.layer.clone()));
                self.fit(scene);
            } else {
                self.view = self.timeline_view.take();
            }
        }
        if self.graph
            && ui
                .add(egui::Button::new("Fit").min_size(vec2(0.0, 32.0)))
                .on_hover_text("Fit the selected curves. Ctrl+wheel zooms; Shift+wheel pans.")
                .clicked()
        {
            self.fit(scene);
        }
        if !self.selected.is_empty() {
            let mut kinds = self.selected.iter().filter_map(|pick| {
                let shape = track_shape(scene, &pick.track)?;
                let key = shape.lane_key(pick.track.lane, pick.time)?;
                Some(easing_label(key.easing(&shape.ease).0))
            });
            let first = kinds.next().unwrap_or("Easing");
            let label = if kinds.all(|label| label == first) {
                first
            } else {
                "Mixed easing"
            };
            egui::ComboBox::from_id_salt("key-easing")
                .height(240.0)
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (label, kind) in EASINGS {
                        if ui.selectable_label(false, label).clicked() {
                            self.apply_easing(scene, kind);
                        }
                    }
                })
                .response
                .on_hover_text("Easing after the selected keys");
        }
    }

    fn fit(&mut self, scene: &Scene) {
        let times = self
            .graph_tracks
            .iter()
            .filter_map(|track| Some(track_shape(scene, track)?.lane_keys(track.lane)))
            .flatten();
        let (lo, hi) = times.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), k| {
            (lo.min(k.time()), hi.max(k.time()))
        });
        if !lo.is_finite() {
            self.view = None;
            return;
        }
        let pad = ((hi - lo) * 0.12).max(0.3);
        let start = (lo - pad).clamp(0.0, scene.duration - 0.12);
        self.view = Some(TimeView {
            start,
            span: (hi + pad).min(scene.duration) - start,
        });
    }

    pub fn row_height(&self, id: &str, lane: Lane) -> f32 {
        if self.graph
            && self
                .graph_tracks
                .iter()
                .any(|t| t.layer == id && t.lane == lane)
        {
            136.0f32.max(34.0 + lane.channels().len() as f32 * 32.0)
        } else {
            32.0
        }
    }

    pub fn toggle_layer(&mut self, id: &str) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_owned());
        }
    }

    pub fn mask_expanded(&self, layer: &str, mask: u64) -> bool {
        self.expanded_masks
            .get(layer)
            .is_some_and(|ids| ids.contains(&mask))
    }

    pub fn reveal_mask(&mut self, layer: String, mask: u64) {
        self.expanded.insert(layer.clone());
        self.expanded_masks.entry(layer).or_default().insert(mask);
    }

    pub fn toggle_mask(&mut self, layer: &str, mask: u64) {
        let ids = self.expanded_masks.entry(layer.to_owned()).or_default();
        if !ids.remove(&mask) {
            ids.insert(mask);
        }
    }

    pub fn row(
        &mut self,
        ui: &mut Ui,
        scene: &mut Scene,
        index: usize,
        lane: Lane,
        line: Rect,
        view: TimeView,
        playhead: &mut f64,
        playing: &mut bool,
    ) -> bool {
        let track = Track {
            layer: scene.shapes[index].id.clone(),
            lane,
        };
        let label = Rect::from_min_size(line.min + vec2(14.0, 0.0), vec2(87.0, 32.0));
        let add = Rect::from_min_size(label.right_top() + vec2(3.0, 0.0), vec2(50.0, 32.0));
        let active = self.graph_tracks.contains(&track);
        let title = match lane {
            Lane::Vector(crate::shape_style::VectorProperty::GradientStop(_, i)) => format!("{} {}", lane.label(), i + 1),
            _ => lane.label().to_owned(),
        };
        let label_response = ui
            .put(label, egui::Button::selectable(active, title))
            .on_hover_text("Select this lane for the graph. Shift or Ctrl selects several lanes.");
        let mut selected_layer = label_response.clicked();
        if selected_layer {
            self.lane_focus = true;
            self.pick_track(&track, extend(ui), true);
        }
        let on_key = scene.shapes[index]
            .lane_key(lane, hundredths(*playhead))
            .is_some();
        if ui
            .put(
                add,
                egui::Button::selectable(on_key, if on_key { "On key" } else { "Add" }),
            )
            .on_hover_text(if on_key {
                "Select the key at the playhead"
            } else {
                "Add a key at the playhead"
            })
            .clicked()
        {
            let time = hundredths(*playhead);
            scene.shapes[index].add_lane_key(lane, time);
            self.select(&track, time, extend(ui), false);
            selected_layer = true;
            *playing = false;
        }

        let rect = Rect::from_min_max(
            pos2(line.left() + LABEL_WIDTH, line.top() + 3.0),
            pos2(
                (line.right() - TIME_MARGIN).max(line.left() + LABEL_WIDTH + 1.0),
                line.bottom() - 3.0,
            ),
        );
        let hit_rect = rect.expand2(vec2(7.0, 0.0));
        let painter = ui.painter().with_clip_rect(hit_rect);
        painter.rect_filled(hit_rect, 4.0, color::VEIL);
        let response = ui.interact(
            hit_rect,
            ui.scope_id().with(("property", &track)),
            Sense::click_and_drag(),
        );
        if response.has_focus() {
            painter.rect_stroke(
                hit_rect,
                4.0,
                Stroke::new(1.0, color::LEAF),
                StrokeKind::Inside,
            );
        }
        let keys = scene.shapes[index].lane_keys(lane);
        let graph = self.graph && active;
        let (plot, points, handles, channel) = if graph {
            let plot = self.graph_plot(scene, index, &track, rect, view, &keys, *playhead);
            let channel = self.graph_channels(ui, &track, line);
            let mut handles = Vec::new();
            let points = self.draw_graph(
                ui,
                &track,
                &scene.shapes[index],
                &keys,
                &plot,
                channel,
                *playhead,
                &mut handles,
            );
            (plot, points, handles, channel)
        } else {
            let points = self.draw_diamonds(&painter, &track, &keys, rect, view);
            (
                Plot {
                    rect,
                    view,
                    values: 0.0..=1.0,
                },
                points,
                Vec::new(),
                0,
            )
        };
        let points: Vec<_> = points
            .into_iter()
            .filter(|p| hit_rect.intersect(ui.clip_rect()).contains(p.at))
            .collect();
        self.frame_points.extend(points.iter().map(|p| {
            (
                Pick {
                    track: track.clone(),
                    time: p.time,
                },
                p.at,
            )
        }));
        self.interact(
            ui, &response, scene, &track, plot, &points, &handles, graph, channel, playhead,
            playing,
        ) || selected_layer
    }

    fn graph_channels(&mut self, ui: &mut Ui, track: &Track, line: Rect) -> usize {
        let channel = self.channels.entry(track.clone()).or_default();
        for (i, &label) in track.lane.channels().iter().enumerate() {
            let at = line.min + vec2(14.0, 34.0 + i as f32 * 32.0);
            let text = egui::RichText::new(label).color(channel_color(i));
            let button = egui::Button::selectable(*channel == i, text);
            if ui
                .put(Rect::from_min_size(at, vec2(100.0, 32.0)), button)
                .on_hover_text("Choose this value where graph points overlap")
                .clicked()
            {
                *channel = i;
            }
        }
        *channel
    }

    fn pick_track(&mut self, track: &Track, extend: bool, toggle: bool) {
        if !extend && (toggle || !self.graph_tracks.contains(track)) {
            self.graph_tracks.clear();
        }
        if !toggle || !extend || !self.graph_tracks.remove(track) {
            self.graph_tracks.insert(track.clone());
        }
    }

    fn select(&mut self, track: &Track, time: f64, extend: bool, toggle: bool) {
        self.lane_focus = true;
        let pick = Pick {
            track: track.clone(),
            time,
        };
        if !extend {
            self.selected.clear();
        }
        match self.selected.iter().position(|p| p.matches(&pick)) {
            Some(index) if toggle => {
                self.selected.remove(index);
            }
            None => self.selected.push(pick),
            _ => {}
        }
        self.pick_track(track, extend, false);
    }

    fn picked(&self, track: &Track, time: f64) -> bool {
        self.selected
            .iter()
            .any(|p| &p.track == track && same_time(p.time, time))
    }

    fn draw_diamonds(
        &self,
        painter: &egui::Painter,
        track: &Track,
        keys: &[LaneKey],
        rect: Rect,
        view: TimeView,
    ) -> Vec<PointHit> {
        keys.iter()
            .filter_map(|key| {
                let time = key.time();
                let at = pos2(view.x(rect, time), rect.center().y);
                if !rect.expand(1.0).contains(at) {
                    return None;
                }
                let picked = self.picked(track, time);
                let points = [
                    vec2(0.0, -5.5),
                    vec2(5.5, 0.0),
                    vec2(0.0, 5.5),
                    vec2(-5.5, 0.0),
                ]
                .map(|delta| at + delta);
                painter.add(egui::Shape::convex_polygon(
                    points.to_vec(),
                    if picked { color::SUN } else { color::INK_SOFT },
                    Stroke::new(1.0, Color32::BLACK),
                ));
                Some(PointHit {
                    at,
                    time,
                    channel: 0,
                })
            })
            .collect()
    }

    fn graph_plot(
        &self,
        scene: &Scene,
        index: usize,
        track: &Track,
        rect: Rect,
        view: TimeView,
        keys: &[LaneKey],
        playhead: f64,
    ) -> Plot {
        if let Some(plot) = match &self.drag {
            Some(Drag::Keys(drag)) if drag.source.track == *track => Some(&drag.plot),
            Some(Drag::Handle(drag)) if drag.track == *track => Some(&drag.plot),
            _ => None,
        } {
            return plot.clone();
        }
        let rect = rect.shrink2(vec2(0.0, 13.0));
        let shape = &scene.shapes[index];
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        let mut include = |v: f32| {
            lo = lo.min(v);
            hi = hi.max(v);
        };
        for channel in 0..track.lane.channels().len() {
            if keys.is_empty() {
                include(track.lane.value(shape, playhead, channel));
            }
            for key in keys {
                include(key.value(channel));
            }
            for ab in keys.windows(2) {
                let (a, b) = (ab[0].value(channel), ab[1].value(channel));
                if let ("custom", Some(curve)) = ab[0].easing(&shape.ease) {
                    include(a + (b - a) * curve[1]);
                    include(a + (b - a) * curve[3]);
                }
            }
        }
        let margin = ((hi - lo) * 0.10).max(1.0);
        Plot {
            rect,
            view,
            values: lo - margin..=hi + margin,
        }
    }

    fn draw_graph(
        &self,
        ui: &Ui,
        track: &Track,
        shape: &Shape,
        keys: &[LaneKey],
        plot: &Plot,
        active_channel: usize,
        playhead: f64,
        handles: &mut Vec<Handle>,
    ) -> Vec<PointHit> {
        let painter = ui
            .painter()
            .with_clip_rect(plot.rect.expand2(vec2(7.0, 12.0)));
        let mut points = Vec::new();
        let channels = 0..track.lane.channels().len();
        for channel in channels
            .filter(|&c| c != active_channel)
            .chain([active_channel])
        {
            let ink = channel_color(channel);
            let start = plot.view.start;
            let end = start + plot.view.span;
            let flat = |from, to, value| {
                if to > from {
                    painter.line_segment(
                        [plot.point(from, value), plot.point(to, value)],
                        Stroke::new(1.5, ink),
                    );
                }
            };
            if let (Some(first), Some(last)) = (keys.first(), keys.last()) {
                flat(start, first.time().min(end), first.value(channel));
                flat(last.time().max(start), end, last.value(channel));
            } else {
                flat(start, end, track.lane.value(shape, playhead, channel));
            }
            for ab in keys.windows(2) {
                let (a, b) = (&ab[0], &ab[1]);
                if b.time() <= a.time()
                    || b.time() < plot.view.start
                    || a.time() > plot.view.start + plot.view.span
                {
                    continue;
                }
                let (v0, v1) = (a.value(channel), b.value(channel));
                let (kind, curve) = a.easing(&shape.ease);
                let from = plot.point(a.time(), v0);
                let to = plot.point(b.time(), v1);
                if kind == "hold" {
                    painter.add(egui::Shape::line(
                        vec![from, pos2(to.x, from.y), to],
                        Stroke::new(2.0, ink),
                    ));
                    continue;
                }
                let stroke = Stroke::new(2.0, ink);
                let samples = (0..=40)
                    .map(|i| {
                        let f = i as f32 / 40.0;
                        let t = a.time() + f64::from(f) * (b.time() - a.time());
                        let value = v0 + (v1 - v0) * ease_at(kind, f, curve);
                        let value = match track.lane {
                            Lane::Mask(_, property) => property.clamp(value),
                            Lane::Vector(property) if property != crate::shape_style::VectorProperty::Path => {
                                property.clamp(channel, value)
                            }
                            _ => value,
                        };
                        plot.point(t, value)
                    })
                    .collect();
                painter.add(egui::Shape::line(samples, stroke));
                if channel == active_channel
                    && (self.picked(track, a.time()) || self.picked(track, b.time()))
                    && (v1 - v0).abs() > f32::EPSILON
                {
                    let [x1, y1, x2, y2] = a.handles(&shape.ease);
                    for (index, (x, y, end)) in
                        [(x1, y1, from), (x2, y2, to)].into_iter().enumerate()
                    {
                        let true_point = plot.point(
                            a.time() + f64::from(x) * (b.time() - a.time()),
                            v0 + y * (v1 - v0),
                        );
                        // A preset can put a handle directly on a key. Give the two hit targets room.
                        let offset =
                            if true_point.distance(from).min(true_point.distance(to)) < 10.0 {
                                let side = if index == 0 { 1.0 } else { -1.0 };
                                vec2(
                                    11.0 * side,
                                    7.0 * (plot.rect.center().y - true_point.y).signum(),
                                )
                            } else {
                                egui::Vec2::ZERO
                            };
                        let at = true_point + offset;
                        painter.line_segment([end, at], Stroke::new(1.0, ink));
                        painter.circle(at, 4.0, color::VEIL_STRONG, Stroke::new(1.5, ink));
                        handles.push(Handle {
                            at,
                            from: (a.time(), v0),
                            to: (b.time(), v1),
                            index,
                            offset,
                        });
                    }
                }
            }
            for key in keys {
                let time = key.time();
                let at = plot.point(time, key.value(channel));
                let picked = self.picked(track, time);
                painter.circle(
                    at,
                    if picked { 5.0 } else { 4.0 },
                    if picked { color::SUN } else { ink },
                    Stroke::new(1.0, Color32::BLACK),
                );
                points.push(PointHit { at, time, channel });
            }
        }
        for y in [plot.rect.top(), plot.rect.bottom()] {
            painter.text(
                pos2(plot.rect.left() + 5.0, y),
                Align2::LEFT_CENTER,
                format!("{:.1}", plot.value(y)),
                font::ui(10.0),
                color::INK_MUTED,
            );
        }
        points
    }

    fn interact(
        &mut self,
        ui: &Ui,
        response: &Response,
        scene: &mut Scene,
        track: &Track,
        plot: Plot,
        points: &[PointHit],
        handles: &[Handle],
        graph: bool,
        channel: usize,
        playhead: &mut f64,
        playing: &mut bool,
    ) -> bool {
        let mut selected_layer = false;
        if let Some(at) = response.hover_pos() {
            if let Some(hit) = point_at(points, at, channel) {
                let tip = if graph {
                    format!(
                        "{} · {:.2} s · {:.2}",
                        track.lane.channels()[hit.channel],
                        hit.time,
                        track
                            .lane
                            .value(track_shape(scene, track).unwrap(), hit.time, hit.channel)
                    )
                } else {
                    format!("{} key · {:.2} s", track.lane.label(), hit.time)
                };
                response.clone().on_hover_text(tip);
                ui.ctx().set_cursor_icon(if graph {
                    egui::CursorIcon::Move
                } else {
                    egui::CursorIcon::ResizeHorizontal
                });
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(response.rect.center());
            if let Some(h) = handles.iter().rev().find(|h| {
                h.at.distance(origin) <= 7.0
                    && point_at(points, origin, channel)
                        .is_none_or(|p| h.at.distance(origin) + 1.0 < p.at.distance(origin))
            }) {
                self.drag = Some(Drag::Handle(HandleDrag {
                    track: track.clone(),
                    handle: Handle {
                        offset: h.offset + (origin - h.at),
                        ..h.clone()
                    },
                    plot,
                }));
            } else if let Some(hit) = point_at(points, origin, channel) {
                if !self.picked(track, hit.time) {
                    self.select(track, hit.time, extend(ui), false);
                }
                let graph_key = if graph {
                    track_shape(scene, track)
                        .and_then(|s| s.lane_key(track.lane, hit.time))
                        .map(|key| (hit.channel, key))
                } else {
                    None
                };
                self.drag = Some(Drag::Keys(KeyDrag {
                    source: Pick {
                        track: track.clone(),
                        time: hit.time,
                    },
                    origin,
                    plot,
                    graph_key,
                    picks: self.selected.clone(),
                    delta: 0.0,
                }));
                selected_layer = true;
            } else {
                let extend = if extend(ui) {
                    self.selected.clone()
                } else {
                    Vec::new()
                };
                self.lane_focus = true;
                self.drag = Some(Drag::Box(BoxDrag {
                    origin,
                    area: Rect::from_min_max(origin, origin),
                    extend,
                }));
            }
            *playing = false;
        }
        if let Some(at) = response.interact_pointer_pos() {
            if response.clicked() {
                if let Some(hit) = point_at(points, at, channel) {
                    self.select(track, hit.time, extend(ui), true);
                    if !extend(ui) {
                        *playhead = hit.time;
                    }
                    selected_layer = true;
                } else if !extend(ui) {
                    self.clear_selection();
                    self.lane_focus = true;
                    self.pick_track(track, false, false);
                }
                *playing = false;
            }
            if response.secondary_clicked() {
                self.menu = point_at(points, at, channel).map(|hit| Pick {
                    track: track.clone(),
                    time: hit.time,
                });
                if let Some(pick) = self.menu.clone() {
                    if !self.picked(track, pick.time) {
                        self.select(track, pick.time, false, false);
                    }
                    selected_layer = true;
                }
            }
        }
        if self.menu.as_ref().is_some_and(|p| p.track == *track) {
            response.context_menu(|ui| {
                for (label, kind) in EASINGS {
                    if ui.button(label).clicked() {
                        self.apply_easing(scene, kind);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Delete").clicked() {
                    self.delete(scene);
                    ui.close();
                }
            });
        }
        selected_layer
    }

    pub fn finish_frame(
        &mut self,
        ui: &Ui,
        scene: &mut Scene,
        area: Rect,
        playhead: f64,
        lock_ratio: bool,
    ) {
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        if let Drag::Keys(drag) = &drag {
            let mut source = drag.source.clone();
            source.time += drag.delta;
            if key_at(scene, &source).is_none() {
                return;
            }
        }
        let (down, released, pointer, free) = ui.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.latest_pos(),
                i.modifiers.alt,
            )
        });
        if let Some(pointer) = pointer.filter(|_| down || released) {
            match &mut drag {
                Drag::Keys(drag) => {
                    if let Some(picks) = drag.apply(scene, pointer, playhead, free, lock_ratio) {
                        self.selected = picks;
                    }
                }
                Drag::Handle(drag) => drag.apply(scene, pointer),
                Drag::Box(drag) => {
                    drag.area = Rect::from_two_pos(drag.origin, pointer);
                    let visible = drag.area.intersect(area);
                    self.selected = drag.extend.clone();
                    for (pick, pt) in &self.frame_points {
                        if visible.contains(*pt) && !self.selected.iter().any(|p| p.matches(pick)) {
                            self.selected.push(pick.clone());
                        }
                    }
                    let painter = ui.painter().with_clip_rect(area);
                    painter.rect_filled(visible, 0.0, color::SELECT_FILL);
                    painter.rect_stroke(
                        visible,
                        0.0,
                        Stroke::new(1.0, color::LEAF),
                        StrokeKind::Inside,
                    );
                }
            }
        }
        if down {
            self.drag = Some(drag);
        }
    }

    fn apply_easing(&self, scene: &mut Scene, kind: &str) {
        for pick in &self.selected {
            if let Some(shape) = track_shape_mut(scene, &pick.track) {
                if let Some(mut key) = shape.lane_key(pick.track.lane, pick.time) {
                    key.set_easing(kind, None);
                    shape.put_lane_key(pick.time, key);
                }
            }
        }
    }

    fn delete(&mut self, scene: &mut Scene) {
        for pick in self.selected.drain(..) {
            if let Some(shape) = track_shape_mut(scene, &pick.track) {
                shape.remove_lane_key(pick.track.lane, pick.time);
            }
        }
    }

    pub fn shortcut(
        &mut self,
        scene: &mut Scene,
        playhead: f64,
        copy: bool,
        paste: bool,
        delete: bool,
    ) -> bool {
        if copy && !self.selected.is_empty() {
            self.clipboard = self
                .selected
                .iter()
                .filter_map(|p| {
                    let mut key = key_at(scene, p)?;
                    key.detach_from_motion();
                    Some((p.track.clone(), key))
                })
                .collect();
            return true;
        }
        if paste
            && self.lane_focus
            && !self.clipboard.is_empty()
            && (!self.graph_tracks.is_empty() || !self.selected.is_empty())
        {
            let start = self
                .clipboard
                .iter()
                .map(|(_, k)| k.time())
                .fold(f64::INFINITY, f64::min);
            let one_source = self
                .clipboard
                .iter()
                .all(|(track, _)| *track == self.clipboard[0].0);
            let patch = self
                .clipboard
                .iter()
                .filter_map(|(source, key)| {
                    let track = if one_source && self.graph_tracks.len() == 1 {
                        let track = self.graph_tracks.iter().next().unwrap();
                        let compatible = match key {
                            LaneKey::Color(_) | LaneKey::Vector(_) => track.lane == source.lane,
                            LaneKey::Mask(_, key) => {
                                matches!(track.lane, Lane::Mask(_, property) if property == key.property)
                            }
                            LaneKey::Pose(_, key) => {
                                track.lane == source.lane && track_shape(scene, track).is_some_and(|shape| {
                                    crate::model::is_linked(shape) == key.linked_space
                                })
                            }
                        };
                        if compatible {
                            track
                        } else {
                            source
                        }
                    } else {
                        source
                    };
                    track_shape(scene, track)?;
                    let time = hundredths(playhead) + (key.time() - start);
                    Some((
                        Pick {
                            track: track.clone(),
                            time,
                        },
                        key.clone().for_lane(track.lane),
                    ))
                })
                .collect::<Vec<_>>();
            if patch.is_empty() {
                return false;
            }
            self.selected.clear();
            for (pick, key) in patch {
                track_shape_mut(scene, &pick.track)
                    .unwrap()
                    .put_lane_key(pick.time, key);
                scene.duration = scene.duration.max(pick.time);
                self.selected.push(pick);
            }
            return true;
        }
        if delete && !self.selected.is_empty() {
            self.delete(scene);
            return true;
        }
        false
    }
}

impl KeyDrag {
    fn apply(
        &mut self,
        scene: &mut Scene,
        pointer: Pos2,
        playhead: f64,
        free: bool,
        lock_ratio: bool,
    ) -> Option<Vec<Pick>> {
        self.picks.retain(|p| key_at(scene, p).is_some());
        let first = self
            .picks
            .iter()
            .map(|p| p.time - self.delta)
            .fold(f64::INFINITY, f64::min);
        let t = self.source.time;
        let view = self.plot.view;
        let mut delta =
            view.time(self.plot.rect, pointer.x) - view.time(self.plot.rect, self.origin.x);
        delta = if !free
            && (view.x(self.plot.rect, t + delta) - view.x(self.plot.rect, playhead)).abs() < 8.0
        {
            hundredths(playhead) - t
        } else {
            hundredths(t + delta) - t
        };
        delta = delta.max(-first);
        let step = delta - self.delta;

        // Stage changes against the live layers. A collision in one lane rejects the whole move.
        let mut changed = Vec::new();
        for live in scene
            .shapes
            .iter()
            .filter(|s| self.picks.iter().any(|p| p.track.layer == s.id))
        {
            let mut shape = live.clone();
            let lanes =
                Lane::ALL
                    .into_iter()
                    .chain(live.masks.iter().flat_map(|mask| {
                        MaskProperty::ALL.map(|property| Lane::Mask(mask.id, property))
                    }))
                    .chain(live.vector.iter().flat_map(|vector| vector.properties()).map(Lane::Vector));
            for lane in lanes {
                let times: Vec<_> = self
                    .picks
                    .iter()
                    .filter(|p| p.track.layer == shape.id && p.track.lane == lane)
                    .map(|p| p.time)
                    .collect();
                if !times.is_empty() && !shape.retime_lane_keys(lane, &times, step) {
                    return None;
                }
            }
            if let Some((channel, original)) = self
                .graph_key
                .as_ref()
                .filter(|_| shape.id == self.source.track.layer)
            {
                let channel = *channel;
                let lane = self.source.track.lane;
                let time = t + delta;
                let mut key = shape.lane_key(lane, time)?;
                let value = original.value(channel) + self.plot.value(pointer.y)
                    - self.plot.value(self.origin.y);
                let size_lane = lane == Lane::Pose(Property::Size);
                let mut desired = original.clone();
                if value != original.value(channel) {
                    let value = if size_lane { value.max(1.0) } else { value };
                    desired.set_value(channel, value, lock_ratio);
                }
                let size_locked = lock_ratio && size_lane;
                if key.value(channel) != desired.value(channel)
                    || (size_locked && key.value(1 - channel) != desired.value(1 - channel))
                {
                    key.set_value(channel, desired.value(channel), false);
                    if size_locked {
                        key.set_value(1 - channel, desired.value(1 - channel), false);
                    }
                    shape.put_lane_key(time, key);
                }
            }
            if shape != *live {
                changed.push(shape);
            }
        }
        for pick in &mut self.picks {
            pick.time += step;
            scene.duration = scene.duration.max(pick.time);
        }
        for shape in changed {
            if let Some(live) = scene.shapes.iter_mut().find(|s| s.id == shape.id) {
                *live = shape;
            }
        }
        self.delta = delta;
        Some(self.picks.clone())
    }
}

impl HandleDrag {
    fn apply(&self, scene: &mut Scene, pointer: Pos2) {
        let Some(shape) = track_shape_mut(scene, &self.track) else {
            return;
        };
        let pointer = pointer - self.handle.offset;
        let (t0, v0) = self.handle.from;
        let (t1, v1) = self.handle.to;
        if let Some(mut key) = shape.lane_key(self.track.lane, t0) {
            let x = (self.plot.view.time(self.plot.rect, pointer.x) - t0) / (t1 - t0);
            let y = (self.plot.value(pointer.y) - v0) / (v1 - v0);
            key.move_handle(&shape.ease, self.handle.index, x as f32, y);
            shape.put_lane_key(t0, key);
        }
    }
}

fn point_at(points: &[PointHit], pos: Pos2, channel: usize) -> Option<&PointHit> {
    points
        .iter()
        .filter(|p| p.at.distance(pos) <= 9.0)
        .min_by(|a, b| {
            a.at.distance_sq(pos)
                .total_cmp(&b.at.distance_sq(pos))
                .then_with(|| (a.channel != channel).cmp(&(b.channel != channel)))
        })
}

fn channel_color(channel: usize) -> Color32 {
    [
        Accent::Clay.color(),
        Accent::Sky.color(),
        Accent::Moss.color(),
        color::SUN,
        color::INK,
    ][channel]
}

fn extend(ui: &Ui) -> bool {
    ui.input(|i| i.modifiers.shift || i.modifiers.command)
}

fn track_shape<'a>(scene: &'a Scene, track: &Track) -> Option<&'a Shape> {
    scene
        .shapes
        .iter()
        .find(|shape| shape.id == track.layer && track.lane.exists_on(shape))
}

fn track_shape_mut<'a>(scene: &'a mut Scene, track: &Track) -> Option<&'a mut Shape> {
    scene
        .shapes
        .iter_mut()
        .find(|shape| shape.id == track.layer && track.lane.exists_on(shape))
}

fn key_at(scene: &Scene, pick: &Pick) -> Option<LaneKey> {
    track_shape(scene, &pick.track)?.lane_key(pick.track.lane, pick.time)
}

fn easing_label(kind: &str) -> &'static str {
    EASINGS
        .iter()
        .find_map(|&(label, value)| (kind == value).then_some(label))
        .unwrap_or("Custom")
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Modifiers, PointerButton, RawInput};

    #[test]
    fn key_drag_survives_layer_list_changes_and_stops_when_its_source_is_deleted() {
        let ctx = Context::default();
        let mut scene = Scene::empty("live-key-drag".into());
        for id in ["before", "source", "other", "removed"] {
            scene.add_shape(id.into(), "rect");
        }
        let lane = Lane::Pose(Property::Position);
        let pick = |layer: &str| Pick {
            track: Track {
                layer: layer.into(),
                lane,
            },
            time: 0.0,
        };
        let area = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 100.0));
        let plot = Plot {
            rect: area,
            view: TimeView {
                start: 0.0,
                span: 4.0,
            },
            values: 0.0..=100.0,
        };
        let mut editor = KeyEditor {
            drag: Some(Drag::Keys(KeyDrag {
                source: pick("source"),
                origin: pos2(0.0, 50.0),
                plot,
                graph_key: None,
                picks: vec![pick("source"), pick("other"), pick("removed")],
                delta: 0.0,
            })),
            ..Default::default()
        };
        let button = Event::PointerButton {
            pos: pos2(50.0, 50.0),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        };
        let mut frame = |scene: &mut Scene, events| {
            ctx.run_ui(
                RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    editor.begin_frame(scene);
                    editor.finish_frame(ui, scene, area, 0.0, false);
                },
            )
            .textures_delta
            .clear();
        };
        frame(&mut scene, vec![button]);
        scene.delete_layers(&["before".into(), "removed".into()], 0.0);
        scene.add_shape("added".into(), "circle");
        scene.shapes.swap(0, 2);
        scene.shapes[1].name = "Renamed during the drag".into();
        let added = scene.shapes[0].clone();
        frame(&mut scene, vec![Event::PointerMoved(pos2(100.0, 50.0))]);
        assert_eq!(scene.shapes[0], added);
        assert_eq!(scene.shapes[1].name, "Renamed during the drag");
        for id in ["source", "other"] {
            let shape = track_shape(&scene, &pick(id).track).unwrap();
            assert!(shape.lane_key(lane, 1.0).is_some());
            assert!(shape.lane_key(lane, 0.5).is_none());
        }
        scene.delete_layers(&["source".into()], 0.0);
        frame(&mut scene, vec![Event::PointerMoved(pos2(150.0, 50.0))]);
        assert!(editor.drag.is_none());
        assert!(scene.shapes[1].lane_key(lane, 1.0).is_some());
        assert!(scene.shapes[1].lane_key(lane, 1.5).is_none());
    }

    #[test]
    fn box_select_drag_copy_paste_and_delete_work_across_lanes() {
        let ctx = Context::default();
        let mut scene = Scene::example("lane-gestures".into());
        let mut editor = KeyEditor::default();
        let lanes = [
            Lane::Pose(Property::Position),
            Lane::Pose(Property::Rotation),
        ];
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 160.0));
        let area = Rect::from_min_max(pos2(0.0, 40.0), pos2(600.0, 104.0));
        let view = TimeView {
            start: 0.0,
            span: 4.0,
        };
        let mut playhead = 0.0;
        let mut playing = false;
        let mut render = |events| {
            ctx.run_ui(
                RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    editor.begin_frame(&scene);
                    for (i, &lane) in lanes.iter().enumerate() {
                        let line = Rect::from_min_size(
                            area.min + vec2(0.0, i as f32 * 32.0),
                            vec2(600.0, 32.0),
                        );
                        editor.row(
                            ui,
                            &mut scene,
                            0,
                            lane,
                            line,
                            view,
                            &mut playhead,
                            &mut playing,
                        );
                    }
                    editor.finish_frame(ui, &mut scene, area, playhead, false);
                },
            )
            .textures_delta
            .clear();
        };
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        render(vec![]);
        let start = pos2(350.0, 44.0);
        let end = pos2(400.0, 100.0);
        render(vec![Event::PointerMoved(start), button(start, true)]);
        render(vec![Event::PointerMoved(end)]);
        render(vec![button(end, false)]);

        let at = pos2(376.0, 56.0);
        let to = at + vec2(52.5, 0.0);
        render(vec![Event::PointerMoved(at), button(at, true)]);
        render(vec![Event::PointerMoved(to)]);
        render(vec![button(to, false)]);

        for lane in lanes {
            assert!(scene.shapes[0].lane_key(lane, 2.0).is_none());
            assert!(scene.shapes[0].lane_key(lane, 2.5).is_some());
        }
        assert!(editor.shortcut(&mut scene, 1.0, true, false, false));
        assert!(editor.shortcut(&mut scene, 1.0, false, true, false));
        for lane in lanes {
            assert!(scene.shapes[0].lane_key(lane, 1.0).is_some());
        }
        assert!(editor.shortcut(&mut scene, 1.0, false, false, true));
        for lane in lanes {
            assert!(scene.shapes[0].lane_key(lane, 1.0).is_none());
            assert!(scene.shapes[0].lane_key(lane, 2.5).is_some());
        }
    }

    #[test]
    fn dragging_a_graph_handle_updates_the_outgoing_curve() {
        let ctx = Context::default();
        let mut scene = Scene::example("curve-gesture".into());
        let lane = Lane::Pose(Property::Rotation);
        let track = Track {
            layer: scene.shapes[1].id.clone(),
            lane,
        };
        let mut editor = KeyEditor {
            graph: true,
            ..Default::default()
        };
        editor.select(&track, 2.0, false, false);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 200.0));
        let line = Rect::from_min_max(pos2(0.0, 30.0), pos2(600.0, 166.0));
        let rect = Rect::from_min_max(pos2(LABEL_WIDTH, 33.0), pos2(600.0 - TIME_MARGIN, 163.0));
        let view = TimeView {
            start: 0.0,
            span: 4.0,
        };
        let keys = scene.shapes[1].lane_keys(lane);
        let plot = editor.graph_plot(&scene, 1, &track, rect, view, &keys, 2.0);
        let at = plot.point(2.0 / 3.0, 15.0);
        let to = at + vec2(16.0, -18.0);
        let mut playhead = 2.0;
        let mut playing = false;
        let mut render = |events| {
            ctx.run_ui(
                RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    editor.begin_frame(&scene);
                    editor.row(
                        ui,
                        &mut scene,
                        1,
                        lane,
                        line,
                        view,
                        &mut playhead,
                        &mut playing,
                    );
                    editor.finish_frame(ui, &mut scene, line, playhead, false);
                },
            )
            .textures_delta
            .clear();
        };
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        render(vec![]);
        render(vec![Event::PointerMoved(at), button(at, true)]);
        render(vec![Event::PointerMoved(to)]);
        render(vec![button(to, false)]);

        let key = scene.shapes[1].lane_key(lane, 0.0).unwrap();
        let (ease, curve) = key.easing(&scene.shapes[1].ease);
        assert_eq!(ease, "custom");
        assert!(curve.unwrap()[1] > 0.0);
        assert_eq!(scene.shapes[1].lane_key(lane, 2.0).unwrap().value(0), 105.0);
    }
}
