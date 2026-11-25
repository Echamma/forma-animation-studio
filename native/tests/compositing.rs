mod common;

use canopy_ui::Place;
use forma_native::compositing::{
    Blend, LayerMask, MaskMode, MaskPoint, MaskPose, MaskProperty, MaskShape, MatteMode, TrackMatte,
};
use forma_native::keyframes::{Lane, Property};
use forma_native::model::Scene;
use forma_native::render::{Frames, Renderer};
use forma_native::transform::Anchor;
use forma_native::{animated_svg, archive};
use std::io::{Read, Write};
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
    let mut scene = Scene::empty("compositing".into());
    (scene.width, scene.height) = (128.0, 128.0);
    scene.bg = "#000000".into();
    scene
}

fn rect(scene: &mut Scene, id: &str, color: &str, x: f32, w: f32) -> usize {
    let index = scene.shapes.len();
    scene.add_shape(id.into(), "rect");
    let shape = &mut scene.shapes[index];
    shape.name = id.into();
    shape.color = color.into();
    let key = &mut shape.keys[0];
    (key.x, key.y, key.w, key.h) = (x, 64.0, w, w);
    index
}

fn mask(id: u64, mode: MaskMode, x: f32, w: f32) -> LayerMask {
    LayerMask {
        id,
        shape: MaskShape::Rectangle,
        mode,
        inverted: false,
        reference_size: [112.0; 2],
        rest: MaskPose::new([x, 0.0], [w, 64.0]),
        keys: Vec::new(),
    }
}

fn matte(source: &str, mode: MatteMode) -> TrackMatte {
    TrackMatte {
        source: source.into(),
        mode,
        show_layer: false,
    }
}

fn draw(scene: &Scene, time: f64) -> Pixmap {
    Renderer::new(scene, &place())
        .frame(scene, time, (128, 128), &Frames::new())
        .unwrap()
}

fn rgb(image: &Pixmap, x: u32, y: u32) -> [u8; 3] {
    let p = image.pixel(x, y).unwrap();
    assert_eq!(p.alpha(), 255);
    [p.red(), p.green(), p.blue()]
}

#[test]
fn all_sixteen_blends_on_known_pixels() {
    let mut scene = scene();
    rect(&mut scene, "Below", "#4080e0", 64.0, 112.0);
    let layer = rect(&mut scene, "Above", "#c05030", 64.0, 64.0);
    let expected = [
        (Blend::Normal, [192, 80, 48]),
        (Blend::Multiply, [48, 40, 42]),
        (Blend::Screen, [208, 168, 230]),
        (Blend::Overlay, [96, 81, 205]),
        (Blend::Darken, [64, 80, 48]),
        (Blend::Lighten, [192, 128, 224]),
        (Blend::Add, [255, 208, 255]),
        (Blend::Difference, [128, 48, 176]),
        (Blend::ColorDodge, [255, 187, 255]),
        (Blend::ColorBurn, [1, 0, 90]),
        (Blend::SoftLight, [96, 104, 207]),
        (Blend::HardLight, [161, 80, 84]),
        (Blend::Hue, [210, 86, 50]),
        (Blend::Saturation, [70, 127, 214]),
        (Blend::Color, [201, 89, 57]),
        (Blend::Luminosity, [55, 119, 215]),
    ];
    let renderer = Renderer::new(&scene, &place());
    for (blend, want) in expected {
        scene.shapes[layer].blend_mode = Some(blend);
        let frame = renderer
            .frame(&scene, 0.0, (128, 128), &Frames::new())
            .unwrap();
        let actual = rgb(&frame, 64, 64);
        assert!(
            actual.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 1),
            "{blend:?}: {actual:?} expected {want:?}"
        );
        assert_eq!(rgb(&frame, 20, 64), [64, 128, 224]);
    }
}

#[test]
fn mask_modes_combine_in_stack_order() {
    let mut scene = scene();
    let layer = rect(&mut scene, "Artwork", "#ff0000", 64.0, 112.0);
    let masks = [
        (MaskMode::Add, [255, 255, 255, 0]),
        (MaskMode::Subtract, [255, 0, 0, 0]),
        (MaskMode::Intersect, [0, 255, 0, 0]),
        (MaskMode::Difference, [255, 0, 255, 0]),
    ];
    for (mode, expected) in masks {
        scene.shapes[layer].masks = vec![
            mask(1, MaskMode::Add, -16.0, 48.0),
            mask(2, mode, 16.0, 48.0),
        ];
        let frame = draw(&scene, 0.0);
        assert_eq!(
            [32, 64, 96, 110].map(|x| rgb(&frame, x, 64)[0]),
            expected,
            "{mode:?}"
        );
    }
    scene.shapes[layer].masks = vec![mask(1, MaskMode::Subtract, 0.0, 32.0)];
    let frame = draw(&scene, 0.0);
    assert_eq!([48, 90].map(|x| rgb(&frame, x, 64)[0]), [0, 255]);
    scene.shapes[layer].masks[0].mode = MaskMode::Intersect;
    let frame = draw(&scene, 0.0);
    assert_eq!([48, 90].map(|x| rgb(&frame, x, 64)[0]), [255, 0]);
}

#[test]
fn inverted_mask_opacity_and_feather_use_alpha() {
    let mut scene = scene();
    let layer = rect(&mut scene, "Artwork", "#ff0000", 64.0, 112.0);
    let mut m = mask(1, MaskMode::Add, 0.0, 32.0);
    m.inverted = true;
    m.rest.opacity = 50.0;
    scene.shapes[layer].masks.push(m);
    let renderer = Renderer::new(&scene, &place());
    let frame = |scene: &Scene| {
        renderer
            .frame(scene, 0.0, (128, 128), &Frames::new())
            .unwrap()
    };
    assert_eq!([64, 104].map(|x| rgb(&frame(&scene), x, 64)[0]), [0, 128]);
    let m = &mut scene.shapes[layer].masks[0];
    m.inverted = false;
    m.rest.opacity = 100.0;
    m.rest.feather = 5.0;
    let feathered = frame(&scene);
    let [inside, edge, outside, far] = [64, 79, 84, 108].map(|x| rgb(&feathered, x, 64)[0]);
    assert!(inside > edge && edge > outside && outside > far);
    assert_eq!(far, 0);
    scene.shapes[layer].masks[0].rest.feather = 64.0;
    let wide = frame(&scene);
    assert!(rgb(&wide, 100, 64)[0] > 0);
    assert_eq!(
        renderer
            .preview(&scene, 0.0, 1.0, &Frames::new())
            .unwrap()
            .data(),
        wide.data(),
    );
}

#[test]
fn expansion_dilates_and_erodes_the_path() {
    let mut scene = scene();
    let layer = rect(&mut scene, "Artwork", "#ff0000", 64.0, 112.0);
    let m = mask(1, MaskMode::Add, 0.0, 32.0);
    scene.shapes[layer].masks.push(m);
    for (expansion, expected) in [(0.0, [255, 0]), (8.0, [255, 255]), (-8.0, [0, 0])] {
        scene.shapes[layer].masks[0].rest.expansion = expansion;
        assert_eq!(
            [75, 84].map(|x| rgb(&draw(&scene, 0.0), x, 64)[0]),
            expected
        );
    }
}

#[test]
fn feather_stretches_with_the_layer() {
    let mut scene = scene();
    let layer = rect(&mut scene, "Artwork", "#ff0000", 64.0, 112.0);
    let mut m = mask(1, MaskMode::Add, 0.0, 32.0);
    m.reference_size = [56.0, 112.0];
    m.rest.size = [32.0, 32.0];
    m.rest.feather = 5.0;
    scene.shapes[layer].masks.push(m);
    let frame = draw(&scene, 0.0);
    let x = rgb(&frame, 103, 64)[0];
    let y = rgb(&frame, 64, 87)[0];
    assert!(
        x > y + 20,
        "more feather should reach along the stretched axis, got {x} and {y}"
    );
}

#[test]
fn an_animated_mask_uses_independent_lanes_and_outgoing_easing() {
    let mut scene = scene();
    let layer = rect(&mut scene, "Artwork", "#ff0000", 64.0, 112.0);
    let shape = &mut scene.shapes[layer];
    let mut m = mask(7, MaskMode::Add, -24.0, 22.0);
    m.shape = MaskShape::Ellipse;
    shape.masks.push(m);
    let position = Lane::Mask(7, MaskProperty::Position);
    shape.add_lane_key(position, 0.0);
    shape.edit_mask(7, MaskProperty::Position, 2.0, [24.0, 0.0]);
    let mut first = shape.lane_key(position, 0.0).unwrap();
    first.set_easing("hold", None);
    shape.put_lane_key(0.0, first);
    assert!(
        shape
            .lane_keys(Lane::Mask(7, MaskProperty::Size))
            .is_empty()
    );
    assert_eq!(rgb(&draw(&scene, 1.0), 40, 64)[0], 255);
    assert_eq!(rgb(&draw(&scene, 2.0), 88, 64)[0], 255);
    assert_eq!(rgb(&draw(&scene, 2.0), 40, 64)[0], 0);
    assert!(scene.shapes[layer].retime_lane_keys(position, &[2.0], 0.5));
    assert_eq!(rgb(&draw(&scene, 2.0), 40, 64)[0], 255);
    assert_eq!(rgb(&draw(&scene, 2.5), 88, 64)[0], 255);
}

#[test]
fn all_mask_properties_interpolate_and_shift_with_the_clip() {
    let mut scene = scene();
    let layer = rect(&mut scene, "Artwork", "#ff0000", 64.0, 112.0);
    let shape = &mut scene.shapes[layer];
    shape.masks.push(mask(1, MaskMode::Add, 0.0, 32.0));
    let properties = [
        (MaskProperty::Position, [20.0, -10.0], [10.0, -5.0]),
        (MaskProperty::Size, [22.0, 44.0], [27.0, 54.0]),
        (MaskProperty::Rotation, [90.0, 0.0], [45.0, 0.0]),
        (MaskProperty::Feather, [10.0, 0.0], [5.0, 0.0]),
        (MaskProperty::Expansion, [-10.0, 0.0], [-5.0, 0.0]),
        (MaskProperty::Opacity, [20.0, 0.0], [60.0, 0.0]),
    ];
    for (property, value, _) in properties {
        shape.add_lane_key(Lane::Mask(1, property), 0.0);
        shape.edit_mask(1, property, 2.0, value);
    }
    assert_eq!(scene.shift_layer(layer, 0.5), 0.5);
    let shape = &scene.shapes[layer];
    let sampled = shape.masks[0].pose(1.5, &shape.ease);
    for (property, _, expected) in properties {
        assert_eq!(sampled.values(property), expected);
    }
    assert_eq!(scene.minimum_duration(), 2.5);
}

#[test]
fn alpha_and_luma_mattes_include_source_opacity_and_inversion() {
    let mut scene = scene();
    rect(&mut scene, "Target", "#ff0000", 64.0, 112.0);
    let source = rect(&mut scene, "Source", "#ff0000", 64.0, 56.0);
    scene.shapes[source].keys[0].o = 50.0;
    for (mode, expected) in [
        (MatteMode::Alpha, [128, 0]),
        (MatteMode::AlphaInverted, [127, 255]),
        (MatteMode::Luma, [27, 0]),
        (MatteMode::LumaInverted, [228, 255]),
    ] {
        assert!(scene.set_matte("Target", Some(matte("Source", mode))));
        let frame = draw(&scene, 0.0);
        let actual = [64, 30].map(|x| rgb(&frame, x, 64)[0]);
        assert!(
            actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
            "{mode:?}: {actual:?}"
        );
    }
}

#[test]
fn a_matte_source_is_hidden_until_shown_and_can_itself_use_a_matte() {
    let mut scene = scene();
    rect(&mut scene, "Target", "#ff0000", 64.0, 112.0);
    rect(&mut scene, "Source", "#0000ff", 64.0, 80.0);
    rect(&mut scene, "Cutout", "#00ff00", 80.0, 32.0);
    assert!(scene.set_matte("Target", Some(matte("Source", MatteMode::Alpha))));
    assert!(scene.set_matte("Source", Some(matte("Cutout", MatteMode::Alpha))));
    let frame = draw(&scene, 0.0);
    assert_eq!(rgb(&frame, 80, 64), [255, 0, 0]);
    assert_eq!(rgb(&frame, 50, 64), [0, 0, 0]);
    let mut shown = matte("Source", MatteMode::Alpha);
    shown.show_layer = true;
    assert!(scene.set_matte("Target", Some(shown)));
    assert_eq!(rgb(&draw(&scene, 0.0), 80, 64), [0, 0, 255]);
}

#[test]
fn matte_cycles_are_refused_and_layer_deletion_unlinks_consumers() {
    let mut scene = scene();
    for id in ["A", "B", "C"] {
        rect(&mut scene, id, "#ffffff", 64.0, 112.0);
    }
    assert!(scene.set_matte("A", Some(matte("B", MatteMode::Alpha))));
    assert!(scene.set_matte("B", Some(matte("C", MatteMode::Alpha))));
    assert!(!scene.set_matte("C", Some(matte("A", MatteMode::Alpha))));
    assert!(!scene.set_matte("C", Some(matte("C", MatteMode::Alpha))));
    assert!(scene.shapes[2].track_matte.is_none());
    scene.shapes[2].track_matte = Some(matte("A", MatteMode::Alpha));
    let bad = serde_json::to_vec(&scene).unwrap();
    assert!(
        Scene::read(&bad)
            .unwrap_err()
            .contains("track matte layers form a loop")
    );
    scene.shapes[2].track_matte = None;
    scene.delete_layers(&["B".into()], 0.0);
    assert!(scene.shapes[0].track_matte.is_none());
}

#[test]
fn masks_follow_animated_parents_and_noncentral_anchors() {
    let mut scene = scene();
    let parent = rect(&mut scene, "Parent", "#ffffff", 54.0, 50.0);
    scene.shapes[parent].keys[0].o = 0.0;
    let child = rect(&mut scene, "Child", "#ff0000", 34.0, 40.0);
    scene.shapes[child].anchor = Some(Anchor { x: 0.0, y: 0.5 });
    let mut m = mask(1, MaskMode::Add, 8.0, 12.0);
    m.reference_size = [40.0; 2];
    m.rest.size[1] = 12.0;
    scene.shapes[child].masks.push(m);
    scene.set_parent(child, Some(parent), 0.0).unwrap();
    let p = &mut scene.shapes[parent];
    let mut pose = p.pose(0.0);
    (pose.x, pose.w, pose.h) = (74.0, 100.0, 100.0);
    p.key_property(1.0, Property::Position, pose);
    p.key_property(1.0, Property::Size, pose);
    let c = &mut scene.shapes[child];
    let mut pose = c.pose(0.0);
    pose.r = 90.0;
    c.key_property(1.0, Property::Rotation, pose);
    assert_eq!(rgb(&draw(&scene, 0.0), 62, 64), [255, 0, 0]);
    let moved = draw(&scene, 1.0);
    assert_eq!(rgb(&moved, 34, 120), [255, 0, 0]);
    assert_eq!(rgb(&moved, 62, 64), [0, 0, 0]);
}

fn project() -> Scene {
    let mut scene = scene();
    let layer = rect(&mut scene, "Artwork", "#ff0000", 64.0, 112.0);
    rect(&mut scene, "Source", "#ffffff", 64.0, 96.0);
    assert!(scene.set_matte("Artwork", Some(matte("Source", MatteMode::Luma))));
    scene.shapes[layer].blend_mode = Some(Blend::Multiply);
    let mut m = mask(19, MaskMode::Add, 0.0, 80.0);
    let point = |point, incoming, outgoing| MaskPoint {
        point,
        incoming,
        outgoing,
    };
    m.shape = MaskShape::Path {
        points: vec![
            point([-0.5, -0.5], None, Some([0.4, -0.1])),
            point([0.5, -0.5], Some([-0.4, -0.1]), None),
            point([0.5, 0.5], None, None),
            point([-0.5, 0.5], None, None),
        ],
    };
    scene.shapes[layer].masks.push(m);
    let lane = Lane::Mask(19, MaskProperty::Feather);
    scene.shapes[layer].add_lane_key(lane, 0.0);
    scene.shapes[layer].edit_mask(19, MaskProperty::Feather, 2.0, [12.0, 0.0]);
    let mut key = scene.shapes[layer].lane_key(lane, 0.0).unwrap();
    key.set_easing("custom", Some([0.3, 0.2, 0.7, 1.1]));
    scene.shapes[layer].put_lane_key(0.0, key);
    scene
}

fn with_saved_project(check: impl FnOnce(&std::path::Path, &Scene, &Place)) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

    let scene = project();
    let place = place();
    let path = place.exports_dir.join(format!(
        "forma-compositing-{}-{}.forma",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    struct Saved(std::path::PathBuf);
    impl Drop for Saved {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let saved = Saved(path);
    archive::pack(&scene, &place, &saved.0).unwrap();
    check(&saved.0, &scene, &place);
}

#[test]
fn a_project_round_trips_paths_handles_keys_blend_and_matte() {
    with_saved_project(|path, scene, place| {
        common::assert_electron_keys(&serde_json::from_slice(&saved_project_json(path)).unwrap());
        let reopened = archive::unpack(path, place).unwrap();
        assert_eq!(&reopened, scene);
        assert!(reopened.shapes[1].masks.is_empty());
        let plain = serde_json::to_value(&reopened.shapes[1]).unwrap();
        for field in ["blendMode", "masks", "trackMatte"] {
            assert!(plain.get(field).is_none());
        }
    });
}

fn saved_project_json(path: &std::path::Path) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut json = Vec::new();
    archive.by_name("project.json").unwrap().read_to_end(&mut json).unwrap();
    json
}

#[test]
#[ignore = "requires Node.js to run Electron Forma's validator"]
fn electron_accepts_the_native_saved_project() {
    use std::process::{Command, Stdio};
    with_saved_project(|path, _, _| {
        let json = saved_project_json(path);
        common::assert_electron_keys(&serde_json::from_slice(&json).unwrap());
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let mut node = Command::new("node")
            .current_dir(root)
            .args([
                "--input-type=module",
                "-e",
                r#"
                import {valid, usableProject} from './src/model/validate.js';
                import {readFileSync} from 'node:fs';
                const project = JSON.parse(readFileSync(0, 'utf8'));
                if (!valid(project) || !usableProject(project)) process.exitCode = 1;
            "#,
            ])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        node.stdin.take().unwrap().write_all(&json).unwrap();
        assert!(node.wait().unwrap().success());
    });
}

#[test]
fn svg_exports_css_modes_and_names_unsupported_features() {
    let mut scene = scene();
    let layer = rect(&mut scene, "Artwork", "#ff0000", 64.0, 112.0);
    scene.shapes[layer].blend_mode = Some(Blend::ColorBurn);
    let place = place();
    assert!(
        animated_svg::animated(&scene, &place)
            .unwrap()
            .contains("mix-blend-mode:color-burn")
    );
    scene.shapes[layer].blend_mode = Some(Blend::Add);
    assert!(
        animated_svg::animated(&scene, &place)
            .unwrap_err()
            .contains("Artwork: Animated SVG can't export the Add blend mode")
    );
    scene.shapes[layer].blend_mode = None;
    scene.shapes[layer]
        .masks
        .push(mask(1, MaskMode::Add, 0.0, 20.0));
    assert!(
        animated_svg::animated(&scene, &place)
            .unwrap_err()
            .contains("Artwork: Animated SVG can't export layer masks")
    );
    scene.shapes[layer].masks.clear();
    rect(&mut scene, "Source", "#ffffff", 64.0, 60.0);
    assert!(scene.set_matte("Artwork", Some(matte("Source", MatteMode::Alpha))));
    assert!(
        animated_svg::animated(&scene, &place)
            .unwrap_err()
            .contains("Artwork: Animated SVG can't export track mattes")
    );
}
