use canopy_ui::Place;
use forma_native::keyframes::{Lane, LaneKey, Property};
use forma_native::model::{Pose, Scene};
use forma_native::paths::{Contour, PathData, Vertex};
use forma_native::render::{Frames, Renderer};
use forma_native::shape_style::{
    Cap, Dash, Gradient, Join, PaintMode, PaintStyle, PaintTarget, Stop, StrokeStyle, Trim,
    VectorKey, VectorLayer, VectorProperty, VectorValue,
};
use forma_native::{animated_svg, archive, svg};
use std::io::Write;
use std::process::{Command, Stdio};
use tiny_skia::{Pixmap, Transform};

fn place() -> Place {
    let root = std::env::temp_dir();
    Place {
        space: None,
        projects_dir: root.clone(),
        data_dir: root.clone(),
        exports_dir: root,
    }
}

fn scene() -> Scene {
    let mut scene = Scene::empty("00000000-0000-0000-0000-000000000003".into());
    scene.width = 120.0;
    scene.height = 100.0;
    scene.duration = 2.0;
    scene.bg = "#000000".into();
    scene
}

fn contour(points: &[[f32; 2]], closed: bool) -> Contour {
    Contour {
        points: points.iter().copied().map(Vertex::corner).collect(),
        closed,
    }
}

fn path(points: &[[f32; 2]], closed: bool) -> PathData {
    PathData {
        contours: vec![contour(points, closed)],
    }
}

fn draw(scene: &Scene, time: f64) -> Pixmap {
    Renderer::new(scene, &place())
        .frame(
            scene,
            time,
            (scene.width as u32, scene.height as u32),
            &Frames::new(),
        )
        .unwrap()
}

fn add_path<'a>(scene: &'a mut Scene, points: &[[f32; 2]], closed: bool) -> &'a mut VectorLayer {
    scene.add_shape("ink".into(), "path");
    let layer = scene.shapes.last_mut().unwrap();
    layer.keys[0].w = 80.0;
    layer.keys[0].h = 80.0;
    layer.ease = "linear".into();
    layer.vector.insert(VectorLayer {
        path: Some(path(points, closed)),
        fill: Some(PaintStyle::new(PaintMode::None)),
        stroke: Some(StrokeStyle {
            width: 6.0,
            ..StrokeStyle::default()
        }),
        ..VectorLayer::default()
    })
}

fn rgb(image: &Pixmap, x: u32, y: u32) -> [u8; 3] {
    let p = image.pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue()]
}

#[test]
fn primitive_conversion_is_pixel_identical_even_when_the_size_changes() {
    for kind in ["rect", "circle", "triangle", "diamond", "arrow"] {
        let mut scene = scene();
        scene.add_shape("primitive".into(), kind);
        let layer = &mut scene.shapes[0];
        let pose = Pose {
            w: 90.0,
            h: 41.0,
            r: 23.0,
            ..Pose::from(&layer.keys[0])
        };
        layer.keys[0].w = 42.0;
        layer.keys[0].h = 65.0;
        layer.key_property(1.0, Property::Size, pose);
        layer.key_property(1.0, Property::Rotation, pose);
        let before: Vec<_> = [0.0, 0.37, 1.0]
            .into_iter()
            .map(|t| draw(&scene, t))
            .collect();
        assert!(scene.shapes[0].convert_to_path([42.0, 65.0]));
        assert_eq!(scene.shapes[0].kind, "path");
        assert!(scene.shapes[0].path_at(0.0, [42.0, 65.0]).unwrap().valid());
        for (t, pixels) in [0.0, 0.37, 1.0].into_iter().zip(before) {
            assert_eq!(
                draw(&scene, t).data(),
                pixels.data(),
                "conversion of {kind} at {t}"
            );
        }
    }
}

#[test]
fn linear_and_radial_gradients_sample_colors_and_stop_opacity() {
    let mut scene = scene();
    scene.add_shape("gradient".into(), "rect");
    let layer = &mut scene.shapes[0];
    layer.keys[0].w = 80.0;
    layer.keys[0].h = 80.0;
    let gradient = Gradient {
        ends: [-0.5, 0.0, 0.5, 0.0],
        stops: vec![
            Stop {
                offset: 0.0,
                rgba: [255.0, 0.0, 0.0, 100.0],
            },
            Stop {
                offset: 100.0,
                rgba: [0.0, 0.0, 255.0, 50.0],
            },
        ],
    };
    layer.vector = Some(VectorLayer {
        fill: Some(PaintStyle {
            mode: PaintMode::Linear,
            gradient: Some(gradient),
        }),
        ..VectorLayer::default()
    });
    let linear = draw(&scene, 0.0);
    assert!(rgb(&linear, 22, 50)[0] >= 240);
    let mid = rgb(&linear, 59, 50);
    assert!(
        (90..=107).contains(&mid[0]) && (90..=107).contains(&mid[2]),
        "{mid:?}"
    );
    assert!((121..=132).contains(&rgb(&linear, 98, 50)[2]));
    let paint = scene.shapes[0]
        .vector
        .as_mut()
        .unwrap()
        .fill
        .as_mut()
        .unwrap();
    paint.mode = PaintMode::Radial;
    paint.gradient.as_mut().unwrap().ends = [0.0, 0.0, 0.5, 0.0];
    let radial = draw(&scene, 0.0);
    assert!(rgb(&radial, 60, 50)[0] >= 240);
    assert!(rgb(&radial, 32, 50)[2] >= 100);
    assert_eq!(rgb(&radial, 32, 50), rgb(&radial, 87, 50));
}

#[test]
fn stroke_caps_joins_dashes_and_animated_offset_change_the_pixels() {
    let mut scene = scene();
    add_path(&mut scene, &[[-0.5, 0.0], [0.5, 0.0]], false);
    let set_cap = |scene: &mut Scene, cap| {
        scene.shapes[0]
            .vector
            .as_mut()
            .unwrap()
            .stroke
            .as_mut()
            .unwrap()
            .cap = cap;
    };
    let butt = draw(&scene, 0.0);
    set_cap(&mut scene, Cap::Round);
    let round = draw(&scene, 0.0);
    set_cap(&mut scene, Cap::Square);
    let square = draw(&scene, 0.0);
    assert_eq!(rgb(&butt, 17, 50), [0; 3]);
    assert!(rgb(&round, 17, 50)[0] > 100);
    assert!(rgb(&square, 17, 47)[0] > rgb(&round, 17, 47)[0]);

    let vector = scene.shapes[0].vector.as_mut().unwrap();
    vector.stroke.as_mut().unwrap().cap = Cap::Butt;
    vector.stroke.as_mut().unwrap().dash = Some(Dash {
        length: 10.0,
        gap: 10.0,
        offset: 0.0,
    });
    number_key(vector, VectorProperty::DashOffset, 0.0, 0.0);
    number_key(vector, VectorProperty::DashOffset, 1.0, 10.0);
    let dashes = draw(&scene, 0.0);
    let shifted = draw(&scene, 1.0);
    assert_eq!(rgb(&dashes, 24, 50), [255; 3]);
    assert_eq!(rgb(&dashes, 34, 50), [0; 3]);
    assert_eq!(rgb(&shifted, 24, 50), [0; 3]);
    assert_eq!(rgb(&shifted, 34, 50), [255; 3]);

    scene.shapes.clear();
    add_path(&mut scene, &[[-0.4, 0.3], [0.0, -0.3], [0.4, 0.3]], false);
    let render_join = |scene: &mut Scene, join| {
        let stroke = scene.shapes[0]
            .vector
            .as_mut()
            .unwrap()
            .stroke
            .as_mut()
            .unwrap();
        stroke.width = 20.0;
        stroke.join = join;
        draw(scene, 0.0)
    };
    let miter = render_join(&mut scene, Join::Miter);
    let round = render_join(&mut scene, Join::Round);
    let bevel = render_join(&mut scene, Join::Bevel);
    assert!(rgb(&miter, 60, 11)[0] > 100);
    assert!(rgb(&round, 60, 17)[0] > 100);
    assert_eq!(rgb(&bevel, 60, 17), [0; 3]);
}

fn number_key(vector: &mut VectorLayer, property: VectorProperty, t: f64, value: f32) {
    vector.key(
        VectorKey::new(
            t,
            property,
            VectorValue::Numbers([value, 0.0, 0.0, 0.0, 0.0]),
        ),
        t,
    );
}

#[test]
fn trim_is_empty_half_and_full_and_wraps_with_offset() {
    let mut scene = scene();
    add_path(&mut scene, &[[-0.5, 0.0], [0.5, 0.0]], false);
    assert!(scene.shapes[0].draw_on(0.0, 2.0));
    let left = |image: &Pixmap| rgb(image, 30, 50);
    let right = |image: &Pixmap| rgb(image, 90, 50);
    assert_eq!(left(&draw(&scene, 0.0)), [0; 3]);
    assert_eq!(left(&draw(&scene, 1.0)), [255; 3]);
    assert_eq!(right(&draw(&scene, 1.0)), [0; 3]);
    assert_eq!(right(&draw(&scene, 2.0)), [255; 3]);
    scene.shapes[0]
        .vector
        .as_mut()
        .unwrap()
        .trim
        .as_mut()
        .unwrap()
        .offset = 270.0;
    let wrapped = draw(&scene, 1.0);
    assert_eq!(left(&wrapped), [255; 3]);
    assert_eq!(right(&wrapped), [255; 3]);
    assert_eq!(rgb(&wrapped, 60, 50), [0; 3]);
}

#[test]
fn trim_can_apply_to_the_fill() {
    let mut scene = scene();
    let vector = add_path(
        &mut scene,
        &[[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]],
        true,
    );
    vector.stroke = None;
    vector.fill = None;
    vector.trim = Some(Trim {
        end: 50.0,
        fill: true,
        ..Trim::default()
    });
    let image = draw(&scene, 0.0);
    assert_ne!(rgb(&image, 80, 35), [0; 3]);
    assert_eq!(rgb(&image, 32, 77), [0; 3]);
}

#[test]
fn trim_spans_subpaths_in_order() {
    let mut scene = scene();
    let vector = add_path(&mut scene, &[[-0.5, -0.2], [0.5, -0.2]], false);
    vector
        .path
        .as_mut()
        .unwrap()
        .contours
        .push(contour(&[[-0.5, 0.2], [0.5, 0.2]], false));
    vector.trim = Some(Trim {
        end: 75.0,
        ..Trim::default()
    });
    let image = draw(&scene, 0.0);
    assert_eq!(rgb(&image, 90, 34), [255; 3]);
    assert_eq!(rgb(&image, 30, 66), [255; 3]);
    assert_eq!(rgb(&image, 90, 66), [0; 3]);
    let error = animated_svg::animated(&scene, &place()).unwrap_err();
    assert!(error.contains("Path") && error.contains("trim across multiple subpaths"));
}

#[test]
fn clearing_follows_preserves_stroke_and_radial_paint_under_a_scaled_parent() {
    let mut scene = scene();
    let vector = add_path(
        &mut scene,
        &[[-0.4, -0.4], [0.4, -0.4], [0.4, 0.4], [-0.4, 0.4]],
        true,
    );
    vector.fill = Some(PaintStyle::new(PaintMode::Radial));
    scene.add_shape("parent".into(), "rect");
    let parent = &mut scene.shapes[1];
    parent.keys[0].w = 30.0;
    parent.keys[0].h = 50.0;
    parent.keys[0].o = 0.0;
    scene.set_parent(0, Some(1), 0.0).unwrap();
    scene.shapes[1].keys[0].w = 33.0;
    scene.shapes[1].keys[0].h = 46.0;
    let before = draw(&scene, 0.0);
    scene.set_parent(0, None, 0.0).unwrap();
    assert_eq!(draw(&scene, 0.0).data(), before.data());
}

#[test]
fn unequal_point_counts_resample_and_use_the_path_lanes_easing() {
    let mut scene = scene();
    add_path(&mut scene, &[[-0.5, 0.0], [0.5, 0.0]], false);
    let layer = &mut scene.shapes[0];
    let lane = Lane::Vector(VectorProperty::Path);
    layer.add_lane_key(lane, 0.0);
    let target = path(&[[-0.5, 0.0], [0.0, -0.5], [0.5, 0.0]], false);
    let mut first = layer.lane_key(lane, 0.0).unwrap();
    first.set_easing("linear", None);
    layer.put_lane_key(0.0, first);
    layer.put_lane_key(
        2.0,
        LaneKey::Vector(VectorKey::new(
            2.0,
            VectorProperty::Path,
            VectorValue::Path(target.clone()),
        )),
    );
    let morph = layer.path_at(1.0, [80.0; 2]).unwrap();
    assert_eq!(morph.contours[0].points[1].point, [0.0, -0.25]);
    assert_eq!(layer.path_at(2.0, [80.0; 2]).unwrap(), target);
    assert!(layer.retime_lane_keys(lane, &[0.0, 2.0], 0.5));
    assert_eq!(
        layer
            .lane_keys(lane)
            .iter()
            .map(|key| key.time())
            .collect::<Vec<_>>(),
        [0.5, 2.5]
    );
}

#[test]
fn gradient_stop_channels_use_the_key_lanes_and_clip_moves() {
    let mut scene = scene();
    scene.add_shape("shape".into(), "rect");
    let layer = &mut scene.shapes[0];
    layer.ease = "linear".into();
    layer.vector = Some(VectorLayer {
        fill: Some(PaintStyle::new(PaintMode::Linear)),
        ..VectorLayer::default()
    });
    let lane = Lane::Vector(VectorProperty::GradientStop(PaintTarget::Fill, 0));
    layer.add_lane_key(lane, 0.0);
    layer.add_lane_key(lane, 1.0);
    let mut key = layer.lane_key(lane, 1.0).unwrap();
    key.set_value(0, 50.0, false);
    key.set_value(1, 0.0, false);
    key.set_value(4, 20.0, false);
    layer.put_lane_key(1.0, key);
    assert_eq!(lane.value(layer, 0.5, 0), 25.0);
    assert_eq!(lane.value(layer, 0.5, 1), 127.5);
    assert_eq!(lane.value(layer, 0.5, 4), 60.0);
    assert_eq!(scene.shift_layer(0, 0.25), 0.25);
    assert_eq!(lane.value(&scene.shapes[0], 0.75, 4), 60.0);
}

fn render_svg(svg_text: &str) -> Pixmap {
    let tree = svg::parse(svg_text).unwrap();
    let size = tree.size().to_int_size();
    let mut image = Pixmap::new(size.width(), size.height()).unwrap();
    resvg::render(&tree, Transform::identity(), &mut image.as_mut());
    image
}

fn compare_svg(raster: &Pixmap, svg: &Pixmap) {
    let total: u64 = raster
        .data()
        .iter()
        .zip(svg.data())
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    let mean = total as f64 / raster.data().len() as f64;
    assert!(mean < 1.0, "mean channel difference {mean}");
}

#[test]
fn animated_svg_has_native_path_gradient_and_dash_attributes_and_matches_frame_zero() {
    let mut scene = scene();
    let vector = add_path(&mut scene, &[[-0.5, 0.0], [0.0, -0.35], [0.5, 0.0]], false);
    let stroke = vector.stroke.as_mut().unwrap();
    stroke.paint = PaintStyle::new(PaintMode::Linear);
    stroke.cap = Cap::Round;
    stroke.join = Join::Bevel;
    stroke.dash = Some(Dash::default());
    number_key(vector, VectorProperty::DashOffset, 0.0, 0.0);
    number_key(vector, VectorProperty::DashOffset, 2.0, 10.0);
    let layer = &mut scene.shapes[0];
    let lane = Lane::Vector(VectorProperty::Path);
    layer.add_lane_key(lane, 0.0);
    layer.put_lane_key(
        2.0,
        LaneKey::Vector(VectorKey::new(
            2.0,
            VectorProperty::Path,
            VectorValue::Path(path(&[[-0.5, 0.2], [0.5, 0.2]], false)),
        )),
    );
    let stop = Lane::Vector(VectorProperty::GradientStop(PaintTarget::Stroke, 0));
    layer.add_lane_key(stop, 0.0);
    layer.add_lane_key(stop, 2.0);
    let mut key = layer.lane_key(stop, 2.0).unwrap();
    key.set_value(1, 20.0, false);
    layer.put_lane_key(2.0, key);
    let svg_text = animated_svg::animated(&scene, &place()).unwrap();
    for part in [
        "<linearGradient",
        "stroke-linecap=\"round\"",
        "stroke-linejoin=\"bevel\"",
        "attributeName=\"d\"",
        "attributeName=\"stroke-dashoffset\"",
        "attributeName=\"stop-color\"",
    ] {
        assert!(svg_text.contains(part), "{part}");
    }
    compare_svg(&draw(&scene, 0.0), &render_svg(&svg_text));
    for t in [0.5, 1.0, 2.0] {
        let still = animated_svg::still(&scene, &place(), t).unwrap();
        compare_svg(&draw(&scene, t), &render_svg(&still));
    }
}

#[test]
fn svg_trim_emits_dash_animation_and_names_unsupported_combinations() {
    let mut scene = scene();
    add_path(&mut scene, &[[-0.5, 0.0], [0.5, 0.0]], false);
    assert!(scene.shapes[0].draw_on(0.0, 2.0));
    let animated = animated_svg::animated(&scene, &place()).unwrap();
    assert!(animated.contains("attributeName=\"stroke-dasharray\""));
    compare_svg(&draw(&scene, 0.0), &render_svg(&animated));
    compare_svg(
        &draw(&scene, 1.0),
        &render_svg(&animated_svg::still(&scene, &place(), 1.0).unwrap()),
    );
    let vector = scene.shapes[0].vector.as_mut().unwrap();
    vector.stroke.as_mut().unwrap().dash = Some(Dash::default());
    let error = animated_svg::animated(&scene, &place()).unwrap_err();
    assert!(error.contains("Path") || error.contains("Rectangle"));
    assert!(error.contains("trim combined with stroke dashes"));
}

fn animated_path_project() -> Scene {
    let mut scene = scene();
    add_path(&mut scene, &[[-0.5, 0.0], [0.5, 0.0]], false);
    let shape = &mut scene.shapes[0];
    shape.add_lane_key(Lane::Vector(VectorProperty::Path), 0.0);
    shape.draw_on(0.5, 1.5);
    let pose = Pose {
        w: 100.0,
        r: 20.0,
        ..Pose::from(&shape.keys[0])
    };
    shape.key_property(1.0, Property::Size, pose);
    shape.key_property(1.0, Property::Rotation, pose);
    scene
}

#[test]
fn path_projects_round_trip_as_json_and_archives() {
    let scene = animated_path_project();
    let data = serde_json::to_vec(&scene).unwrap();
    assert_eq!(Scene::read(&data).unwrap(), scene);

    let file = std::env::temp_dir().join(format!("forma-shape-paths-{}.forma", std::process::id()));
    archive::pack(&scene, &place(), &file).unwrap();
    let reopened = archive::unpack(&file, &place()).unwrap();
    std::fs::remove_file(file).unwrap();
    assert_eq!(reopened, scene);
}

#[test]
#[ignore = "requires Node.js to run Electron Forma's validator"]
fn the_existing_electron_validator_accepts_path_projects() {
    let data = serde_json::to_vec(&animated_path_project()).unwrap();
    let mut validator = Command::new("node")
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
        .args(["--input-type=module", "-e", r#"
            import {readFileSync} from 'node:fs';
            import {valid, usableProject} from './src/model/validate.js';
            const project = JSON.parse(readFileSync(0, 'utf8'));
            if (!valid(project) || !usableProject(project) || project.shapes[0].type !== 'svg') process.exit(1);
        "#])
        .stdin(Stdio::piped()).spawn().expect("Node is required to run Electron's project validator");
    validator.stdin.take().unwrap().write_all(&data).unwrap();
    assert!(validator.wait().unwrap().success());
}

#[test]
#[ignore = "reports software-renderer timing on this machine"]
fn render_thirty_1080p_layers_while_scrubbing() {
    let mut scene = scene();
    scene.width = 1920.0;
    scene.height = 1080.0;
    for i in 0..30 {
        let kind = ["circle", "rect", "triangle", "diamond", "arrow"][i % 5];
        scene.add_shape(format!("layer-{i}"), kind);
        let shape = scene.shapes.last_mut().unwrap();
        shape.keys[0].x = 170.0 + (i % 6) as f32 * 300.0;
        shape.keys[0].y = 120.0 + (i / 6) as f32 * 205.0;
        shape.keys[0].w = 230.0;
        assert!(shape.convert_to_path([230.0, 160.0]));
        let vector = shape.vector.as_mut().unwrap();
        vector.fill = Some(PaintStyle::new(PaintMode::Linear));
        vector.stroke = Some(StrokeStyle::default());
        assert!(shape.draw_on(0.0, scene.duration));
    }
    let renderer = Renderer::new(&scene, &place());
    let frames = Frames::new();
    let _ = renderer.frame(&scene, 0.0, (1920, 1080), &frames).unwrap();
    let mut timings = Vec::new();
    for i in 0..30 {
        let start = std::time::Instant::now();
        let t = f64::from((i * 17) % 30) / 15.0;
        let frame = renderer.frame(&scene, t, (1920, 1080), &frames).unwrap();
        timings.push(start.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(frame);
    }
    timings.sort_by(f64::total_cmp);
    eprintln!(
        "1080p, 30 layers: {:.1} ms median, {:.1} ms p95",
        timings[15], timings[28]
    );
}
