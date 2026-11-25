mod common;

use forma_native::{
    animated_svg, expressions,
    keyframes::{Lane, Property},
    lottie,
    model::{Scene, Shape},
    render::{Frames, Renderer},
    spatial::{Property as SpatialProperty, Spatial},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn place() -> canopy_ui::Place {
    let root = std::env::temp_dir();
    canopy_ui::Place {
        space: None,
        data_dir: root.clone(),
        projects_dir: root.clone(),
        exports_dir: root,
    }
}

fn scene() -> Scene {
    let mut scene = Scene::empty("phase-seven".into());
    (scene.width, scene.height, scene.duration) = (320.0, 240.0, 2.0);
    scene.add_shape("art".into(), "rect");
    let art = &mut scene.shapes[0];
    art.name = "Foreground card".into();
    art.color = "#de512a".into();
    (art.keys[0].x, art.keys[0].y, art.keys[0].w, art.keys[0].h) = (160.0, 120.0, 130.0, 100.0);
    scene
}

const CLIP_END: f64 = 0.8;
const LATER: f64 = 1.35;
const POSITION: Lane = Lane::Pose(Property::Position);
const ROTATION: Lane = Lane::Pose(Property::Rotation);

fn set_spatial_key(shape: &mut Shape, property: SpatialProperty, at: f64, value: f32) {
    let lane = Lane::Spatial(property);
    shape.add_lane_key(lane, at);
    let mut key = shape.lane_key(lane, at).unwrap();
    key.set_value(0, value, false);
    shape.put_lane_key(at, key);
}

fn spatial_animation() -> Scene {
    let mut scene = scene();
    let art = &mut scene.shapes[0];
    art.spatial = Some(Spatial::default());
    for (property, start, end) in [
        (SpatialProperty::Depth, -55.0, 90.0),
        (SpatialProperty::RotationY, 5.0, 75.0),
    ] {
        set_spatial_key(art, property, 0.0, start);
        set_spatial_key(art, property, 2.0, end);
    }
    art.set_expression(
        Lane::Spatial(SpatialProperty::RotationX),
        Some("time * 25".into()),
    );
    scene
}

fn expression_animation() -> Scene {
    let mut scene = scene();
    let art = &mut scene.shapes[0];
    art.set_expression(POSITION, Some("wiggle(3, 40)".into()));
    art.set_expression(ROTATION, Some("time * 90".into()));
    scene
}

fn frame(renderer: &Renderer, scene: &Scene, at: f64) -> tiny_skia::Pixmap {
    renderer
        .frame(scene, at, (320, 240), &Frames::new())
        .unwrap()
}

fn assert_frozen(shape: &Shape, latest: f64) {
    assert!(shape.expressions.is_empty());
    assert!(
        shape
            .spatial
            .iter()
            .flat_map(|spatial| spatial.tracks.values())
            .all(|track| track.keys.is_empty())
    );
    assert_eq!(shape.latest_time(), latest);
}

fn continue_pose(scene: &mut Scene) -> usize {
    scene
        .continue_after(0, None, CLIP_END, "Next".into(), "Track".into())
        .unwrap()
}

fn assert_continued_pose(mut scene: Scene) {
    let renderer = Renderer::new(&scene, &place());
    let expected = frame(&renderer, &scene, CLIP_END);
    assert!(frame(&renderer, &scene, LATER).data() != expected.data());

    let next = continue_pose(&mut scene);
    assert_frozen(&scene.shapes[next], 1.6);
    let bytes = serde_json::to_vec(&scene).unwrap();
    common::assert_electron_keys(&serde_json::from_slice(&bytes).unwrap());
    let reopened = Scene::read(&bytes).unwrap();
    assert_frozen(&reopened.shapes[next], 1.6);
    for (scene, at) in [(&scene, CLIP_END), (&scene, LATER), (&reopened, LATER)] {
        assert!(
            frame(&renderer, scene, at).data() == expected.data(),
            "continued frame at {at}"
        );
    }
}

fn assert_ghost(mut scene: Scene) {
    let mut source = scene.clone();
    source.shapes[0].keys[0].o = 30.0;
    let at = CLIP_END - 0.001;
    let renderer = Renderer::new(&source, &place());
    let expected = frame(&renderer, &source, at);
    assert!(frame(&renderer, &source, LATER).data() != expected.data());

    let next = continue_pose(&mut scene);
    let ghost = scene.onion_skin(next, LATER).unwrap();
    assert_frozen(&ghost, 0.0);
    scene.shapes = vec![ghost];
    for at in [at, LATER, 2.0] {
        assert!(
            frame(&renderer, &scene, at).data() == expected.data(),
            "ghost frame at {at}"
        );
    }
}

#[test]
fn continued_pose_freezes_animated_depth_and_y_rotation() {
    assert_continued_pose(spatial_animation());
}

#[test]
fn onion_skin_freezes_animated_depth_and_y_rotation() {
    assert_ghost(spatial_animation());
}

#[test]
fn continued_pose_freezes_wiggle_and_rotation_expressions() {
    assert_continued_pose(expression_animation());
}

#[test]
fn onion_skin_freezes_wiggle_and_rotation_expressions() {
    assert_ghost(expression_animation());
}

#[test]
fn the_onion_skin_keeps_links_to_the_live_source_layer() {
    let mut scene = expression_animation();
    let next = continue_pose(&mut scene);
    scene.add_shape("follower".into(), "rect");
    let follower = &mut scene.shapes[2];
    follower.set_expression(ROTATION, Some(r#"layerId("art").rotation + index"#.into()));
    follower.set_expression(
        POSITION,
        Some(r#"layer("Foreground card").position"#.into()),
    );
    let expected =
        [POSITION, ROTATION].map(|lane| expressions::evaluate(&scene, 2, lane, LATER).unwrap());

    let ghost = scene.onion_skin(next, LATER).unwrap();
    scene.shapes.insert(0, ghost);
    for (lane, value) in [POSITION, ROTATION].into_iter().zip(expected) {
        assert_eq!(expressions::evaluate(&scene, 3, lane, LATER), Ok(value));
    }
}

#[test]
fn continued_pose_samples_linked_lanes_before_rippling_the_track() {
    let mut scene = scene();
    scene.add_shape("driver".into(), "rect");
    let driver = &mut scene.shapes[1];
    driver.in_time = Some(CLIP_END);
    driver.keys[0].x = -1000.0;
    driver.keys[0].r = 10.0;
    driver.add_lane_key(ROTATION, 2.0);
    let mut key = driver.lane_key(ROTATION, 2.0).unwrap();
    key.set_value(0, 80.0, false);
    driver.put_lane_key(2.0, key);
    for shape in &mut scene.shapes {
        shape.extra.insert("trackId".into(), "Track".into());
    }
    scene.shapes[0].set_expression(ROTATION, Some(r#"layerId("driver").rotation"#.into()));
    let renderer = Renderer::new(&scene, &place());
    let expected = frame(&renderer, &scene, CLIP_END);

    let next = continue_pose(&mut scene);
    assert_eq!(scene.shapes[2].in_time, Some(1.6));
    assert_frozen(&scene.shapes[next], 1.6);
    assert!(frame(&renderer, &scene, LATER).data() == expected.data());
}

#[test]
fn pasted_pose_freezes_the_clipboard_templates_expressions_at_its_start() {
    let mut scene = scene();
    scene.add_shape("template".into(), "rect");
    let template = &mut scene.shapes[1];
    template.in_time = Some(0.3);
    template.set_expression(Lane::Color, Some("[time * 100, 0, 0]".into()));
    let clipboard = template.clone();
    template.set_expression(Lane::Color, Some("[0, 100, 0]".into()));

    let next = scene
        .continue_after(0, Some(&clipboard), CLIP_END, "Next".into(), "Track".into())
        .unwrap();
    let copy = &scene.shapes[next];
    assert_frozen(copy, 1.6);
    assert_eq!(copy.color_at(LATER), "#1e0000");
}

#[test]
fn camera_copies_freeze_their_field_of_view_and_focus_expression() {
    let mut scene = scene();
    scene.shapes[0].spatial = Some(Spatial::default());
    scene.add_camera("camera".into()).unwrap();
    let camera = &mut scene.shapes[1];
    let fov = SpatialProperty::FieldOfView;
    set_spatial_key(camera, fov, 0.0, 45.0);
    set_spatial_key(camera, fov, 2.0, 90.0);
    let focus = Lane::Spatial(SpatialProperty::FocusDistance);
    camera.set_expression(focus, Some("180 + time * 40".into()));
    let renderer = Renderer::new(&scene, &place());
    let expected_copy = frame(&renderer, &scene, CLIP_END);
    let ghost_time = CLIP_END - 0.001;
    let expected_ghost = frame(&renderer, &scene, ghost_time);
    assert!(frame(&renderer, &scene, LATER).data() != expected_copy.data());

    let next = scene
        .continue_after(1, None, CLIP_END, "Next".into(), "Track".into())
        .unwrap();
    let copy = &scene.shapes[next];
    assert_frozen(copy, 1.6);
    assert_eq!(
        focus.value(copy, LATER, 0),
        (180.0 + CLIP_END * 40.0) as f32
    );
    for at in [CLIP_END, LATER] {
        assert!(frame(&renderer, &scene, at).data() == expected_copy.data());
    }

    let ghost = scene.onion_skin(next, LATER).unwrap();
    assert_frozen(&ghost, 0.0);
    assert_eq!(
        focus.value(&ghost, LATER, 0),
        (180.0 + ghost_time * 40.0) as f32
    );
    scene.shapes.truncate(1);
    scene.shapes.push(ghost);
    for at in [ghost_time, LATER] {
        assert!(frame(&renderer, &scene, at).data() == expected_ghost.data());
    }
}

#[test]
fn native_round_trip_and_electron_project_validators_accept_phase_seven_fields() {
    let mut scene = scene();
    scene.add_camera("camera".into()).unwrap();
    let art = &mut scene.shapes[0];
    art.spatial = Some(Spatial::default());
    art.set_expression(
        Lane::Pose(Property::Rotation),
        Some("value + time * 30".into()),
    );
    let lane = Lane::Spatial(SpatialProperty::Orientation);
    art.add_lane_key(lane, 0.5);
    let mut key = art.lane_key(lane, 0.5).unwrap();
    key.set_value(2, 30.0, false);
    key.set_easing("custom", Some([0.25, -0.3, 0.8, 1.0]));
    art.put_lane_key(0.5, key);
    scene.shift_layer(0, 0.2);
    assert!(scene.shapes[0].lane_key(lane, 0.7).is_some());
    scene.set_parent(1, Some(0), 0.0).unwrap();

    let bytes = serde_json::to_vec(&scene).unwrap();
    let project: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    common::assert_electron_keys(&project);
    assert_eq!(project["shapes"][1]["type"], "svg");
    assert_eq!(Scene::read(&bytes).unwrap(), scene);
    let mut validator = Command::new("node")
        .current_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap(),
        )
        .args([
            "--input-type=module",
            "-e",
            r#"
            import { readFileSync } from 'node:fs';
            import { valid, usableProject } from './src/model/validate.js';
            const project = JSON.parse(readFileSync(0, 'utf8'));
            process.exitCode = valid(project) && usableProject(project) ? 0 : 1;
        "#,
        ])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    validator.stdin.take().unwrap().write_all(&bytes).unwrap();
    assert!(validator.wait().unwrap().success());
}

#[test]
fn painter_depth_sort_has_the_expected_front_layer_in_raster_exports() {
    let mut scene = scene();
    scene.shapes[0].spatial = Some(Spatial::default());
    scene.add_shape("back".into(), "rect");
    let back = &mut scene.shapes[1];
    back.spatial = Some(Spatial::default());
    back.color = "#074faf".into();
    (
        back.keys[0].x,
        back.keys[0].y,
        back.keys[0].w,
        back.keys[0].h,
    ) = (160.0, 120.0, 130.0, 100.0);
    let depth = Lane::Spatial(SpatialProperty::Depth);
    back.add_lane_key(depth, 0.0);
    let mut key = back.lane_key(depth, 0.0).unwrap();
    key.set_value(0, 100.0, false);
    back.put_lane_key(0.0, key.clone());
    let renderer = Renderer::new(&scene, &place());
    let first = renderer
        .frame(&scene, 0.0, (320, 240), &Frames::new())
        .unwrap();
    assert_eq!(&first.pixel(160, 120).unwrap().demultiply().red(), &0xde);
    key.set_value(0, -70.0, false);
    scene.shapes[1].put_lane_key(0.0, key);
    let second = renderer
        .frame(&scene, 0.0, (320, 240), &Frames::new())
        .unwrap();
    assert_eq!(&second.pixel(160, 120).unwrap().demultiply().red(), &0x07);
}

#[test]
fn depth_of_field_softens_raster_alpha_outside_the_focal_plane() {
    let mut scene = scene();
    scene.transparent_background = true;
    scene.shapes[0].spatial = Some(Spatial::default());
    let depth = Lane::Spatial(SpatialProperty::Depth);
    scene.shapes[0].add_lane_key(depth, 0.0);
    let mut key = scene.shapes[0].lane_key(depth, 0.0).unwrap();
    key.set_value(0, 110.0, false);
    scene.shapes[0].put_lane_key(0.0, key);
    scene.add_camera("camera".into()).unwrap();
    let renderer = Renderer::new(&scene, &place());
    let frames = Frames::new();
    let crisp = renderer.frame(&scene, 0.0, (320, 240), &frames).unwrap();
    scene.shapes[1].spatial.as_mut().unwrap().depth_of_field = true;
    let blurred = renderer.frame(&scene, 0.0, (320, 240), &frames).unwrap();
    let alpha_near_edge = |pixmap: &tiny_skia::Pixmap| {
        (1..75)
            .filter(|&x| {
                let a = pixmap.pixel(160 + x, 120).unwrap().alpha();
                a > 0 && a < 255
            })
            .count()
    };
    assert!(alpha_near_edge(&blurred) > alpha_near_edge(&crisp) + 3);
    assert_eq!(blurred.pixel(160, 120), crisp.pixel(160, 120));
}

#[test]
fn half_quality_is_close_to_full_downscaled_and_preview_matches_export() {
    let mut scene = scene();
    scene.add_camera("camera".into()).unwrap();
    let art = &mut scene.shapes[0];
    art.spatial = Some(Spatial::default());
    art.set_expression(
        Lane::Spatial(SpatialProperty::RotationY),
        Some("time * 70".into()),
    );
    let renderer = Renderer::new(&scene, &place());
    let frames = Frames::new();
    let full = renderer
        .view(&scene, 0.57, ((0.0, 0.0), 1.0, (320, 240)), true, &frames)
        .unwrap();
    let half = renderer
        .view(&scene, 0.57, ((0.0, 0.0), 0.5, (160, 120)), true, &frames)
        .unwrap();
    let export = renderer.frame(&scene, 0.57, (320, 240), &frames).unwrap();
    assert_eq!(full.data(), export.data());
    let full = image::RgbaImage::from_raw(320, 240, full.data().to_vec()).unwrap();
    let down = image::imageops::resize(&full, 160, 120, image::imageops::FilterType::Triangle);
    let mae = down
        .as_raw()
        .iter()
        .zip(half.data())
        .map(|(&a, &b)| u64::from(a.abs_diff(b)))
        .sum::<u64>() as f64
        / half.data().len() as f64;
    assert!(mae < 0.9, "mean difference {mae}");
}

#[test]
fn svg_and_lottie_bake_2d_expressions_and_name_the_first_3d_layer() {
    let mut scene = scene();
    let lane = Lane::Pose(Property::Rotation);
    scene.shapes[0].set_expression(lane, Some("time * 45".into()));
    let svg = animated_svg::animated(&scene, &place()).unwrap();
    assert!(svg.contains("attributeName=\"transform\"") && svg.contains("type=\"rotate\""));
    let json: serde_json::Value = serde_json::from_slice(&lottie::export(&scene).unwrap()).unwrap();
    let rotation = json["layers"][0]["ks"]["r"]["k"].as_array().unwrap();
    let sample = rotation
        .iter()
        .find(|key| key["t"] == serde_json::json!(15.0))
        .unwrap();
    assert_eq!(sample["s"][0], serde_json::json!(22.5));
    scene.shapes[0].spatial = Some(Spatial::default());
    for error in [
        animated_svg::animated(&scene, &place()).unwrap_err(),
        lottie::export(&scene).unwrap_err(),
    ] {
        assert!(
            error.contains("Foreground card") && error.contains("3D"),
            "{error}"
        );
    }
}
