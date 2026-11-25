mod common;

use canopy_ui::Place;
use forma_native::compositing::{LayerMask, MaskMode, MaskPose, MaskShape};
use forma_native::effects::{Effect, Kind, Parameter};
use forma_native::keyframes::Lane;
use forma_native::model::Scene;
use forma_native::motion_blur::Settings;
use forma_native::render::{Frames, Renderer};
use forma_native::{animated_svg, svg};
use std::io::Write;
use std::process::{Command, Stdio};
use tiny_skia::Pixmap;

fn place() -> Place {
    let dir = std::env::temp_dir();
    Place {
        space: None,
        projects_dir: dir.clone(),
        data_dir: dir.clone(),
        exports_dir: dir,
    }
}

fn scene() -> Scene {
    let mut scene = Scene::empty("effects".into());
    (scene.width, scene.height) = (128.0, 128.0);
    scene.bg = "#000000".into();
    scene
}

fn rect(scene: &mut Scene, id: &str, color: &str, w: f32, h: f32) -> usize {
    scene.add_shape(id.into(), "rect");
    let index = scene.shapes.len() - 1;
    let shape = &mut scene.shapes[index];
    shape.name = id.into();
    shape.color = color.into();
    (shape.keys[0].w, shape.keys[0].h) = (w, h);
    index
}

fn frame(renderer: &Renderer, scene: &Scene, t: f64) -> Pixmap {
    renderer
        .frame(scene, t, (128, 128), &Frames::new())
        .unwrap()
}

fn rgb(image: &Pixmap, x: u32, y: u32) -> [u8; 3] {
    let p = image.pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue()]
}

fn edit(effect: &mut Effect, p: Parameter, value: f32) {
    effect.edit(p, 0.0, [value, 0.0, 0.0]);
}

fn animated_effect_scene() -> Scene {
    let mut scene = scene();
    scene.duration = 3.0;
    let i = rect(&mut scene, "Subject", "#f0a050", 40.0, 30.0);
    let shape = &mut scene.shapes[i];
    shape.effects = vec![
        Effect::new(1, Kind::WaveWarp),
        Effect::new(2, Kind::GaussianBlur),
    ];
    for (id, parameter, start, end) in [
        (1, Parameter::Speed, -0.5, 2.25),
        (2, Parameter::Radius, 1.0, 14.0),
    ] {
        let lane = Lane::Effect(id, parameter);
        edit(shape.effect_mut(id).unwrap(), parameter, start);
        shape.add_lane_key(lane, 0.0);
        shape.add_lane_key(lane, 3.0);
        shape
            .effect_mut(id)
            .unwrap()
            .edit(parameter, 3.0, [end, 0.0, 0.0]);
    }
    scene
}

#[test]
fn continued_pose_freezes_effects_and_keeps_their_pixels_after_reopening() {
    let mut scene = animated_effect_scene();
    let renderer = Renderer::new(&scene, &place());
    let at = 0.8;
    let later = 1.35;
    let expected = frame(&renderer, &scene, at);
    assert!(frame(&renderer, &scene, later).data() != expected.data());

    let next = scene
        .continue_after(0, None, at, "Next".into(), "Track".into())
        .unwrap();
    let copy = &scene.shapes[next];
    assert_eq!(copy.latest_time(), 1.6);
    for effect in &copy.effects {
        let source = scene.shapes[0].effect(effect.id).unwrap();
        for (&parameter, track) in &effect.tracks {
            assert!(track.keys.is_empty());
            assert_eq!(track.rest, source.values(parameter, at, &copy.ease));
        }
    }
    let reopened = Scene::read(&serde_json::to_vec(&scene).unwrap()).unwrap();
    for (scene, time) in [(&scene, at), (&scene, later), (&reopened, later)] {
        assert!(
            frame(&renderer, scene, time).data() == expected.data(),
            "frame at {time}"
        );
    }
}

#[test]
fn adding_speed_keys_restarts_a_frozen_wave_from_its_captured_phase() {
    let mut scene = animated_effect_scene();
    let at = 0.8;
    let next = scene
        .continue_after(0, None, at, "Next".into(), "Track".into())
        .unwrap();
    let renderer = Renderer::new(&scene, &place());
    let expected = frame(&renderer, &scene, 1.0);
    let shape = &mut scene.shapes[next];
    let speed = Lane::Effect(1, Parameter::Speed);
    shape.add_lane_key(speed, 1.0);
    shape.add_lane_key(speed, 1.5);
    shape
        .effect_mut(1)
        .unwrap()
        .edit(Parameter::Speed, 1.0, [1.0, 0.0, 0.0]);
    shape
        .effect_mut(1)
        .unwrap()
        .edit(Parameter::Speed, 1.5, [2.0, 0.0, 0.0]);
    assert_eq!(frame(&renderer, &scene, 0.9).data(), expected.data());
    assert_eq!(frame(&renderer, &scene, 1.0).data(), expected.data());
    let animated = frame(&renderer, &scene, 1.25);
    assert_ne!(animated.data(), expected.data());
    let reopened = Scene::read(&serde_json::to_vec(&scene).unwrap()).unwrap();
    assert_eq!(frame(&renderer, &reopened, 1.25).data(), animated.data());
}

#[test]
fn adjustment_effects_repeat_the_artboard_edges_in_preview_and_export() {
    let mut scene = scene();
    scene.bg = "#f4ead8".into();
    let index = rect(&mut scene, "Full-frame grade", "#ffffff", 128.0, 128.0);
    scene.shapes[index].kind = "adjustment".into();
    let renderer = Renderer::new(&scene, &place());
    let cream = [244, 234, 216];
    for kind in [
        Kind::GaussianBlur,
        Kind::DirectionalBlur,
        Kind::ZoomBlur,
        Kind::ChromaticAberration,
        Kind::WaveWarp,
    ] {
        scene.shapes[index].effects = vec![Effect::new(1, kind)];
        let exported = frame(&renderer, &scene, 0.0);
        let preview = renderer
            .view(
                &scene,
                0.0,
                ((-13.0, -19.0), 1.0, (180, 180)),
                false,
                &Frames::new(),
            )
            .unwrap();
        for (x, y) in [(0, 0), (127, 0), (0, 64), (127, 127)] {
            assert_eq!(rgb(&exported, x, y), cream, "{kind:?} at {x},{y}");
            assert_eq!(exported.pixel(x, y).unwrap().alpha(), 255, "{kind:?}");
            assert_eq!(
                rgb(&preview, x + 13, y + 19),
                cream,
                "{kind:?} at {x},{y} in preview"
            );
        }
        assert_eq!(
            preview.pixel(12, 19).unwrap().alpha(),
            0,
            "{kind:?} spilled onto pasteboard"
        );
    }

    scene.shapes[index].effects = vec![Effect::new(1, Kind::GaussianBlur)];
    let still = animated_svg::still(&scene, &place(), 0.0).unwrap();
    let tree = svg::parse(&still).unwrap();
    let svg = svg::rasterize(&tree, 128, 128).unwrap();
    assert_eq!(rgb(&svg, 0, 0), cream);
    assert_eq!(svg.pixel(0, 0).unwrap().alpha(), 255);
    scene.shapes[index].effects[0].repeat_edge_pixels = Some(false);
    let feathered = frame(&renderer, &scene, 0.0);
    assert!(feathered.pixel(0, 0).unwrap().alpha() < 240);

    let adjustment = &mut scene.shapes[index];
    adjustment.effects[0].repeat_edge_pixels = None;
    adjustment.keys[0].w = 200.0;
    adjustment.keys[0].h = 200.0;
    adjustment.keys[0].r = 25.0;
    let rotated = frame(&renderer, &scene, 0.0);
    for (x, y) in [(0, 0), (127, 0), (0, 64), (127, 127)] {
        let pixel = rotated.pixel(x, y).unwrap();
        assert!(pixel.alpha() >= 250, "rotated fringe at {x},{y}: {pixel:?}");
    }
}

#[test]
fn full_frame_layer_can_repeat_edges_while_ordinary_shapes_feather_by_default() {
    use base64::Engine;

    let mut scene = scene();
    scene.transparent_background = true;
    let index = rect(&mut scene, "Full frame", "#f4ead8", 128.0, 128.0);
    let mut pixels = Pixmap::new(8, 8).unwrap();
    pixels.fill(tiny_skia::Color::from_rgba8(244, 234, 216, 255));
    let png = base64::engine::general_purpose::STANDARD.encode(pixels.encode_png().unwrap());
    scene.shapes[index].kind = "image".into();
    scene.shapes[index].extra.insert(
        "assetData".into(),
        format!("data:image/png;base64,{png}").into(),
    );
    scene.shapes[index].effects = vec![Effect::new(1, Kind::GaussianBlur)];
    let renderer = Renderer::new(&scene, &place());
    assert!(frame(&renderer, &scene, 0.0).pixel(0, 0).unwrap().alpha() < 240);
    scene.shapes[index].effects[0].repeat_edge_pixels = Some(true);
    let repeated = frame(&renderer, &scene, 0.0);
    assert_eq!(rgb(&repeated, 0, 0), [244, 234, 216]);
    assert_eq!(repeated.pixel(0, 0).unwrap().alpha(), 255);
    let error = animated_svg::animated(&scene, &place()).unwrap_err();
    assert!(error.contains("Full frame") && error.contains("Repeat edge pixels"));
    let error = forma_native::lottie::export(&scene).unwrap_err();
    assert!(error.contains("Full frame") && error.contains("Repeat edge pixels"));

    scene.shapes[index].kind = "rect".into();
    scene.shapes[index].keys[0].w = 40.0;
    scene.shapes[index].keys[0].h = 40.0;
    scene.shapes[index].effects[0].repeat_edge_pixels = None;
    let halo = frame(&renderer, &scene, 0.0);
    assert!((1..255).contains(&halo.pixel(43, 64).unwrap().alpha()));
    scene.shapes[index].effects[0].repeat_edge_pixels = Some(true);
    let bounded = frame(&renderer, &scene, 0.0);
    assert_eq!(bounded.pixel(43, 64).unwrap().alpha(), 0);
}

#[test]
fn onion_skin_freezes_effects_at_the_previous_clip_end() {
    let mut scene = animated_effect_scene();
    let end = 0.8;
    let at = end - 0.001;
    let next = scene
        .continue_after(0, None, end, "Next".into(), "Track".into())
        .unwrap();
    let ghost = scene.onion_skin(next, 1.0).unwrap();
    assert_eq!(ghost.latest_time(), 0.0);
    assert!(
        ghost
            .effects
            .iter()
            .flat_map(|effect| effect.tracks.values())
            .all(|track| track.keys.is_empty())
    );

    // Match the onion skin's opacity when comparing against the source.
    let mut source = scene.clone();
    source.shapes.truncate(1);
    source.shapes[0].keys[0].o = 30.0;
    let renderer = Renderer::new(&source, &place());
    let expected = frame(&renderer, &source, at);
    assert!(frame(&renderer, &source, 0.4).data() != expected.data());
    scene.shapes = vec![ghost];
    for time in [at, 1.0, 2.0] {
        assert!(
            frame(&renderer, &scene, time).data() == expected.data(),
            "ghost at {time}"
        );
    }
}

#[test]
fn masks_feed_effects_and_directional_effects_rotate_with_the_layer() {
    let mut scene = scene();
    let i = rect(&mut scene, "Subject", "#ff0000", 24.0, 24.0);
    let shape = &mut scene.shapes[i];
    shape.keys[0].r = 90.0;
    shape.masks.push(LayerMask {
        id: 1,
        mode: MaskMode::Add,
        shape: MaskShape::Rectangle,
        inverted: false,
        reference_size: [24.0; 2],
        rest: MaskPose::new([-6.0, 0.0], [12.0, 24.0]),
        keys: Vec::new(),
    });
    let mut shadow = Effect::new(1, Kind::DropShadow);
    shadow.edit(Parameter::Color, 0.0, [0.0, 255.0, 0.0]);
    for (parameter, value) in [
        (Parameter::Angle, 0.0),
        (Parameter::Distance, 20.0),
        (Parameter::Softness, 0.0),
        (Parameter::Opacity, 100.0),
    ] {
        edit(&mut shadow, parameter, value);
    }
    shape.effects.push(shadow);
    let renderer = Renderer::new(&scene, &place());
    let result = frame(&renderer, &scene, 0.0);
    assert_eq!(rgb(&result, 64, 59), [255, 0, 0]);
    assert_eq!(rgb(&result, 64, 78), [0, 255, 0]);
    assert_eq!(rgb(&result, 64, 90), [0; 3]);
    assert_eq!(rgb(&result, 84, 59), [0; 3]);
}

#[test]
fn changing_stack_order_and_disabling_effects_updates_the_same_renderer() {
    let mut scene = scene();
    let i = rect(&mut scene, "Subject", "#f0a050", 40.0, 40.0);
    let renderer = Renderer::new(&scene, &place());
    let original = frame(&renderer, &scene, 0.0);
    scene.shapes[i].effects = vec![Effect::new(1, Kind::Invert), Effect::new(2, Kind::Fill)];
    assert_eq!(rgb(&frame(&renderer, &scene, 0.0), 64, 64), [237, 121, 94]);
    scene.shapes[i].effects.swap(0, 1);
    assert_eq!(rgb(&frame(&renderer, &scene, 0.0), 64, 64), [18, 134, 161]);
    for effect in &mut scene.shapes[i].effects {
        effect.enabled = false;
    }
    assert_eq!(frame(&renderer, &scene, 0.0).data(), original.data());
}

#[test]
fn numeric_and_color_lanes_animate_ease_retime_and_shift_with_the_layer() {
    let mut scene = scene();
    let i = rect(&mut scene, "Subject", "#0080ff", 40.0, 40.0);
    let shape = &mut scene.shapes[i];
    shape.effects.push(Effect::new(1, Kind::Fill));
    let amount = Lane::Effect(1, Parameter::Amount);
    let color = Lane::Effect(1, Parameter::Color);
    let fx = shape.effect_mut(1).unwrap();
    edit(fx, Parameter::Amount, 0.0);
    fx.edit(Parameter::Color, 0.0, [0.0, 0.0, 0.0]);
    for lane in [amount, color] {
        shape.add_lane_key(lane, 0.0);
        shape.add_lane_key(lane, 2.0);
        let mut first = shape.lane_key(lane, 0.0).unwrap();
        first.set_easing("ease-in", None);
        shape.put_lane_key(0.0, first);
    }
    let fx = shape.effect_mut(1).unwrap();
    fx.edit(Parameter::Amount, 2.0, [100.0, 0.0, 0.0]);
    fx.edit(Parameter::Color, 2.0, [255.0, 0.0, 0.0]);
    assert!(shape.retime_lane_keys(amount, &[2.0], -1.0));
    assert_eq!(
        shape
            .lane_keys(amount)
            .iter()
            .map(|k| k.time())
            .collect::<Vec<_>>(),
        [0.0, 1.0]
    );
    assert_eq!(scene.shift_layer(i, 1.0), 1.0);
    assert_eq!(
        scene.shapes[i].lane_key(color, 3.0).unwrap().value(0),
        255.0
    );
    assert_eq!(
        scene.shapes[i].lane_key(amount, 2.0).unwrap().value(0),
        100.0
    );
    assert_eq!(
        scene.shapes[i]
            .lane_key(amount, 1.0)
            .unwrap()
            .easing("smooth")
            .0,
        "ease-in"
    );
    assert!((amount.value(&scene.shapes[i], 1.5, 0) - 31.54).abs() < 0.2);
    let renderer = Renderer::new(&scene, &place());
    assert_eq!(rgb(&frame(&renderer, &scene, 1.0), 64, 64), [0, 128, 255]);
    assert_eq!(rgb(&frame(&renderer, &scene, 3.0), 64, 64), [255, 0, 0]);
    assert_ne!(rgb(&frame(&renderer, &scene, 1.5), 64, 64), [0, 128, 255]);
}

#[test]
fn adjustment_is_clipped_by_bounds_and_mask_and_respects_order_opacity_and_time() {
    let mut scene = scene();
    rect(&mut scene, "Below", "#ff0000", 120.0, 105.0);
    scene.add_shape("adjust".into(), "adjustment");
    let adjustment = &mut scene.shapes[1];
    (adjustment.keys[0].w, adjustment.keys[0].h) = (96.0, 80.0);
    adjustment.keys[0].o = 50.0;
    adjustment.in_time = Some(0.5);
    adjustment.out_time = Some(1.5);
    adjustment.effects.push(Effect::new(1, Kind::Invert));
    adjustment.masks.push(LayerMask {
        id: 1,
        mode: MaskMode::Add,
        shape: MaskShape::Rectangle,
        inverted: false,
        reference_size: [96.0, 80.0],
        rest: MaskPose::new([-30.0, 0.0], [70.0, 128.0]),
        keys: Vec::new(),
    });
    let above = rect(&mut scene, "Above", "#00ff00", 20.0, 16.0);
    scene.shapes[above].keys[0].x = 42.0;
    let renderer = Renderer::new(&scene, &place());
    let adjusted = frame(&renderer, &scene, 1.0);
    assert_eq!(rgb(&adjusted, 22, 64), [127, 128, 128]);
    assert_eq!(rgb(&adjusted, 10, 64), [255, 0, 0]);
    assert_eq!(rgb(&adjusted, 22, 20), [255, 0, 0]);
    assert_eq!(rgb(&adjusted, 86, 64), [255, 0, 0]);
    assert_eq!(rgb(&adjusted, 42, 64), [0, 255, 0]);
    for time in [0.25, 1.5] {
        assert_eq!(rgb(&frame(&renderer, &scene, time), 22, 64), [255, 0, 0]);
    }
}

#[test]
fn shutter_smears_only_switched_layers_and_preview_is_opt_in() {
    let mut scene = scene();
    scene.motion_blur = Some(Settings {
        shutter_angle: 720.0,
        samples: 16,
    });
    let i = rect(&mut scene, "Fast", "#ffffff", 8.0, 8.0);
    let shape = &mut scene.shapes[i];
    shape.keys[0].x = 32.0;
    shape.keys[0].y = 40.0;
    shape.ease = "linear".into();
    shape.motion_blur = true;
    let mut second = shape.keys[0].clone();
    (second.t, second.x) = (0.1, 96.0);
    shape.keys.push(second);
    let j = rect(&mut scene, "Sharp", "#ff0000", 8.0, 8.0);
    scene.shapes[j].keys = scene.shapes[i].keys.clone();
    for key in &mut scene.shapes[j].keys {
        key.y = 80.0;
    }
    scene.shapes[j].ease = "linear".into();
    let renderer = Renderer::new(&scene, &place());
    let blurred = frame(&renderer, &scene, 0.05);
    assert!(rgb(&blurred, 49, 40)[0] > 0 && rgb(&blurred, 79, 40)[0] > 0);
    assert_eq!(rgb(&blurred, 64, 24), [0; 3]);
    assert_eq!(rgb(&blurred, 49, 80), [0; 3]);
    assert_eq!(rgb(&blurred, 64, 80), [255, 0, 0]);
    let sharp = renderer.preview(&scene, 0.05, 1.0, &Frames::new()).unwrap();
    assert_eq!(rgb(&sharp, 49, 40), [0; 3]);
    assert_eq!(rgb(&sharp, 64, 40), [255; 3]);
    scene.motion_blur_preview = true;
    let shown = renderer.preview(&scene, 0.05, 1.0, &Frames::new()).unwrap();
    assert_eq!(shown.data(), blurred.data());
    let rest = frame(&renderer, &scene, 0.5);
    scene.motion_blur.as_mut().unwrap().shutter_angle = 0.0;
    assert_eq!(rest.data(), frame(&renderer, &scene, 0.5).data());
    assert_eq!(rgb(&frame(&renderer, &scene, 0.05), 64, 40), [255; 3]);
}

#[test]
fn effect_scrubbing_at_full_hd() {
    let mut scene = scene();
    (scene.width, scene.height) = (1920.0, 1080.0);
    for i in 0..30 {
        let layer = rect(&mut scene, &i.to_string(), "#b48224", 280.0, 200.0);
        let shape = &mut scene.shapes[layer];
        shape.keys[0].x = 160.0 + (i % 6) as f32 * 280.0;
        shape.keys[0].y = 100.0 + (i / 6) as f32 * 200.0;
        let mut last = shape.keys[0].clone();
        (last.t, last.x) = (2.0, last.x + 35.0);
        shape.keys.push(last);
    }
    scene.shapes[13].effects = vec![
        Effect::new(1, Kind::GaussianBlur),
        Effect::new(2, Kind::Fill),
    ];
    scene.shapes[14].effects = vec![Effect::new(1, Kind::Glow), Effect::new(2, Kind::Noise)];
    let renderer = Renderer::new(&scene, &place());
    let start = std::time::Instant::now();
    for i in 0..30 {
        let time = f64::from(i) / 30.0;
        let shot = renderer.preview(&scene, time, 1.0, &Frames::new()).unwrap();
        assert_eq!((shot.width(), shot.height()), (1920, 1080));
    }
    eprintln!(
        "1080p with 30 layers and four effects: {:.1} ms/frame",
        start.elapsed().as_secs_f64() * 1000.0 / 30.0
    );
}

#[test]
fn animated_svg_emits_ordered_equivalents_and_names_features_it_cannot_export() {
    let mut scene = scene();
    let i = rect(&mut scene, "Subject", "#ff0000", 24.0, 24.0);
    scene.shapes[i].effects = [
        Kind::GaussianBlur,
        Kind::DropShadow,
        Kind::Invert,
        Kind::TurbulentDisplace,
        Kind::Levels,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, kind)| Effect::new(i as u64, kind))
    .collect();
    scene.shapes[i].add_lane_key(Lane::Effect(0, Parameter::Radius), 0.0);
    scene.shapes[i].add_lane_key(Lane::Effect(0, Parameter::Radius), 1.0);
    scene.shapes[i]
        .effect_mut(0)
        .unwrap()
        .edit(Parameter::Radius, 1.0, [12.0, 0.0, 0.0]);
    let code = animated_svg::animated(&scene, &place()).unwrap();
    assert!(svg::parse(&code).is_ok());
    assert!(code.contains(r#"<feGaussianBlur in="SourceGraphic" result="effect-0""#));
    assert!(code.contains(r#"<animate attributeName="stdDeviation""#));
    assert!(code.contains(r#"<feMergeNode in="effect-0"/>"#));
    assert!(code.contains(r#"<feColorMatrix in="effect-1""#));
    assert!(code.contains(r#"<feDisplacementMap in="effect-2""#));
    assert!(code.contains(r#"<feComponentTransfer in="effect-3""#));

    let mut unsupported = Effect::new(10, Kind::Twirl);
    unsupported.enabled = false;
    scene.shapes[i].effects.push(unsupported);
    assert!(animated_svg::animated(&scene, &place()).is_ok());
    scene.shapes[i].effect_mut(10).unwrap().enabled = true;
    let error = animated_svg::animated(&scene, &place()).unwrap_err();
    assert!(
        error.contains("Subject") && error.contains("Twirl"),
        "{error}"
    );
    scene.shapes[i].effect_mut(10).unwrap().enabled = false;
    scene.shapes[i].motion_blur = true;
    let error = animated_svg::animated(&scene, &place()).unwrap_err();
    assert!(
        error.contains("Subject") && error.contains("motion blur"),
        "{error}"
    );
    scene.shapes[i].motion_blur = false;
    scene.add_shape("adjust".into(), "adjustment");
    assert!(
        animated_svg::animated(&scene, &place())
            .unwrap_err()
            .contains("adjustment layers")
    );
}

#[test]
fn animated_svg_and_raster_agree_for_local_shadow_and_invert_at_frame_zero() {
    let mut scene = scene();
    let i = rect(&mut scene, "Subject", "#ff0000", 24.0, 24.0);
    let mut shadow = Effect::new(1, Kind::DropShadow);
    for (parameter, value) in [
        (Parameter::Angle, 0.0),
        (Parameter::Distance, 20.0),
        (Parameter::Softness, 0.0),
        (Parameter::Opacity, 100.0),
    ] {
        edit(&mut shadow, parameter, value);
    }
    scene.shapes[i].effects = vec![shadow, Effect::new(2, Kind::Invert)];
    let code = animated_svg::animated(&scene, &place()).unwrap();
    let vector = svg::rasterize(&svg::parse(&code).unwrap(), 128, 128).unwrap();
    let raster = frame(&Renderer::new(&scene, &place()), &scene, 0.0);
    for (x, y, expected) in [
        (64, 64, [0, 255, 255]),
        (84, 64, [255; 3]),
        (64, 84, [0; 3]),
    ] {
        assert_eq!(rgb(&vector, x, y), expected);
        assert_eq!(rgb(&raster, x, y), expected);
    }
}

#[test]
fn phase_five_projects_round_trip_and_pass_electrons_actual_validator() {
    let mut scene = scene();
    let i = rect(&mut scene, "Subject", "#8f12aa", 40.0, 40.0);
    let before = serde_json::to_value(&scene).unwrap();
    assert!(before.get("motionBlur").is_none() && before.get("motionBlurPreview").is_none());
    assert!(
        before["shapes"][i].get("effects").is_none()
            && before["shapes"][i].get("motionBlur").is_none()
    );
    scene.motion_blur = Some(Settings {
        shutter_angle: 500.0,
        samples: 32,
    });
    scene.motion_blur_preview = true;
    scene.shapes[i].motion_blur = true;
    scene.shapes[i].extra.insert("blur".into(), 42.into());
    scene.shapes[i].effects = Kind::ALL
        .iter()
        .enumerate()
        .map(|(i, &kind)| Effect::new(i as u64, kind))
        .collect();
    scene.shapes[i].effects[0].repeat_edge_pixels = Some(true);
    scene.shapes[i].add_lane_key(Lane::Effect(9, Parameter::WhiteColor), 0.5);
    scene.add_shape("grade".into(), "adjustment");
    scene.shapes[1].effects.push(Effect::new(1, Kind::Tint));
    let mut blur = Effect::new(2, Kind::DirectionalBlur);
    blur.repeat_edge_pixels = Some(false);
    scene.shapes[1].effects.push(blur);
    let bytes = serde_json::to_vec(&scene).unwrap();
    assert_eq!(Scene::read(&bytes).unwrap(), scene);
    let project: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(project["shapes"][0]["effects"][0]["repeatEdgePixels"], true);
    assert!(
        project["shapes"][0]["effects"][1]
            .get("repeatEdgePixels")
            .is_none()
    );
    assert_eq!(
        project["shapes"][1]["effects"][1]["repeatEdgePixels"],
        false
    );
    common::assert_electron_keys(&project);
    let mut node = Command::new("node")
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
        .args(["--input-type=module", "-e", r#"
            import {readFileSync} from 'node:fs';
            import {valid, usableProject} from './src/model/validate.js';
            const project = JSON.parse(readFileSync(0, 'utf8'));
            if (!valid(project) || !usableProject(project) || project.shapes[1].type !== 'svg') process.exit(1);
        "#])
        .stdin(Stdio::piped()).spawn().unwrap();
    node.stdin.take().unwrap().write_all(&bytes).unwrap();
    assert!(node.wait().unwrap().success());
}
