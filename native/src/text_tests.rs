use crate::keyframes::{Lane, Property};
use crate::model::{Pose, Scene};
use crate::paths::{Contour, PathData, Vertex};
use crate::render::{Frames, Renderer};
use crate::shape_style::{PaintMode, PaintStyle, StrokeStyle, VectorLayer};
use crate::text_animator::{
    Alignment, Animator, RangeShape, TextLane, TextLayout, TextProperty, Units,
};
use crate::text_layout::TextCache;
use crate::text_presets::Preset;
use crate::text_render::{GlyphEffect, SampledAnimator};
use canopy_ui::Place;
use std::io::Write;
use std::process::{Command, Stdio};
use tiny_skia::{Color, FillRule, Pixmap, Transform};

fn place() -> Place {
    let dir = std::env::temp_dir();
    Place {
        space: None,
        projects_dir: dir.clone(),
        data_dir: dir.clone(),
        exports_dir: dir,
    }
}

fn scene(words: &str) -> Scene {
    let mut scene = Scene::empty("phase-four".into());
    (scene.width, scene.height) = (560.0, 320.0);
    scene.bg = "#000000".into();
    scene.add_text("title".into());
    let text = &mut scene.shapes[0];
    text.name = "Example title".into();
    text.extra.insert("text".into(), words.into());
    text.color = "#ffffff".into();
    (text.keys[0].w, text.keys[0].h) = (232.0, 52.0);
    scene
}

fn draw(renderer: &Renderer, scene: &Scene, t: f64) -> Pixmap {
    renderer
        .frame(
            scene,
            t,
            (scene.width as u32, scene.height as u32),
            &Frames::new(),
        )
        .unwrap()
}

fn mass(pixmap: &Pixmap) -> u64 {
    pixmap.pixels().iter().map(|p| u64::from(p.red())).sum()
}

fn bounds(pixmap: &Pixmap, rows: std::ops::Range<u32>) -> (u32, u32, u32, u32) {
    let width = pixmap.width();
    let (mut left, mut right, mut top, mut bottom) = (width, 0, pixmap.height(), 0);
    for y in rows {
        for x in 0..width {
            if pixmap.pixel(x, y).is_some_and(|p| p.red() >= 48) {
                left = left.min(x);
                right = right.max(x + 1);
                top = top.min(y);
                bottom = bottom.max(y + 1);
            }
        }
    }
    assert!(left < right && top < bottom, "the queried area has no text");
    (left, right, top, bottom)
}

fn all_bounds(pixmap: &Pixmap) -> (u32, u32, u32, u32) {
    bounds(pixmap, 0..pixmap.height())
}

#[test]
fn rustybuzz_keeps_kerning_and_ligatures() {
    let mut cache = TextCache::default();
    let style = TextLayout::default();
    let width = |cache: &mut TextCache, words| cache.layout(words, words, &style).width;
    let a = width(&mut cache, "A");
    let v = width(&mut cache, "V");
    assert!(width(&mut cache, "AV") < a + v - 0.015);
    let ligature = cache.layout("ligature", "office", &style);
    assert!(ligature.lines[0].glyphs.len() < 6);
    assert_eq!(
        ligature.count(Units::Characters),
        ligature.lines[0].clusters.len()
    );
    let again = cache.layout("ligature", "office", &style);
    assert!(std::sync::Arc::ptr_eq(&ligature, &again));
}

#[test]
fn multiline_alignment_line_height_and_tracking_move_ink() {
    let mut scene = scene("MMMM\n\nMM");
    let renderer = Renderer::new(&scene, &place());
    let mut short_lefts = Vec::new();
    for alignment in [Alignment::Left, Alignment::Center, Alignment::Right] {
        scene.shapes[0].text_layout.as_mut().unwrap().alignment = alignment;
        let frame = draw(&renderer, &scene, 0.0);
        let long_left = bounds(&frame, 40..135).0;
        if alignment == Alignment::Left {
            short_lefts.push(long_left);
        } else {
            assert_eq!(long_left, short_lefts[0]);
        }
        assert!(
            frame
                .pixels()
                .chunks_exact(560)
                .skip(135)
                .take(42)
                .flatten()
                .all(|p| p.red() == 0),
            "the blank line leaves a gap"
        );
        short_lefts.push(bounds(&frame, 177..260).0);
    }
    assert!(
        short_lefts[1..]
            .windows(2)
            .all(|pair| pair[1] - pair[0] > 45)
    );
    let before = all_bounds(&draw(&renderer, &scene, 0.0));
    TextLane::Style(TextProperty::LineHeight).set(&mut scene.shapes[0], 0.0, [180.0, 0.0, 0.0]);
    TextLane::Style(TextProperty::Tracking).set(&mut scene.shapes[0], 0.0, [10.0, 0.0, 0.0]);
    let after = all_bounds(&draw(&renderer, &scene, 0.0));
    assert!(after.0 + 25 <= before.0);
    assert!(after.2 + 25 <= before.2 && after.3 >= before.3 + 25);
}

#[test]
fn every_range_shape_uses_the_same_character_word_and_line_units() {
    use RangeShape::*;
    let shaped = TextCache::default().layout("range", "AA B\nC DD", &TextLayout::default());
    let lines = &shaped.lines;
    assert_eq!(
        [
            Units::Characters,
            Units::VisibleCharacters,
            Units::Words,
            Units::Lines
        ]
        .map(|unit| shaped.count(unit)),
        [8, 6, 4, 2]
    );
    assert_eq!(lines[0].clusters[2].unit(Units::Characters), Some(2));
    assert_eq!(lines[0].clusters[2].unit(Units::VisibleCharacters), None);
    assert_eq!(lines[0].clusters[3].unit(Units::Words), Some(1));
    assert_eq!(lines[1].clusters[2].unit(Units::Lines), Some(1));

    let checkpoints = [
        (Square, [0.0, 1.0, 1.0, 1.0, 0.0]),
        (RampUp, [0.0, 0.25, 0.5, 0.75, 1.0]),
        (RampDown, [1.0, 0.75, 0.5, 0.25, 0.0]),
        (Triangle, [0.0, 0.5, 1.0, 0.5, 0.0]),
        (Round, [0.0, 0.8660254, 1.0, 0.8660254, 0.0]),
        (Smooth, [0.0, 0.5, 1.0, 0.5, 0.0]),
    ];
    for (shape, expected) in checkpoints {
        for (at, expected) in [0.0, 35.0, 50.0, 65.0, 100.0].into_iter().zip(expected) {
            assert!(
                (shape.amount(at, 20.0, 80.0) - expected).abs() < 0.00001,
                "{shape:?}"
            );
        }
        for units in [
            Units::Characters,
            Units::VisibleCharacters,
            Units::Words,
            Units::Lines,
        ] {
            let mut animator = Animator {
                units,
                range_shape: shape,
                ..Animator::default()
            };
            animator.enable(TextProperty::Start, [20.0, 0.0, 0.0]);
            animator.enable(TextProperty::End, [80.0, 0.0, 0.0]);
            animator.enable(TextProperty::Opacity, [0.0; 3]);
            let sampled = SampledAnimator::new(&animator, &shaped, 0.0, "linear");
            for cluster in lines.iter().flat_map(|line| &line.clusters) {
                let count = shaped.count(units);
                let expected = cluster.unit(units).map_or(1.0, |index| {
                    1.0 - shape.amount((index as f32 + 0.5) * 100.0 / count as f32, 20.0, 80.0)
                });
                assert!(
                    (GlyphEffect::at(std::slice::from_ref(&sampled), cluster).opacity - expected)
                        .abs()
                        < 0.00001
                );
            }
        }
    }
    assert_eq!(Square.amount(50.0, 50.0, 50.0), 0.0);
    assert_eq!(RampDown.amount(25.0, 50.0, 50.0), 1.0);
}

#[test]
fn seeded_random_order_is_a_stable_permutation_and_range_ease_pins_endpoints() {
    let mut animator = Animator {
        seed: Some(42),
        range_shape: RangeShape::RampUp,
        ..Animator::default()
    };
    let shuffled = animator.weights(16, 0.0, "linear");
    assert_eq!(shuffled, animator.weights(16, 2.0, "hold"));
    animator.seed = Some(7);
    let other = animator.weights(16, 0.0, "linear");
    assert_ne!(shuffled, other);
    let mut sorted = shuffled;
    sorted.sort_by(f32::total_cmp);
    animator.seed = None;
    assert_eq!(sorted, animator.weights(16, 0.0, "linear"));
    animator.enable(TextProperty::Start, [15.0, 0.0, 0.0]);
    animator.enable(TextProperty::End, [85.0, 0.0, 0.0]);
    let neutral = animator.weights(10, 0.0, "linear");
    animator.enable(TextProperty::EaseLow, [100.0, 0.0, 0.0]);
    let low = animator.weights(10, 0.0, "linear");
    animator.enable(TextProperty::EaseHigh, [100.0, 0.0, 0.0]);
    let both = animator.weights(10, 0.0, "linear");
    assert!(low[2] < neutral[2] && both[7] > low[7]);
    assert_eq!((both[0], both[9]), (0.0, 1.0));
    assert!(animator.weights(0, 0.0, "linear").is_empty());
}

fn preset_scene(preset: Preset, words: &str, width: f32) -> (Scene, f64) {
    let mut scene = scene(words);
    scene.shapes[0].keys[0].w = width;
    let end = preset.length();
    preset
        .apply(
            scene.shapes[0].text_layout.as_mut().unwrap(),
            0.0,
            end,
            52.0,
        )
        .unwrap();
    (scene, end)
}

#[test]
fn typewriter_fade_up_and_pop_reveal_text_with_distinct_motion() {
    for preset in [Preset::Typewriter, Preset::FadeUp, Preset::Pop] {
        let (scene, end) = preset_scene(preset, "HI HI HI", 232.0);
        let renderer = Renderer::new(&scene, &place());
        let start = draw(&renderer, &scene, 0.0);
        let middle = draw(&renderer, &scene, end * 0.46);
        let final_frame = draw(&renderer, &scene, end);
        assert_eq!(mass(&start), 0, "{preset:?}");
        assert!(
            mass(&middle) > 0 && mass(&middle) < mass(&final_frame),
            "{preset:?}"
        );
        assert!(
            all_bounds(&middle).1 < all_bounds(&final_frame).1,
            "{preset:?}"
        );
    }
    let (scene, end) = preset_scene(Preset::FadeUp, "HI HI", 180.0);
    let renderer = Renderer::new(&scene, &place());
    let partial_word = draw(&renderer, &scene, 0.25 * end);
    assert!(all_bounds(&partial_word).2 > all_bounds(&draw(&renderer, &scene, end)).2 + 12);
    assert!(partial_word.pixels().iter().map(|p| p.red()).max().unwrap() < 200);

    let (scene, end) = preset_scene(Preset::Pop, "H", 50.0);
    let renderer = Renderer::new(&scene, &place());
    let rest = mass(&draw(&renderer, &scene, end));
    assert!(
        (5..16).any(
            |step| mass(&draw(&renderer, &scene, f64::from(step) * end / 20.0)) > rest * 12 / 10
        )
    );
}

#[test]
fn wave_and_bounce_cross_the_baseline_at_different_times() {
    for preset in [Preset::Wave, Preset::Bounce] {
        let (scene, end) = preset_scene(preset, "H", 50.0);
        let renderer = Renderer::new(&scene, &place());
        let rest = draw(&renderer, &scene, 0.0);
        assert_eq!(rest.data(), draw(&renderer, &scene, end).data());
        let baseline = all_bounds(&rest).2;
        let lifted = all_bounds(&draw(
            &renderer,
            &scene,
            end * if matches!(preset, Preset::Wave) {
                0.5
            } else {
                0.39
            },
        ))
        .2;
        let later = all_bounds(&draw(&renderer, &scene, end * 0.61)).2;
        assert!(lifted + 25 < baseline);
        if matches!(preset, Preset::Bounce) {
            assert!(
                later >= baseline + 12,
                "the landing rebounds below baseline"
            );
        } else {
            assert!(later < baseline, "the wave stays above baseline");
        }
    }
}

#[test]
fn slide_tracking_and_blur_settle_across_frames() {
    let (scene, end) = preset_scene(Preset::Slide, "HI\nHI", 150.0);
    let renderer = Renderer::new(&scene, &place());
    let start = draw(&renderer, &scene, 0.0);
    let middle = draw(&renderer, &scene, end * 0.5);
    let finish = draw(&renderer, &scene, end);
    let left = |pixmap, rows| bounds(pixmap, rows).0;
    assert!(left(&start, 40..155) + 100 < left(&middle, 40..155));
    assert_eq!(left(&middle, 40..155), left(&finish, 40..155));
    assert_eq!(left(&middle, 160..260), left(&start, 160..260));

    let (scene, end) = preset_scene(Preset::Tracking, "HELLO", 160.0);
    let renderer = Renderer::new(&scene, &place());
    let width = |t| {
        let (l, r, _, _) = all_bounds(&draw(&renderer, &scene, t));
        r - l
    };
    assert!(width(0.0) > width(end * 0.45) && width(end * 0.45) > width(end));

    let (scene, end) = preset_scene(Preset::Blur, "HELLO", 160.0);
    let renderer = Renderer::new(&scene, &place());
    let energy = |t| {
        draw(&renderer, &scene, t)
            .pixels()
            .chunks_exact(560)
            .flat_map(|row| row.windows(2))
            .map(|p| (i64::from(p[1].red()) - i64::from(p[0].red())).pow(2))
            .sum::<i64>()
    };
    assert!(energy(0.0) < energy(end * 0.5) && energy(end * 0.5) < energy(end) / 2);
}

fn add_path(scene: &mut Scene) {
    scene.add_shape("track".into(), "path");
    let track = scene.shapes.last_mut().unwrap();
    (
        track.keys[0].x,
        track.keys[0].y,
        track.keys[0].w,
        track.keys[0].h,
        track.keys[0].o,
    ) = (245.0, 185.0, 300.0, 100.0, 0.0);
    track.vector = Some(VectorLayer {
        path: Some(PathData {
            contours: vec![Contour {
                points: vec![Vertex::corner([-0.5, 0.0]), Vertex::corner([0.5, 0.0])],
                closed: false,
            }],
        }),
        ..VectorLayer::default()
    });
    scene.shapes[0].text_layout.as_mut().unwrap().path = Some("track".into());
}

#[test]
fn text_follows_the_path_transform_and_animates_its_offset() {
    let mut scene = scene("HI");
    scene.shapes[0].keys[0].w = 90.0;
    add_path(&mut scene);
    let renderer = Renderer::new(&scene, &place());
    let lane = TextLane::Style(TextProperty::PathOffset);
    let text = &mut scene.shapes[0];
    text.ease = "linear".into();
    text.add_lane_key(Lane::Text(lane), 0.0);
    lane.set(text, 1.0, [65.0, 0.0, 0.0]);
    let a = all_bounds(&draw(&renderer, &scene, 0.0));
    let b = all_bounds(&draw(&renderer, &scene, 1.0));
    assert_eq!(b.0 - a.0, 65);
    assert_eq!(a.2, b.2);
    assert!(a.3 <= 185);
    let track = &mut scene.shapes[1];
    track.key_property(
        2.0,
        Property::Rotation,
        Pose {
            r: 90.0,
            ..track.pose(0.0)
        },
    );
    let c = all_bounds(&draw(&renderer, &scene, 2.0));
    assert!(c.0 >= 245 && c.3 > b.3 + 70 && c.1 - c.0 < c.3 - c.2);
    let frame = draw(&renderer, &scene, 1.2);
    assert_eq!(
        frame.data(),
        renderer
            .preview(&scene, 1.2, 1.0, &Frames::new())
            .unwrap()
            .data()
    );
    let err = crate::animated_svg::animated(&scene, &place()).unwrap_err();
    assert!(err.contains("Example title") && err.contains("text along a path"));
    scene.delete_layers(&["track".into()], 1.0);
    let text = scene.shapes[0].text_layout.as_ref().unwrap();
    assert!(text.path.is_none() && !text.style.contains_key(&TextProperty::PathOffset));
}

#[test]
fn fill_and_stroke_animators_change_the_selected_glyph_and_keep_the_layer_opacity() {
    let mut scene = scene("HH");
    let text = &mut scene.shapes[0];
    text.keys[0].o = 50.0;
    text.vector = Some(VectorLayer {
        fill: Some(PaintStyle::new(PaintMode::Solid)),
        stroke: Some(StrokeStyle {
            rgba: [0.0, 0.0, 255.0, 100.0],
            width: 5.0,
            ..StrokeStyle::default()
        }),
        ..VectorLayer::default()
    });
    let mut animator = Animator::default();
    animator.enable(TextProperty::End, [50.0, 0.0, 0.0]);
    animator.enable(TextProperty::Fill, [255.0, 0.0, 0.0]);
    animator.enable(TextProperty::Stroke, [0.0, 255.0, 0.0]);
    animator.enable(TextProperty::Skew, [20.0, 0.0, 0.0]);
    text.text_layout.as_mut().unwrap().add(animator);
    let pixmap = draw(&Renderer::new(&scene, &place()), &scene, 0.0);
    let left: Vec<_> = pixmap
        .pixels()
        .chunks_exact(560)
        .flat_map(|row| &row[..280])
        .collect();
    assert!(
        left.iter()
            .any(|p| p.red() >= 125 && p.green() <= 2 && p.blue() == 0)
    );
    assert!(
        left.iter()
            .any(|p| p.green() >= 125 && p.red() <= 2 && p.blue() == 0)
    );
    let right: Vec<_> = pixmap
        .pixels()
        .chunks_exact(560)
        .flat_map(|row| &row[280..])
        .collect();
    assert!(
        right
            .iter()
            .any(|p| p.blue() >= 125 && p.green() <= 2 && p.red() <= 2)
    );
    assert!(
        right
            .iter()
            .any(|p| p.red() >= 125 && p.red() == p.green() && p.red() == p.blue())
    );
    assert!(
        pixmap
            .pixels()
            .iter()
            .all(|p| p.red().max(p.green()).max(p.blue()) <= 128)
    );
}

#[test]
fn legacy_linked_text_matches_its_previous_outline_pixels() {
    let scene = Scene::read(include_bytes!("../tests/fixtures/animated.json")).unwrap();
    let shape = scene
        .shapes
        .iter()
        .find(|shape| shape.kind == "text")
        .unwrap();
    assert!(shape.text_layout.is_none());
    let id = shape.id.clone();
    let mut with_only_text = scene;
    with_only_text.bg = "#000000".into();
    // Keep the linked target and its animated pose, but give its artwork the scene background color.
    for layer in &mut with_only_text.shapes {
        if layer.id != id {
            layer.color = "#000000".into();
            layer.color_keys.clear();
        }
    }
    let shape = with_only_text
        .shapes
        .iter()
        .find(|shape| shape.id == id)
        .unwrap();
    let renderer = Renderer::new(&with_only_text, &place());
    for t in [0.0, 1.2, 2.7] {
        let frame = with_only_text.layer_frame(shape, t);
        let outline = crate::text::outline(crate::text::content(shape), frame.pose.w, frame.pose.h)
            .unwrap()
            .transform(frame.transform)
            .unwrap();
        let mut expected = Pixmap::new(960, 600).unwrap();
        expected.fill(Color::BLACK);
        expected.fill_path(
            &outline,
            &crate::render::fill(&shape.color_at(t), frame.pose.o),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        assert_eq!(
            expected.data(),
            draw(&renderer, &with_only_text, t).data(),
            "time {t}"
        );
    }
}

#[test]
fn text_lanes_use_graph_curves_retime_and_move_with_the_clip() {
    let (mut scene, _) = preset_scene(Preset::Typewriter, "Hello", 160.0);
    let text = &mut scene.shapes[0];
    let id = text.text_layout.as_ref().unwrap().animators[0].id;
    let property = TextLane::Animator(id, TextProperty::Offset);
    let lane = Lane::Text(property);
    let mut key = text.lane_key(lane, 0.0).unwrap();
    key.move_handle(&text.ease, 0, 0.4, -0.2);
    key.move_handle(&text.ease, 1, 0.8, 0.2);
    text.put_lane_key(0.0, key);
    let middle = lane.value(text, 0.7, 0);
    assert!(middle < 35.0);
    assert!(text.retime_lane_keys(lane, &[0.0, 1.4], 0.5));
    assert_eq!(
        text.lane_keys(lane)
            .iter()
            .map(|k| k.time())
            .collect::<Vec<_>>(),
        [0.5, 1.9]
    );
    scene.shift_layer(0, 1.0);
    let text = &scene.shapes[0];
    assert!((lane.value(text, 2.2, 0) - middle).abs() < 0.0001);
    assert_eq!(
        text.lane_keys(lane)
            .iter()
            .map(|k| k.time())
            .collect::<Vec<_>>(),
        [1.5, 2.9]
    );
    assert!(text.lane_key(lane, 1.5).unwrap().handles(&text.ease)[1] < 0.0);
}

#[test]
#[ignore = "requires Node.js to run Electron Forma's validators"]
fn text_phase_round_trips_through_electrons_actual_validators() {
    let (mut scene, _) = preset_scene(Preset::Pop, "AV waves\nHello", 232.0);
    add_path(&mut scene);
    scene.link_text(0, 1, 0.0);
    scene.shapes[0].text_layout.as_mut().unwrap().animators[0].seed = Some(412);
    let lane = TextLane::Style(TextProperty::PathOffset);
    scene.shapes[0].add_lane_key(Lane::Text(lane), 0.0);
    lane.set(&mut scene.shapes[0], 1.0, [60.0, 0.0, 0.0]);
    scene.shapes[0].extra.insert("blur".into(), 30.into());
    let bytes = serde_json::to_vec(&scene).unwrap();
    assert_eq!(Scene::read(&bytes).unwrap(), scene);
    let mut node = Command::new("node")
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
        .args([
            "--input-type=module",
            "-e",
            r#"
            import {readFileSync} from 'node:fs';
            import {valid, usableProject} from './src/model/validate.js';
            const project = JSON.parse(readFileSync(0, 'utf8'));
            if (!valid(project) || !usableProject(project)) process.exit(1);
            process.stdout.write(JSON.stringify(project));
        "#,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    node.stdin.take().unwrap().write_all(&bytes).unwrap();
    let output = node.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Electron rejected the serialized scene"
    );
    let after = Scene::read(&output.stdout).unwrap();
    assert_eq!(after.shapes[0].text_layout, scene.shapes[0].text_layout);
    assert_eq!(after.shapes[1].vector, scene.shapes[1].vector);
    for t in [0.0, 0.5, 1.0, 2.0] {
        assert_eq!(
            after.render_pose(&after.shapes[0], t),
            scene.render_pose(&scene.shapes[0], t)
        );
    }
}

#[test]
fn svg_blocks_text_animators_and_still_svg_uses_the_rendered_glyphs() {
    let (scene, end) = preset_scene(Preset::Wave, "AV waves\nHello", 232.0);
    let error = crate::animated_svg::animated(&scene, &place()).unwrap_err();
    assert!(error.contains("Example title") && error.contains("per-glyph text animators"));
    let t = end * 0.45;
    let image = crate::animated_svg::still(&scene, &place(), t).unwrap();
    assert!(image.contains("data:image/png;base64,"));
    let tree = crate::svg::parse(&image).unwrap();
    let mut svg = Pixmap::new(560, 320).unwrap();
    resvg::render(&tree, Transform::identity(), &mut svg.as_mut());
    let frame = draw(&Renderer::new(&scene, &place()), &scene, t);
    let max_difference = frame
        .data()
        .iter()
        .zip(svg.data())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(max_difference <= 2, "SVG pixel error {max_difference}");
}

#[test]
fn animators_keep_linked_text_moving_with_its_target() {
    let mut scene = scene("Linked");
    scene.add_shape("parent".into(), "rect");
    let parent = &mut scene.shapes[1];
    parent.color = "#000000".into();
    let pose = Pose {
        x: parent.keys[0].x + 90.0,
        y: parent.keys[0].y - 23.0,
        ..parent.pose(0.0)
    };
    parent.key_property(1.0, Property::Position, pose);
    scene.link_text(0, 1, 0.0);
    let text = &mut scene.shapes[0];
    let mut animator = Animator::default();
    animator.enable(TextProperty::Rotation, [10.0, 0.0, 0.0]);
    text.text_layout.as_mut().unwrap().add(animator);
    let renderer = Renderer::new(&scene, &place());
    let a = all_bounds(&draw(&renderer, &scene, 0.0));
    let b = all_bounds(&draw(&renderer, &scene, 1.0));
    assert_eq!(b.0 - a.0, 90);
    assert_eq!(a.2 - b.2, 23);
}

#[test]
fn thirty_text_layers_scrub_at_full_hd() {
    let mut scene = scene("Frame");
    scene.width = 1920.0;
    scene.height = 1080.0;
    scene.shapes.clear();
    for i in 0..30 {
        scene.add_text(format!("title-{i}"));
        let shape = &mut scene.shapes[i];
        let key = &mut shape.keys[0];
        (key.x, key.y, key.w, key.h) = (
            230.0 + (i % 4) as f32 * 475.0,
            65.0 + (i / 4) as f32 * 135.0,
            190.0,
            34.0,
        );
        shape
            .extra
            .insert("text".into(), format!("Forma title {i}").into());
        shape.color = "#ffffff".into();
        // Include moving glyphs, whole-word opacity, and a few with per-glyph blur.
        let preset = [
            Preset::Wave,
            Preset::FadeUp,
            Preset::Blur,
            Preset::Tracking,
            Preset::Bounce,
        ][i % 5];
        preset
            .apply(
                shape.text_layout.as_mut().unwrap(),
                0.0,
                preset.length(),
                34.0,
            )
            .unwrap();
    }
    let renderer = Renderer::new(&scene, &place());
    let view = ((0.0, 0.0), 1.0, (1920, 1080));
    let frames = Frames::new();
    renderer.view(&scene, 0.5, view, true, &frames).unwrap();
    let mut durations = Vec::new();
    for step in 0..12 {
        let time = 0.12 + f64::from(step) * 0.075;
        let start = std::time::Instant::now();
        let frame = renderer.view(&scene, time, view, true, &frames).unwrap();
        durations.push(start.elapsed());
        assert!(mass(&frame) > 0);
    }
    durations.sort_unstable();
    eprintln!(
        "30 text layers at 1080p, warm renderer: median {:.1} ms, slowest {:.1} ms",
        durations[durations.len() / 2].as_secs_f64() * 1000.0,
        durations.last().unwrap().as_secs_f64() * 1000.0
    );
}

#[cfg(windows)]
#[test]
fn windows_font_picker_uses_real_bold_and_italic_outlines() {
    use crate::text_layout::{available_styles, font_families};
    let family = font_families()
        .iter()
        .find(|name| name.as_str() == "Segoe UI")
        .expect("Segoe UI is bundled with Windows");
    assert_eq!(available_styles(family), (true, true));
    let mut scene = scene("AV slants");
    let renderer = Renderer::new(&scene, &place());
    scene.shapes[0].text_layout.as_mut().unwrap().font = Some(family.clone());
    let normal = draw(&renderer, &scene, 0.0);
    scene.shapes[0].text_layout.as_mut().unwrap().bold = true;
    let bold = draw(&renderer, &scene, 0.0);
    assert!(mass(&bold) > mass(&normal) + mass(&normal) / 10);
    let text = scene.shapes[0].text_layout.as_mut().unwrap();
    text.bold = false;
    text.italic = true;
    let italic = draw(&renderer, &scene, 0.0);
    assert_ne!(normal.data(), italic.data());
}

#[test]
fn basic_multiline_text_exports_as_shaped_vector_contours() {
    let mut scene = scene("Office\nAV");
    scene.shapes[0].text_layout.as_mut().unwrap().alignment = Alignment::Right;
    TextLane::Style(TextProperty::LineHeight).set(&mut scene.shapes[0], 0.0, [140.0, 0.0, 0.0]);
    let source = crate::animated_svg::animated(&scene, &place()).unwrap();
    assert!(
        source.contains("<path") && !source.contains("<text") && !source.contains("data:image")
    );
    let tree = crate::svg::parse(&source).unwrap();
    let mut svg = Pixmap::new(560, 320).unwrap();
    resvg::render(&tree, Transform::identity(), &mut svg.as_mut());
    let expected = draw(&Renderer::new(&scene, &place()), &scene, 0.0);
    let pixel_error: u64 = svg
        .pixels()
        .iter()
        .zip(expected.pixels())
        .map(|(svg, native)| u64::from(svg.red().abs_diff(native.red())))
        .sum();
    assert!(
        pixel_error < mass(&expected) / 20,
        "pixel error {pixel_error}"
    );
}

#[test]
fn new_text_and_linked_text_use_colors_that_contrast_with_the_canvas() {
    let mut scene = Scene::empty("text-colors".into());
    scene.add_layer("shape".into(), "rect");
    scene.add_text("title".into());
    scene.add_linked_text("caption".into(), 0);
    assert_eq!(scene.shapes[1].color, "#1F3D2B");
    assert_eq!(scene.shapes[2].color, "#1F3D2B");
    scene.bg = "#141617".into();
    scene.add_text("dark-title".into());
    scene.add_linked_text("dark-caption".into(), 0);
    assert_eq!(scene.shapes[3].color, "#ffffff");
    assert_eq!(scene.shapes[4].color, "#ffffff");
}

#[test]
fn multiline_selection_uses_animated_leading_and_keeps_single_line_height() {
    let factor = crate::text_render::selection_height_factor;
    let mut scene = scene("Hello Forma");
    assert_eq!(factor(&scene.shapes[0], 0.0), 1.0);
    scene.shapes[0]
        .extra
        .insert("text".into(), "Hello Forma\nsecond line".into());
    assert_eq!(factor(&scene.shapes[0], 0.0), 2.2);
    let lane = TextLane::Style(TextProperty::LineHeight);
    let shape = &mut scene.shapes[0];
    shape.add_lane_key(Lane::Text(lane), 0.0);
    lane.set(shape, 1.0, [200.0, 0.0, 0.0]);
    assert_eq!(factor(shape, 0.0), 2.2);
    assert_eq!(factor(shape, 1.0), 3.0);
}
