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
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        app,
    );
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
