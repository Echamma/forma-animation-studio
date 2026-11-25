use super::*;
use crate::{
    compositing::{LayerMask, MaskMode, MaskPose, MaskShape},
    effects::{Effect, Kind},
    shape_style::{Dash, Gradient, PaintMode, PaintStyle, StrokeStyle, Trim, VectorLayer},
    spatial::Spatial,
    template,
    text_animator::{Animator, TextProperty, Units},
};
use std::collections::HashSet;

struct LayoutHost;

impl Host for LayoutHost {
    fn toast(&mut self, _: String) {}
    fn exported(&mut self, _: std::path::PathBuf) {}
}

fn fixture() -> Forma {
    let root = std::env::temp_dir().join(format!("forma-layout-{}", new_id()));
    let mut forma = Forma::new(Place {
        space: None,
        projects_dir: root.clone(),
        data_dir: root.clone(),
        exports_dir: root,
    });
    forma.scene = Scene::example(new_id());
    forma.scene.width = 480.0;
    forma.scene.height = 300.0;
    forma.scene.add_layer("flat".into(), "rect");
    forma.scene.add_layer("depth".into(), "rect");
    let depth_index = forma.scene.shapes.len() - 1;
    forma.scene.shapes[depth_index].vector = Some(VectorLayer {
        fill: Some(PaintStyle {
            mode: PaintMode::Linear,
            gradient: Some(Gradient::default()),
        }),
        stroke: Some(StrokeStyle {
            dash: Some(Dash::default()),
            ..Default::default()
        }),
        trim: Some(Trim::default()),
        ..Default::default()
    });
    let flat_index = depth_index - 1;
    forma.scene.shapes[flat_index].name = "A layered background with a very long name".into();
    forma
        .scene
        .set_parent(depth_index, Some(flat_index), 0.0)
        .unwrap();
    forma.scene.add_text("words".into());
    forma
        .scene
        .shapes
        .iter_mut()
        .find(|s| s.id == "depth")
        .unwrap()
        .spatial = Some(Spatial {
        enabled: true,
        ..Default::default()
    });
    let text = forma.scene.shapes.last_mut().unwrap();
    text.extra
        .insert("text".into(), "Hello Forma\nsecond line".into());
    let mut animator = Animator {
        name: "Range selector".into(),
        units: Units::VisibleCharacters,
        seed: Some(3),
        ..Default::default()
    };
    for p in TextProperty::EFFECTS {
        animator.enable(p, p.initial());
    }
    let animator = text.text_layout.as_mut().unwrap().add(animator).unwrap();
    text.effects = vec![
        Effect::new(1, Kind::DropShadow),
        Effect::new(2, Kind::GradientRamp),
    ];
    text.masks.push(LayerMask {
        id: 1,
        shape: MaskShape::Rectangle,
        mode: MaskMode::Add,
        inverted: false,
        reference_size: [100.0, 100.0],
        rest: MaskPose::new([0.0, 0.0], [100.0, 100.0]),
        keys: Vec::new(),
    });
    text.template_fields.push(template::Field {
        id: "tp-text".into(),
        label: "Title".into(),
        target: template::Target::Text,
    });
    let lane = Lane::Pose(Property::Position);
    text.set_expression(lane, Some("value +".into()));
    forma
        .key_editor
        .open_graph_expression_for_test("words", lane);
    forma.key_editor.toggle_animator("words".into(), animator);
    forma.key_editor.toggle_effects("words".into());
    forma.key_editor.toggle_mask("words", 1);
    let text_index = forma.scene.shapes.len() - 1;
    forma.add_fade(text_index);
    forma.add_spin(text_index);
    let camera = forma.add_camera().unwrap();
    let camera = forma
        .scene
        .shapes
        .iter_mut()
        .find(|s| s.id == camera)
        .unwrap();
    camera.id = "camera".into();
    camera.spatial.as_mut().unwrap().depth_of_field = true;
    forma.scene.compositions.push(Scene::empty("child".into()));
    forma
        .scene
        .add_composition_layer("nested".into(), "child")
        .unwrap();
    forma.show_projects = false;
    forma.drawer_decided = true;
    forma.edited_at = None;
    forma
}

fn run_frame(ctx: &egui::Context, size: Vec2, app: impl FnMut(&mut Ui)) -> egui::FullOutput {
    input_frame(ctx, size, Vec::new(), app)
}

fn input_frame(
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
    app: impl FnMut(&mut Ui),
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        events,
        ..Default::default()
    };
    run_input(ctx, input, app)
}

fn run_input(
    ctx: &egui::Context,
    input: egui::RawInput,
    app: impl FnMut(&mut Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, app);
    output.textures_delta.clear();
    output
}

fn setup() -> egui::Context {
    let ctx = egui::Context::default();
    canopy_ui::install(&ctx);
    ctx.style_mut_of(egui::Theme::Dark, |style| style.animation_time = 0.0);
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique("forma-test-open-sections"), true));
    ctx
}

fn text_shapes(shape: &egui::Shape, f: &mut impl FnMut(&egui::epaint::TextShape)) {
    match shape {
        egui::Shape::Text(text) => f(text),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| text_shapes(shape, f)),
        _ => {}
    }
}

fn overlaps(a: Rect, b: Rect) -> bool {
    let shared = a.intersect(b);
    shared.width() > 1.0 && shared.height() > 1.0
}

fn check_text(output: &egui::FullOutput, pane: Rect, case: &str) -> HashSet<String> {
    let mut visible: Vec<(Rect, String)> = Vec::new();
    let mut labels = HashSet::new();
    for clipped in &output.shapes {
        text_shapes(&clipped.shape, &mut |text| {
            let galley = &text.galley;
            let label = galley.text();
            let rect = galley.rect.translate(text.pos.to_vec2());
            if label.trim().is_empty() || rect.intersect(clipped.clip_rect).height() <= 1.0 {
                return;
            }
            if clipped.clip_rect.expand(1.0).contains_rect(rect) {
                if label == "Characters excluding spaces" {
                    assert!(!galley.elided, "{case}: the Based on choice was truncated");
                }
                labels.insert(label.to_owned());
            }
            // A scrolling row can cross the top or bottom of its viewport during a scroll.
            // Horizontal clipping, and painting beyond the outer pane, are layout defects.
            let clip = clipped.clip_rect.intersect(pane).expand(1.0);
            assert!(
                rect.left() >= clip.left()
                    && rect.right() <= clip.right()
                    && pane
                        .expand(1.0)
                        .contains_rect(rect.intersect(clipped.clip_rect)),
                "{case}: clipped text {label:?}, {rect:?} in {clip:?}"
            );
            for rows in galley.rows.windows(2) {
                let last = rows[0].row.glyphs.last().map(|g| g.chr);
                let first = rows[1].row.glyphs.first().map(|g| g.chr);
                assert!(
                    rows[0].ends_with_newline
                        || !last.is_some_and(char::is_alphanumeric)
                        || !first.is_some_and(char::is_alphanumeric),
                    "{case}: a word breaks across lines in {label:?}"
                );
            }
            for row in &galley.rows {
                let row_rect = row
                    .rect_without_leading_space()
                    .translate(text.pos.to_vec2())
                    .intersect(clipped.clip_rect);
                if !row_rect.is_positive() {
                    continue;
                }
                for (prior, name) in &visible {
                    assert!(
                        !overlaps(*prior, row_rect),
                        "{case}: {label:?} {row_rect:?} overlaps {name:?} {prior:?}"
                    );
                }
                visible.push((row_rect, label.to_owned()));
            }
        });
    }
    labels
}

fn check_widgets(ctx: &egui::Context, case: &str) {
    let widgets = ctx.interactive_rects_last_pass();
    for (i, &a) in widgets.iter().enumerate() {
        for &b in &widgets[..i] {
            // egui reports the containing drag target for a scroll area too, and Forma's
            // canvas reports a drag target under the canvas bar, gizmo and quick actions.
            // Timeline clip edge hit targets are also intentionally inside their clip.
            let contains = |outer: Rect, inner: Rect| {
                outer.contains_rect(inner)
                    && (outer.size() == inner.size() || outer.width() >= 150.0)
            };
            if contains(a, b) || contains(b, a) {
                continue;
            }
            assert!(
                !overlaps(a, b),
                "{case}: interactive rectangles overlap: {a:?}, {b:?}"
            );
        }
    }
}

fn pane_at(size: Vec2, minimum_inspector: bool) -> Rect {
    let left = if minimum_inspector {
        size.x - 1130.0
    } else {
        267.0
    };
    Rect::from_min_max(pos2(left, 96.0), pos2(size.x - 12.0, size.y - 12.0))
}

fn painted_label(
    output: &egui::FullOutput,
    bounds: Rect,
    label: &str,
    matches: impl Fn(Rect) -> bool,
) -> Option<Rect> {
    output.shapes.iter().rev().find_map(|clipped| {
        let mut found = None;
        text_shapes(&clipped.shape, &mut |text| {
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            if text.galley.text() == label
                && !text.galley.elided
                && bounds.contains_rect(rect)
                && clipped.clip_rect.contains_rect(rect)
                && matches(rect)
            {
                found = Some(rect);
            }
        });
        found
    })
}

fn pointer_button(pos: Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn key_event(key: egui::Key, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers,
    }
}

struct PopupTest {
    forma: Forma,
    ctx: egui::Context,
    size: Vec2,
    pane: Rect,
    clock: Duration,
    modifiers: egui::Modifiers,
    saved: Option<std::path::PathBuf>,
}

struct ExportHost<'a>(&'a mut Option<std::path::PathBuf>);

impl Host for ExportHost<'_> {
    fn toast(&mut self, _: String) {}
    fn exported(&mut self, path: std::path::PathBuf) {
        *self.0 = Some(path);
    }
}

impl PopupTest {
    fn new(size: Vec2, tab: Tab, layer: &str) -> Self {
        let mut forma = fixture();
        forma.tab = tab;
        forma.selected = Some(layer.into());
        forma.last_picked = (forma.selected_motion.clone(), forma.selected_spin.clone());
        Self {
            forma,
            ctx: setup(),
            size,
            pane: pane_at(size, false),
            clock: Duration::ZERO,
            modifiers: egui::Modifiers::NONE,
            saved: None,
        }
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.advance(Duration::from_millis(16), events)
    }

    fn advance(&mut self, delay: Duration, events: Vec<egui::Event>) -> egui::FullOutput {
        self.clock += delay;
        let input = egui::RawInput {
            screen_rect: Some(self.screen()),
            time: Some(self.clock.as_secs_f64()),
            events,
            ..Default::default()
        };
        run_input(&self.ctx, input, |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(self.pane), |ui| {
                ui.set_clip_rect(self.pane);
                CanopyApp::ui(&mut self.forma, ui, &mut ExportHost(&mut self.saved));
            });
        })
    }

    fn settle_repaints(&mut self, mut output: egui::FullOutput) -> egui::FullOutput {
        let deadline = self.clock + Duration::from_millis(500);
        loop {
            let delay = repaint_delay(&output);
            if delay > deadline.saturating_sub(self.clock) {
                return output;
            }
            output = self.advance(delay, Vec::new());
        }
    }

    fn scroll_to(&mut self, label: &str, inspector: Rect) -> Pos2 {
        self.scroll_to_label(None, label, inspector)
    }

    fn scroll_to_label(&mut self, field: Option<&str>, label: &str, bounds: Rect) -> Pos2 {
        self.frame(Vec::new());
        let offset_id = egui::Id::unique("forma-test-scroll");
        let extent_id = egui::Id::unique("forma-test-scroll-extent");
        let mut offset: f32 = 0.0;
        loop {
            self.ctx.data_mut(|d| d.insert_temp(offset_id, offset));
            let output = self.frame(Vec::new());
            let title = field.and_then(|field| painted_label(&output, bounds, field, |_| true));
            let found = painted_label(&output, bounds, label, |rect| match (field, title) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(_), Some(title)) => (0.0..=42.0).contains(&(rect.top() - title.top())),
            });
            if let Some(rect) = found {
                self.ctx.data_mut(|d| d.remove::<f32>(offset_id));
                return rect.center();
            }
            let extent = self
                .ctx
                .data(|d| d.get_temp::<f32>(extent_id))
                .unwrap_or_default();
            assert!(
                offset < extent,
                "could not scroll to {label:?} at {:?}",
                self.size
            );
            offset = (offset + 120.0).min(extent);
        }
    }

    fn screen(&self) -> Rect {
        Rect::from_min_size(Pos2::ZERO, self.size)
    }

    fn inspector(&self) -> Rect {
        let width = if self.pane.width() < 1150.0 {
            250.0
        } else {
            310.0
        };
        Rect::from_min_max(
            pos2(self.pane.right() - width, self.pane.top()),
            self.pane.max,
        )
    }

    fn label(&mut self, text: &str) -> Pos2 {
        self.label_in(text, self.screen())
    }

    fn label_in(&mut self, text: &str, bounds: Rect) -> Pos2 {
        let output = self.frame(Vec::new());
        painted_label(&output, bounds, text, |_| true)
            .unwrap_or_else(|| panic!("missing control {text:?} at {:?}", self.size))
            .center()
    }

    fn click(&mut self, pos: Pos2, button: egui::PointerButton) -> egui::FullOutput {
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        self.frame(vec![pointer_button(pos, button, true)]);
        self.frame(vec![pointer_button(pos, button, false)])
    }

    fn short_click(&mut self, pos: Pos2) -> egui::FullOutput {
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        // Both pointer events can arrive before the native event loop draws its next frame.
        self.frame(vec![
            pointer_button(pos, egui::PointerButton::Primary, true),
            pointer_button(pos, egui::PointerButton::Primary, false),
        ])
    }

    fn press_key(&mut self, key: egui::Key) -> egui::FullOutput {
        self.frame(vec![key_event(key, true, self.modifiers)]);
        self.frame(vec![key_event(key, false, self.modifiers)])
    }

    fn modified_click(&mut self, pos: Pos2, key: egui::Key, modifiers: egui::Modifiers) {
        self.modifiers = modifiers;
        self.frame(vec![
            key_event(key, true, modifiers),
            egui::Event::ModifiersChanged(modifiers),
        ]);
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            self.frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
            }]);
        }
        self.modifiers = egui::Modifiers::NONE;
        self.frame(vec![
            key_event(key, false, self.modifiers),
            egui::Event::ModifiersChanged(self.modifiers),
        ]);
    }

    fn workspace_rect(&self, name: &'static str) -> Rect {
        self.ctx
            .data(|data| data.get_temp(egui::Id::unique(("workspace bounds", name))))
            .unwrap_or_else(|| panic!("missing workspace rect {name}"))
    }

    fn popup_option(&self, output: &egui::FullOutput, option: &str) -> Pos2 {
        let pos = painted_label(output, self.screen(), option, |rect| {
            self.ctx
                .layer_id_at(rect.center())
                .is_some_and(|layer| layer.order == egui::Order::Foreground)
        })
        .unwrap_or_else(|| {
            panic!(
                "popup option {option:?} is missing or clipped at {:?}",
                self.size
            )
        })
        .center();
        let layer = self.ctx.layer_id_at(pos).unwrap();
        let popup = self.ctx.read_response(layer.id).unwrap().rect;
        assert!(
            self.screen().contains_rect(popup),
            "{option:?} popup extends outside the window: {popup:?}"
        );
        pos
    }

    fn root_popup_option(&self, output: &egui::FullOutput, option: &str) -> Pos2 {
        let pos = self.popup_option(output, option);
        let layer = self.ctx.layer_id_at(pos).unwrap();
        assert!(
            egui::Popup::is_id_open(&self.ctx, layer.id),
            "{option:?} is covered by {layer:?}"
        );
        pos
    }

    fn choose(
        &mut self,
        output: &egui::FullOutput,
        options: &[&str],
        choice: &str,
    ) -> egui::FullOutput {
        assert!(
            egui::Popup::is_any_open(&self.ctx),
            "no popup is open for {choice:?}"
        );
        for option in options {
            self.popup_option(output, option);
        }
        let pos = self.popup_option(output, choice);
        self.click(pos, egui::PointerButton::Primary)
    }

    fn inspector_menu(&mut self, field: Option<&str>, trigger: &str) -> egui::FullOutput {
        let button = self.scroll_to_label(field, trigger, self.inspector());
        self.click(button, egui::PointerButton::Primary)
    }

    fn layer(&self, id: &str) -> &Shape {
        self.forma
            .scene
            .shapes
            .iter()
            .find(|shape| shape.id == id)
            .unwrap()
    }

    fn layer_mut(&mut self, id: &str) -> &mut Shape {
        self.forma
            .scene
            .shapes
            .iter_mut()
            .find(|shape| shape.id == id)
            .unwrap()
    }
}

fn repaint_delay(output: &egui::FullOutput) -> Duration {
    output.viewport_output[&egui::ViewportId::ROOT]
        .repaint_delay
        .max(Duration::from_millis(16))
}

const WINDOW_SIZES: [Vec2; 2] = [vec2(1456.0, 819.0), vec2(1920.0, 1080.0)];

fn narrow_panes() -> [(Vec2, Rect); 2] {
    let single = vec2(1036.0, 790.0);
    let split = WINDOW_SIZES[0];
    let mut half = pane_at(split, false);
    half.min.x = half.center().x + 3.0;
    [(single, pane_at(single, false)), (split, half)]
}

#[test]
fn drawers_dock_without_covering_canvas_or_transport_in_narrow_panes() {
    for (size, pane) in narrow_panes() {
        let mut test = PopupTest::new(size, Tab::Layer, "depth");
        test.pane = pane;
        test.forma.quality = crate::preview_cache::Quality::Quarter;
        let rail = Rect::from_min_max(pane.min, pos2(pane.left() + RAIL_WIDTH, pane.bottom()));
        for (name, drawer) in [("Add", Drawer::Add), ("Layers", Drawer::Layers)] {
            let button = test.label_in(name, rail);
            let closed = test.workspace_rect("canvas");
            assert!(!test.workspace_rect("drawer").is_positive());
            let opened = test.click(button, egui::PointerButton::Primary);
            assert_eq!(test.forma.drawer, Some(drawer));
            let bounds = test.workspace_rect("drawer");
            assert!(bounds.is_positive() && pane.contains_rect(bounds));
            for name in ["canvas", "canvas-bar", "transport", "timeline"] {
                let rect = test.workspace_rect(name);
                assert!(
                    pane.expand(1.0).contains_rect(rect),
                    "{size:?} {name} goes outside {pane:?}: {rect:?}"
                );
                assert!(
                    !overlaps(bounds, rect),
                    "{size:?} {drawer:?} overlaps {name}: {bounds:?}, {rect:?}"
                );
            }
            let canvas = test.workspace_rect("canvas");
            // The inspector collapses when the columns cannot coexist, freeing space on the right.
            assert!(canvas.left() > closed.left() && canvas.width() >= MIN_CANVAS_WIDTH);
            let transport = test.workspace_rect("transport");
            assert!(
                transport.height() <= 33.0,
                "{size:?} {drawer:?} transport wraps: {transport:?}"
            );
            for label in ["Loop", "Quarter", "Graph", "Fit"] {
                assert!(
                    painted_label(&opened, transport, label, |_| true).is_some(),
                    "{size:?} {drawer:?} missing {label} in {transport:?}"
                );
            }
            assert!(
                ["Onion", "Onion skin"]
                    .iter()
                    .any(|label| { painted_label(&opened, transport, label, |_| true).is_some() })
            );
            let timecode = format!(
                "{:.2} / {:.2} s",
                test.forma.time, test.forma.scene.duration
            );
            assert!(
                painted_label(
                    &opened,
                    test.workspace_rect("timeline"),
                    &timecode,
                    |rect| { rect.top() >= transport.bottom() }
                )
                .is_some()
            );
            assert!(
                painted_label(&opened, test.workspace_rect("canvas-bar"), "100%", |_| true)
                    .is_some()
            );
            test.click(button, egui::PointerButton::Primary);
            assert!(!test.workspace_rect("drawer").is_positive());
            assert_eq!(test.workspace_rect("canvas"), closed);
        }
        let add = test.label_in("Add", rail);
        test.click(add, egui::PointerButton::Primary);
        let inspector = test.label_in("Inspector", rail);
        let opened = test.click(inspector, egui::PointerButton::Primary);
        assert!(!test.workspace_rect("drawer").is_positive());
        assert!(
            painted_label(&opened, pane, "Position & size", |_| true).is_some()
                || painted_label(&opened, pane, "Animate", |_| true).is_some()
        );
    }
}

#[test]
fn layers_drawer_rows_stay_in_place_when_selection_changes() {
    use egui::{Key, Modifiers, PointerButton};

    for (size, pane) in narrow_panes() {
        let mut test = PopupTest::new(size, Tab::Layer, "first");
        test.pane = pane;
        test.forma.drawer = Some(Drawer::Layers);
        test.forma.scene.shapes.clear();
        let names = ["First", "Second", "Third"];
        for name in names {
            test.forma.scene.add_layer(name.to_lowercase(), "rect");
            test.layer_mut(&name.to_lowercase()).name = name.into();
        }
        let output = test.frame(Vec::new());
        let drawer = test.workspace_rect("drawer");
        let rows = |output: &egui::FullOutput| {
            names.map(|name| painted_label(output, drawer, name, |_| true).unwrap())
        };
        let before = rows(&output);
        let ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        test.modified_click(before[1].center(), Key::ControlLeft, ctrl);
        assert_eq!(test.forma.multi_selected, ["first", "second"]);
        assert_eq!(
            rows(&test.frame(Vec::new())),
            before,
            "{size:?} with two selected"
        );
        let output = test.click(before[2].center(), PointerButton::Primary);
        assert_eq!(test.forma.selected.as_deref(), Some("third"));
        assert_eq!(
            rows(&output),
            before,
            "{size:?} after clicking the third layer"
        );
        test.modified_click(before[2].center(), Key::ControlLeft, ctrl);
        assert_eq!(test.forma.selected, None);
        assert_eq!(
            rows(&test.frame(Vec::new())),
            before,
            "{size:?} with nothing selected"
        );
    }
}

#[test]
fn layer_selection_and_precompose_work_from_drawer_and_timeline() {
    use egui::{Key, Modifiers, PointerButton};

    for surface in ["drawer", "timeline"] {
        let mut test = PopupTest::new(WINDOW_SIZES[0], Tab::Layer, "first");
        test.forma.scene.shapes.clear();
        for name in ["First", "Middle", "Last"] {
            let id = name.to_lowercase();
            test.forma.scene.add_layer(id.clone(), "rect");
            test.layer_mut(&id).name = name.into();
        }
        test.forma.key_editor = KeyEditor::default();
        test.forma.drawer = (surface == "drawer").then_some(Drawer::Layers);
        test.frame(Vec::new());
        let bounds = test.workspace_rect(surface);
        let first = test.label_in("First", bounds);
        test.click(first, PointerButton::Primary);
        let last = test.label_in("Last", bounds);
        test.modified_click(last, Key::ShiftLeft, Modifiers::SHIFT);
        assert_eq!(
            test.forma.multi_selected,
            ["first", "middle", "last"],
            "{surface}"
        );

        let middle = test.label_in("Middle", bounds);
        let ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        test.modified_click(middle, Key::ControlLeft, ctrl);
        assert_eq!(test.forma.multi_selected, ["first", "last"], "{surface}");
        test.modified_click(middle, Key::ControlLeft, ctrl);
        assert_eq!(
            test.forma.multi_selected,
            ["first", "last", "middle"],
            "{surface}"
        );
        let first = test.label_in("First", bounds);
        test.modified_click(first, Key::ControlLeft, ctrl);
        assert_eq!(test.forma.multi_selected, ["last", "middle"], "{surface}");
        if surface == "timeline" {
            let layers = test.label("Layers");
            test.click(layers, PointerButton::Primary);
        }
        let precompose = test.label_in("Precompose", test.workspace_rect("drawer"));
        let before = test.forma.scene.compositions.len();
        test.click(precompose, PointerButton::Primary);
        assert_eq!(test.forma.scene.compositions.len(), before + 1);
        let child = test
            .forma
            .scene
            .compositions
            .iter()
            .find(|scene| !scene.shapes.is_empty())
            .unwrap();
        let ids: Vec<_> = child.shapes.iter().map(|shape| shape.id.as_str()).collect();
        assert_eq!(ids, ["middle", "last"]);
        assert!(
            test.forma
                .scene
                .shapes
                .iter()
                .any(|shape| shape.id == "first")
        );
        test.forma.undo();
        assert_eq!(test.forma.scene.shapes.len(), 3);
    }
}

#[test]
fn effect_search_uses_keyboard_from_the_opening_frame() {
    use egui::{Key, Modifiers};

    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Layer, "words");
        let opened = test.inspector_menu(None, "Add effect\u{2026}");
        test.popup_option(&opened, "Search effects");
        assert!(test.ctx.text_edit_focused());
        let count = test.layer("words").effects.len();
        test.frame(vec![
            egui::Event::Text("wave".into()),
            key_event(Key::Enter, true, Modifiers::NONE),
        ]);
        test.frame(vec![key_event(Key::Enter, false, Modifiers::NONE)]);
        assert_eq!(test.layer("words").effects.len(), count + 1);
        assert_eq!(
            test.layer("words").effects.last().unwrap().kind,
            Kind::WaveWarp
        );
        assert!(!egui::Popup::is_any_open(&test.ctx));

        let trigger = test.scroll_to("Add effect\u{2026}", test.inspector());
        let reopened = test.short_click(trigger);
        let reopened = test.settle_repaints(reopened);
        test.root_popup_option(&reopened, "Search effects");
        test.frame(vec![egui::Event::Text("blur".into())]);
        test.press_key(Key::ArrowDown);
        test.press_key(Key::ArrowDown);
        test.press_key(Key::ArrowUp);
        assert!(test.ctx.text_edit_focused());
        test.press_key(Key::Enter);
        assert_eq!(
            test.layer("words").effects.last().unwrap().kind,
            Kind::DirectionalBlur
        );

        test.forma.camera_tool = true;
        test.inspector_menu(None, "Add effect\u{2026}");
        test.frame(vec![egui::Event::Text("wave".into())]);
        test.press_key(Key::Escape);
        assert!(!egui::Popup::is_any_open(&test.ctx));
        assert_eq!(test.layer("words").effects.len(), count + 2);
        assert_eq!(test.forma.selected.as_deref(), Some("words"));
        assert!(test.forma.camera_tool);
    }
}

#[test]
fn repeat_edges_checkbox_uses_layer_defaults_and_undo() {
    for kind in ["rect", "adjustment"] {
        let mut test = PopupTest::new(WINDOW_SIZES[0], Tab::Layer, "flat");
        let shape = test.layer_mut("flat");
        shape.kind = kind.into();
        shape.effects = vec![Effect::new(1, Kind::GaussianBlur)];
        let checkbox = test.scroll_to("Repeat edge pixels", test.inspector());
        test.click(checkbox, egui::PointerButton::Primary);
        let override_value = Some(kind != "adjustment");
        assert_eq!(
            test.layer("flat").effects[0].repeat_edge_pixels,
            override_value
        );
        test.forma.undo();
        assert_eq!(test.layer("flat").effects[0].repeat_edge_pixels, None);
        test.forma.redo();
        assert_eq!(
            test.layer("flat").effects[0].repeat_edge_pixels,
            override_value
        );
    }
}

#[test]
fn layer_popups_paint_and_accept_input_after_scrolling() {
    use crate::compositing::{Blend, MatteMode};
    use crate::shape_style::{Cap, Join};

    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Layer, "depth");
        test.layer_mut("flat").name = "Backdrop".into();
        test.layer_mut("depth").masks = test.layer("words").masks.clone();
        let mut second_mask = test.layer("depth").masks[0].clone();
        second_mask.id = 2;
        test.layer_mut("depth").masks.push(second_mask);
        test.forma.mask_editor.select("depth", 1);
        let opened = test.inspector_menu(Some("Blend mode"), "Normal");
        test.choose(&opened, &["Normal", "Multiply", "Screen"], "Multiply");
        assert_eq!(test.layer("depth").blend(), Blend::Multiply);
        test.forma.undo();
        assert_eq!(test.layer("depth").blend(), Blend::Normal);
        test.forma.redo();
        assert_eq!(test.layer("depth").blend(), Blend::Multiply);

        let opened = test.inspector_menu(Some("Blend mode"), "Multiply");
        let at = test.popup_option(&opened, "Screen");
        let scrolled = test.frame(vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -1200.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        let scrolled = test.settle_repaints(scrolled);
        test.choose(&scrolled, &["Color", "Luminosity"], "Luminosity");
        assert_eq!(test.layer("depth").blend(), Blend::Luminosity);

        let opened = test.inspector_menu(Some("Follows"), "Backdrop");
        test.choose(&opened, &["None", "Backdrop"], "None");
        assert!(test.layer("depth").parent.is_none());

        let opened = test.inspector_menu(Some("Matte"), "None");
        test.choose(&opened, &["None", "Backdrop"], "Backdrop");
        assert_eq!(
            test.layer("depth").track_matte.as_ref().unwrap().source,
            "flat"
        );
        let opened = test.inspector_menu(Some("Matte mode"), "Alpha");
        test.choose(
            &opened,
            &["Alpha", "Alpha inverted", "Luma", "Luma inverted"],
            "Luma",
        );
        assert_eq!(
            test.layer("depth").track_matte.as_ref().unwrap().mode,
            MatteMode::Luma
        );

        let opened = test.inspector_menu(Some("Mask tool"), "Off");
        test.choose(
            &opened,
            &[
                "Off",
                "Rectangle",
                "Ellipse",
                "Freeform path",
                "Edit masks",
                "Edit mask points",
            ],
            "Ellipse",
        );
        assert!(test.forma.mask_editor.active());
        let opened = test.inspector_menu(None, "Mask 1");
        test.choose(&opened, &["Mask 1", "Mask 2"], "Mask 2");
        let opened = test.inspector_menu(Some("Mask mode"), "Add");
        test.choose(
            &opened,
            &["Add", "Subtract", "Intersect", "Difference"],
            "Subtract",
        );
        assert_eq!(test.layer("depth").masks[0].mode, MaskMode::Add);
        assert_eq!(test.layer("depth").masks[1].mode, MaskMode::Subtract);

        let paint_options = ["None", "Solid", "Linear gradient", "Radial gradient"];
        let opened = test.inspector_menu(Some("Fill"), "Linear gradient");
        test.choose(&opened, &paint_options, "Radial gradient");
        assert_eq!(
            test.layer("depth")
                .vector
                .as_ref()
                .unwrap()
                .fill
                .as_ref()
                .unwrap()
                .mode,
            PaintMode::Radial
        );
        let opened = test.inspector_menu(Some("Stroke paint"), "Solid");
        test.choose(&opened, &paint_options, "Linear gradient");
        let opened = test.inspector_menu(Some("Cap"), "Butt");
        test.choose(&opened, &["Butt", "Round", "Square"], "Square");
        let opened = test.inspector_menu(Some("Join"), "Miter");
        test.choose(&opened, &["Miter", "Round", "Bevel"], "Bevel");
        let stroke = test
            .layer("depth")
            .vector
            .as_ref()
            .unwrap()
            .stroke
            .as_ref()
            .unwrap();
        assert_eq!(stroke.paint.mode, PaintMode::Linear);
        assert_eq!(stroke.cap, Cap::Square);
        assert_eq!(stroke.join, Join::Bevel);
    }
}

#[test]
fn text_and_effect_popups_paint_and_accept_input_after_scrolling() {
    use crate::text_animator::Alignment;

    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Layer, "words");
        test.layer_mut("flat").name = "Backdrop".into();
        test.forma.scene.add_layer("path".into(), "rect");
        assert!(test.layer_mut("path").convert_to_path([80.0, 80.0]));
        test.layer_mut("path").name = "Text guide".into();

        let opened = test.inspector_menu(Some("Text linked to"), "Nothing");
        test.choose(&opened, &["Backdrop", "Text guide"], "Backdrop");
        assert_eq!(test.layer("words").extra["linkId"], "flat");
        let opened = test.inspector_menu(Some("Path layer"), "None");
        test.choose(&opened, &["None", "Text guide"], "Text guide");
        assert_eq!(
            test.layer("words")
                .text_layout
                .as_ref()
                .unwrap()
                .path
                .as_deref(),
            Some("path")
        );

        let opened = test.inspector_menu(Some("Alignment"), "Center");
        test.choose(&opened, &["Left", "Center", "Right"], "Right");
        assert_eq!(
            test.layer("words").text_layout.as_ref().unwrap().alignment,
            Alignment::Right
        );

        let family = crate::text_layout::font_families()
            .iter()
            .find(|name| *name != crate::text_layout::BUNDLED_FAMILY)
            .expect("at least one system font");
        let opened = test.inspector_menu(Some("Font"), crate::text_layout::BUNDLED_FAMILY);
        let search = test.popup_option(&opened, "Find a font");
        test.click(search, egui::PointerButton::Primary);
        assert!(test.ctx.text_edit_focused());
        let filtered = test.frame(vec![egui::Event::Text(family.clone())]);
        let filtered = test.settle_repaints(filtered);
        test.choose(&filtered, &[family], family);
        assert_eq!(
            test.layer("words")
                .text_layout
                .as_ref()
                .unwrap()
                .font
                .as_ref(),
            Some(family)
        );
        assert!(!egui::Popup::is_any_open(&test.ctx));
        let trigger = test.scroll_to_label(Some("Font"), family, test.inspector());
        let reopened = test.short_click(trigger);
        let reopened = test.settle_repaints(reopened);
        let bundled = crate::text_layout::BUNDLED_FAMILY;
        test.root_popup_option(&reopened, bundled);
        test.choose(&reopened, &[bundled, family], bundled);
        assert!(
            test.layer("words")
                .text_layout
                .as_ref()
                .unwrap()
                .font
                .is_none()
        );

        let opened = test.inspector_menu(None, "Add effect\u{2026}");
        let search = test.popup_option(&opened, "Search effects");
        test.click(search, egui::PointerButton::Primary);
        assert!(test.ctx.text_edit_focused());
        let filtered = test.frame(vec![egui::Event::Text("wave warp".into())]);
        let filtered = test.settle_repaints(filtered);
        test.choose(&filtered, &["Distort", "Wave warp"], "Wave warp");
        assert!(
            test.layer("words")
                .effects
                .iter()
                .any(|effect| effect.kind == Kind::WaveWarp)
        );

        let count = test.layer("words").effects.len();
        let opened = test.inspector_menu(None, "Options");
        test.choose(
            &opened,
            &["Move up", "Move down", "Duplicate", "Delete"],
            "Duplicate",
        );
        assert_eq!(test.layer("words").effects.len(), count + 1);
    }
}

#[test]
fn color_picker_paints_and_accepts_input() {
    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Layer, "flat");
        let label = test.scroll_to("Fill color", test.inspector());
        let well = test
            .ctx
            .interactive_rects_last_pass()
            .into_iter()
            .filter(|rect| rect.left() > label.x && rect.contains(pos2(rect.center().x, label.y)))
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .unwrap();
        let opened = test.click(well.center(), egui::PointerButton::Primary);
        let before = test.layer("flat").color_at(0.0);
        let color_space = test
            .ctx
            .global_style()
            .visuals
            .numeric_color_space
            .to_string();
        test.popup_option(&opened, &color_space);
        let palette = test
            .ctx
            .interactive_rects_last_pass()
            .into_iter()
            .find(|rect| {
                rect.width() >= 270.0
                    && rect.height() >= 270.0
                    && test
                        .ctx
                        .layer_id_at(rect.center())
                        .is_some_and(|layer| layer.order == egui::Order::Foreground)
            })
            .unwrap();
        test.click(palette.center(), egui::PointerButton::Primary);
        assert_ne!(test.layer("flat").color_at(0.0), before);
    }
}

#[test]
fn animate_popups_paint_and_accept_input_after_scrolling() {
    use crate::text_animator::RangeShape;

    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Animate, "words");
        test.forma.live_animation_preview = false;
        let spin = test.forma.selected_spin.clone();
        let flat = test
            .forma
            .scene
            .shapes
            .iter()
            .position(|shape| shape.id == "flat")
            .unwrap();
        test.forma.add_spin(flat);
        test.forma.selected_spin = spin;
        test.forma.edited_at = None;
        let opened = test.inspector_menu(None, "Text presets");
        test.choose(
            &opened,
            &["Typewriter", "Fade up by word", "Scramble-free bounce"],
            "Typewriter",
        );
        assert!(
            test.layer("words")
                .text_layout
                .as_ref()
                .unwrap()
                .animators
                .iter()
                .any(|a| a.name == "Typewriter")
        );

        let opened = test.inspector_menu(Some("Based on"), "Characters excluding spaces");
        test.choose(
            &opened,
            &[
                "Characters",
                "Characters excluding spaces",
                "Words",
                "Lines",
            ],
            "Words",
        );
        assert_eq!(
            test.layer("words").text_layout.as_ref().unwrap().animators[0].units,
            Units::Words
        );
        let opened = test.inspector_menu(Some("Range shape"), "Square");
        test.choose(
            &opened,
            &[
                "Square",
                "Ramp up",
                "Ramp down",
                "Triangle",
                "Round",
                "Smooth",
            ],
            "Triangle",
        );
        assert_eq!(
            test.layer("words").text_layout.as_ref().unwrap().animators[0].range_shape,
            RangeShape::Triangle
        );

        let opened = test.inspector_menu(Some("Easing"), "Smooth");
        test.choose(
            &opened,
            &["Smooth", "Steady", "Hold", "Speed up", "Slow down"],
            "Slow down",
        );
        assert_eq!(test.layer("words").ease, "ease-out");
        let opened = test.inspector_menu(Some("Easing"), "Steady");
        test.choose(&opened, &["Steady", "Smooth"], "Smooth");
        assert_eq!(test.layer("words").rotation_clips[0].ease, "smooth");
        let opened = test.inspector_menu(Some("Comes back"), "Never");
        test.choose(
            &opened,
            &["Never", "At a time", "With an animation"],
            "With an animation",
        );
        assert_eq!(
            test.layer("words")
                .disappear
                .as_ref()
                .unwrap()
                .reappear_mode,
            "clip"
        );
        let flat = test.layer("flat");
        let clip = &flat.rotation_clips[0];
        let choice = format!(
            "{} \u{b7} Rotation 1 \u{b7} ends {:.2} s",
            flat.name,
            clip.start_time + clip.duration
        );
        let id = clip.id.clone();
        let opened = test.inspector_menu(None, "Choose an animation");
        test.choose(&opened, &[&choice], &choice);
        assert_eq!(
            test.layer("words")
                .disappear
                .as_ref()
                .unwrap()
                .trigger_clip_id
                .as_ref(),
            Some(&id)
        );
    }
}

#[test]
fn export_format_opens_and_reopens_from_idle() {
    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Layer, "nested");
        test.ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.animation_time = egui::Style::default().animation_time;
        });
        test.forma.show_export = true;
        let initial = test.frame(Vec::new());
        let idle = test.settle_repaints(initial);
        assert_eq!(
            idle.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
            Duration::MAX
        );
        let format = painted_label(&idle, test.screen(), "MP4", |_| true)
            .unwrap()
            .center();
        // No pointer movement, scrolling or playback is needed once the opening click is sent.
        let opened = test.click(format, egui::PointerButton::Primary);
        test.popup_option(&opened, "WebM (VP9)");
        let opened = test.settle_repaints(opened);
        test.choose(
            &opened,
            &[
                "MP4",
                "WebM (VP9)",
                "MOV (ProRes 4444)",
                "PNG sequence (ZIP)",
                "GIF",
            ],
            "WebM (VP9)",
        );
        assert!(test.forma.video_settings.format == video::Format::WebM);

        let after_selection = test.frame(vec![egui::Event::PointerGone]);
        let idle = test.settle_repaints(after_selection);
        assert_eq!(
            idle.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
            Duration::MAX
        );
        let format = painted_label(&idle, test.screen(), "WebM (VP9)", |_| true)
            .unwrap()
            .center();
        let reopened = test.click(format, egui::PointerButton::Primary);
        let reopened = test.settle_repaints(reopened);
        test.popup_option(&reopened, "MP4");
        test.press_key(egui::Key::Escape);
        assert!(!egui::Popup::is_any_open(&test.ctx));
        assert!(test.forma.show_export);
        test.press_key(egui::Key::Escape);
        assert!(!test.forma.show_export);
    }
}

#[test]
fn export_format_remains_clickable_after_saving_a_gif() {
    let size = vec2(1036.0, 790.0);
    let mut test = PopupTest::new(size, Tab::Layer, "flat");
    test.forma.scene = Scene::empty(new_id());
    test.forma.scene.duration = 0.25;
    (test.forma.scene.width, test.forma.scene.height) = (160.0, 100.0);
    test.ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.animation_time = egui::Style::default().animation_time;
    });
    let click = |test: &mut PopupTest, output: &egui::FullOutput, label| {
        let at = painted_label(output, test.screen(), label, |_| true)
            .unwrap_or_else(|| panic!("missing {label:?}"))
            .center();
        let output = test.click(at, egui::PointerButton::Primary);
        test.settle_repaints(output)
    };
    let output = test.frame(Vec::new());
    let output = test.settle_repaints(output);
    let output = click(&mut test, &output, "Export");
    let opened = click(&mut test, &output, "MP4");
    test.popup_option(&opened, "GIF");
    let output = test.press_key(egui::Key::Escape);
    let output = test.settle_repaints(output);
    assert!(test.forma.show_export);
    assert!(!egui::Popup::is_any_open(&test.ctx));
    assert_eq!(repaint_delay(&output), Duration::MAX);

    let opened = click(&mut test, &output, "MP4");
    let chosen = test.choose(&opened, &["GIF"], "GIF");
    let output = test.settle_repaints(chosen);
    assert_eq!(test.forma.video_settings.format, video::Format::Gif);
    let export = painted_label(&output, test.screen(), "Export video", |_| true)
        .unwrap()
        .center();
    let mut output = test.click(export, egui::PointerButton::Primary);
    assert!(!test.forma.show_export);

    let start = Instant::now();
    while test.saved.is_none() {
        let delay = repaint_delay(&output);
        assert!(
            delay <= Duration::from_secs(1) && start.elapsed() < Duration::from_secs(15),
            "GIF did not save"
        );
        std::thread::sleep(delay);
        output = test.advance(delay, Vec::new());
    }
    let saved = test.saved.as_ref().unwrap();
    assert!(std::fs::read(saved).unwrap().starts_with(b"GIF"));
    std::fs::remove_file(saved).unwrap();
    std::fs::remove_dir(&test.forma.place.exports_dir).unwrap();

    let output = test.settle_repaints(output);
    let reopened = click(&mut test, &output, "Export");
    let at = painted_label(&reopened, test.screen(), "GIF", |_| true)
        .unwrap()
        .center();
    let opened = test.short_click(at);
    let opened = test.settle_repaints(opened);
    test.root_popup_option(&opened, "WebM (VP9)");
    test.choose(
        &opened,
        &[
            "MP4",
            "WebM (VP9)",
            "MOV (ProRes 4444)",
            "PNG sequence (ZIP)",
            "GIF",
        ],
        "WebM (VP9)",
    );
    assert_eq!(test.forma.video_settings.format, video::Format::WebM);
}

#[test]
fn svg_color_picker_works_after_reopening_its_dialog() {
    let mut test = PopupTest::new(vec2(1036.0, 790.0), Tab::Layer, "flat");
    let layer = test.layer_mut("flat");
    layer.kind = "svg".into();
    layer.extra.insert(
        "svgSource".into(),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="80"><rect width="80" height="80" fill="red"/></svg>"#.into(),
    );
    for reopen in [false, true] {
        let output = test.inspector_menu(None, "Edit SVG\u{2026}");
        let output = test.settle_repaints(output);
        let recolor = painted_label(&output, test.screen(), "Recolor", |_| true)
            .unwrap()
            .center();
        let color = test
            .ctx
            .interactive_rects_last_pass()
            .into_iter()
            .filter(|rect| rect.right() < recolor.x && rect.y_range().contains(recolor.y))
            .max_by(|a, b| a.right().total_cmp(&b.right()))
            .unwrap();
        let output = if reopen {
            test.short_click(color.center())
        } else {
            test.click(color.center(), egui::PointerButton::Primary)
        };
        let output = test.settle_repaints(output);
        let space = test
            .ctx
            .global_style()
            .visuals
            .numeric_color_space
            .to_string();
        let toggle = test.root_popup_option(&output, &space);
        let output = test.click(toggle, egui::PointerButton::Primary);
        let output = test.settle_repaints(output);
        let changed = test
            .ctx
            .global_style()
            .visuals
            .numeric_color_space
            .to_string();
        assert_ne!(space, changed);
        test.root_popup_option(&output, &changed);
        let output = test.press_key(egui::Key::Escape);
        test.settle_repaints(output);
        assert!(test.forma.svg_edit.is_some());
        assert!(!egui::Popup::is_any_open(&test.ctx));
        let output = test.press_key(egui::Key::Escape);
        test.settle_repaints(output);
        assert!(test.forma.svg_edit.is_none());
    }
}

#[test]
fn escape_closes_svg_color_picker_before_its_dialog() {
    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Layer, "flat");
        test.forma.svg_edit = Some(SvgEdit {
            id: "flat".into(),
            source: r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="80"><rect width="80" height="80" fill="red"/></svg>"#.into(),
            color: [255, 0, 0],
            selected_part: None,
            status: String::new(),
        });
        test.frame(Vec::new());
        let recolor = test.label("Recolor");
        let color = test
            .ctx
            .interactive_rects_last_pass()
            .into_iter()
            .filter(|rect| rect.right() < recolor.x && rect.y_range().contains(recolor.y))
            .max_by(|a, b| a.right().total_cmp(&b.right()))
            .unwrap();
        test.click(color.center(), egui::PointerButton::Primary);
        assert!(egui::Popup::is_any_open(&test.ctx));
        test.press_key(egui::Key::Escape);
        assert!(!egui::Popup::is_any_open(&test.ctx));
        assert!(test.forma.svg_edit.is_some());
        test.press_key(egui::Key::Escape);
        assert!(test.forma.svg_edit.is_none());
    }
}

#[test]
fn composition_dialog_can_add_a_layer() {
    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Layer, "nested");
        test.forma.show_compositions = true;
        test.frame(Vec::new());
        let add = test.label("Add layer");
        let count = test.forma.scene.shapes.len();
        test.click(add, egui::PointerButton::Primary);
        assert_eq!(test.forma.scene.shapes.len(), count + 1);
        assert!(!test.forma.show_compositions);
    }
}

#[test]
fn timeline_and_expression_popups_paint_and_accept_input() {
    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Layer, "words");
        let quality = test.label("Full");
        let opened = test.click(quality, egui::PointerButton::Primary);
        test.choose(&opened, &["Full", "Half", "Quarter"], "Quarter");
        assert!(test.forma.quality == crate::preview_cache::Quality::Quarter);

        test.layer_mut("flat").name = "Backdrop".into();
        let lane = Lane::Pose(Property::Position);
        test.layer_mut("words").set_expression(lane, None);
        let timeline = Rect::from_min_max(
            test.pane.min,
            pos2(test.inspector().left(), test.pane.bottom()),
        );
        let link = test.scroll_to("Link to\u{2026}", timeline);
        let opened = test.click(link, egui::PointerButton::Primary);
        let submenu = test.choose(&opened, &["Backdrop"], "Backdrop");
        test.choose(&submenu, &["Position", "Size"], "Position");
        assert_eq!(
            test.layer("words").expression(lane),
            crate::expressions::link_source(test.layer("flat"), lane, None).as_deref()
        );

        let mut test = PopupTest::new(size, Tab::Layer, "depth");
        test.forma
            .scene
            .shapes
            .retain(|shape| shape.id == "depth" || shape.id == "flat");
        test.layer_mut("flat").name = "Backdrop".into();
        test.layer_mut("depth").name = "Foreground".into();
        test.layer_mut("depth")
            .extra
            .insert("trackId".into(), "foreground".into());
        let clip_label = test.label("Foreground");
        let opened = test.click(clip_label, egui::PointerButton::Secondary);
        let submenu = test.choose(
            &opened,
            &[
                "Follows",
                "Split at playhead",
                "Duplicate",
                "Copy",
                "Delete clip",
            ],
            "Follows",
        );
        test.choose(&submenu, &["None", "Backdrop"], "None");
        assert!(test.layer("depth").parent.is_none());

        let opened = test.click(clip_label, egui::PointerButton::Secondary);
        let count = test.forma.scene.shapes.len();
        test.choose(
            &opened,
            &[
                "Follows",
                "Split at playhead",
                "Duplicate",
                "Copy",
                "Delete clip",
            ],
            "Duplicate",
        );
        assert_eq!(test.forma.scene.shapes.len(), count + 1);
    }
}

#[test]
fn timeline_key_easing_popups_edit_the_selected_key() {
    let lane = Lane::Pose(Property::Position);
    let labels = crate::keyframes::EASINGS.map(|(label, _)| label);
    for size in WINDOW_SIZES {
        let mut test = PopupTest::new(size, Tab::Layer, "flat");
        test.forma.scene.shapes.retain(|shape| shape.id == "flat");
        test.forma.key_editor = KeyEditor::default();
        test.forma.key_editor.expanded.insert("flat".into());
        test.layer_mut("flat").add_lane_key(lane, 0.5);
        test.frame(Vec::new());
        let point = test
            .forma
            .key_editor
            .visible_key_point_for_test("flat", lane, 0.5)
            .unwrap();
        let opened = test.click(point, egui::PointerButton::Secondary);
        test.choose(&opened, &labels, "Hold");
        let key = test.layer("flat").lane_key(lane, 0.5).unwrap();
        assert_eq!(key.easing(&test.layer("flat").ease).0, "hold");

        let easing = test.label("Hold");
        let opened = test.click(easing, egui::PointerButton::Primary);
        test.choose(&opened, &labels, "Speed up");
        let key = test.layer("flat").lane_key(lane, 0.5).unwrap();
        assert_eq!(key.easing(&test.layer("flat").ease).0, "ease-in");
    }
}

#[test]
fn workspace_panels_fit_at_both_window_sizes() {
    let mut forma = fixture();
    for size in [vec2(1456.0, 819.0), vec2(1920.0, 1080.0)] {
        for narrow in [false, true] {
            let pane = pane_at(size, narrow);
            for (tab, layer, drawer, expected) in [
                (
                    Tab::Layer,
                    "words",
                    Some(Drawer::Add),
                    &[
                        "Blend & mask",
                        "Editable in StreamCuts",
                        "Effects",
                        "Position & size",
                        "Drop shadow",
                        "Distance",
                    ][..],
                ),
                (
                    Tab::Layer,
                    "depth",
                    Some(Drawer::Layers),
                    &[
                        "3D transform",
                        "X rotation",
                        "Gradient stops",
                        "Dash offset",
                        "Trim paths",
                    ],
                ),
                (Tab::Layer, "nested", None, &["Composition playback"]),
                (
                    Tab::Layer,
                    "camera",
                    None,
                    &["Aim and focus", "Focus distance"],
                ),
                (
                    Tab::Animate,
                    "words",
                    None,
                    &[
                        "Based on",
                        "Characters excluding spaces",
                        "Range easing and order",
                        "What changes",
                        "Comes back",
                    ],
                ),
                (Tab::Scene, "flat", None, &["Motion blur", "Shutter angle"]),
            ] {
                let ctx = setup();
                forma.tab = tab;
                forma.selected = Some(layer.into());
                forma.last_picked = (forma.selected_motion.clone(), forma.selected_spin.clone());
                forma.drawer = drawer;
                forma.preview = None;
                let case = format!("{size:?}, narrow={narrow}, {tab:?}, {layer}");
                let mut labels = HashSet::new();
                // Interleave viewports so controls cut by one scroll boundary are fully visible at the next step.
                let offset_id = egui::Id::unique("forma-test-scroll");
                let extent_id = egui::Id::unique("forma-test-scroll-extent");
                let mut offset = 0.0;
                loop {
                    ctx.data_mut(|d| d.insert_temp(offset_id, offset));
                    let output = run_frame(&ctx, size, |ui| {
                        ui.scope_builder(UiBuilder::new().max_rect(pane), |ui| {
                            ui.set_clip_rect(pane);
                            CanopyApp::ui(&mut forma, ui, &mut LayoutHost);
                        });
                    });
                    let case = format!("{case}, scroll={offset}");
                    let here = check_text(&output, pane, &case);
                    check_widgets(&ctx, &case);
                    for fixed in ["Layer", "Animate", "Scene"] {
                        assert!(here.contains(fixed), "{case}: missing tab {fixed}");
                    }
                    labels.extend(here);
                    let extent = ctx
                        .data(|d| d.get_temp::<f32>(extent_id))
                        .unwrap_or_default();
                    if offset >= extent {
                        break;
                    }
                    offset = (offset + 120.0).min(extent);
                }
                for label in expected
                    .iter()
                    .copied()
                    .chain(["value +", "Link to\u{2026}"])
                {
                    assert!(
                        labels.contains(label),
                        "{case}: never fully visible: {label:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn dialog_actions_fit_at_both_window_sizes() {
    let mut forma = fixture();
    let src = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="red"/></svg>"#;
    for size in [vec2(1456.0, 819.0), vec2(1920.0, 1080.0)] {
        forma.pane = pane_at(size, false);
        for (title, action, draw) in [
            (
                "Export",
                "Export video",
                Forma::export_window as fn(&mut Forma, &mut Ui, &mut dyn Host),
            ),
            ("Compositions", "Done", |f, ui, _| f.compositions_window(ui)),
            ("Start screen", "Back to the editor", |f, ui, _| {
                f.projects_window(ui)
            }),
            ("Quick guide", "Got it", |f, ui, _| f.help_window(ui)),
            ("Edit SVG", "Apply", |f, ui, _| f.svg_window(ui)),
        ] {
            let ctx = setup();
            forma.show_export = title == "Export";
            forma.show_compositions = title == "Compositions";
            forma.show_projects = title == "Start screen";
            forma.show_help = title == "Quick guide";
            forma.svg_edit = (title == "Edit SVG").then(|| SvgEdit {
                id: "flat".into(),
                source: src.into(),
                color: [255, 255, 255],
                selected_part: None,
                status: String::new(),
            });
            forma.svg_preview = None;
            forma.export_error = Some("One of the layers could not be exported. ".repeat(100));
            for _ in 0..2 {
                run_frame(&ctx, size, |ui| draw(&mut forma, ui, &mut LayoutHost));
            }
            let output = run_frame(&ctx, size, |ui| draw(&mut forma, ui, &mut LayoutHost));
            let labels = check_text(&output, forma.pane, title);
            assert!(
                labels.contains(action),
                "{title}: action {action:?} is outside the viewport, visible labels: {labels:?}"
            );
            check_widgets(&ctx, title);
        }
    }
}

#[test]
fn dragging_the_timeline_divider_persists_the_chosen_height() {
    let mut forma = fixture();
    let ctx = setup();
    let size = vec2(1456.0, 819.0);
    let rect = pane_at(size, false);
    let mut timeline = Rect::NOTHING;
    run_frame(&ctx, size, |ui| {
        timeline = forma.split_workspace(ui, rect).1;
    });
    assert!((300.0..=330.0).contains(&timeline.height()));
    let start = timeline.center_top() - vec2(0.0, 4.0);
    let end = start - vec2(0.0, 60.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for events in [
        vec![egui::Event::PointerMoved(start), button(start, true)],
        vec![egui::Event::PointerMoved(end)],
        vec![button(end, false)],
    ] {
        input_frame(&ctx, size, events, |ui| {
            timeline = forma.split_workspace(ui, rect).1;
        });
    }
    assert!((360.0..=390.0).contains(&timeline.height()));
    assert_eq!(
        storage::timeline_height(&forma.place),
        Some(timeline.height())
    );
    std::fs::remove_file(forma.place.data_dir.join("timeline-height.json")).unwrap();
    std::fs::remove_dir(&forma.place.data_dir).unwrap();
}

#[test]
fn multiline_text_can_be_selected_in_the_space_around_its_second_line() {
    let mut forma = fixture();
    forma.scene.shapes.clear();
    forma.scene.add_text("words".into());
    forma.scene.shapes[0]
        .extra
        .insert("text".into(), "Hello Forma\nsecond line".into());
    forma.selected = None;
    let size = vec2(800.0, 500.0);
    let ctx = setup();
    run_frame(&ctx, size, |ui| forma.canvas_view(ui));
    let em = forma.scene.shapes[0].pose(0.0).h;
    let click = (size / 2.0).to_pos2() + vec2(0.0, 0.9 * em);
    for pressed in [true, false] {
        input_frame(
            &ctx,
            size,
            vec![
                egui::Event::PointerMoved(click),
                egui::Event::PointerButton {
                    pos: click,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            |ui| forma.canvas_view(ui),
        );
    }
    assert_eq!(forma.selected.as_deref(), Some("words"));
}
