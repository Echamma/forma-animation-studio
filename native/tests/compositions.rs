mod common;

use forma_native::{
    archive,
    composition::Playback,
    effects::{Effect, Kind, Parameter},
    keyframes::Lane,
    model::{Key, Scene},
    motion_blur::Settings,
    render, template,
};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use tiny_skia::Pixmap;

fn scene(id: &str) -> Scene {
    let mut scene = Scene::empty(id.into());
    (scene.width, scene.height, scene.duration) = (160.0, 120.0, 2.0);
    scene.transparent_background = true;
    scene
}

fn picture(scene: &Scene, time: f64) -> Pixmap {
    render::frame(scene, &place(&std::env::temp_dir()), time).unwrap()
}

fn place(root: &Path) -> canopy_ui::Place {
    canopy_ui::Place {
        space: None,
        data_dir: root.into(),
        projects_dir: root.into(),
        exports_dir: root.into(),
    }
}

fn assert_electron_project(json: &[u8]) {
    let mut node = Command::new("node")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
        .args([
            "--input-type=module",
            "-e",
            r#"
            import {readFileSync} from 'node:fs';
            import {valid, usableProject} from './src/model/validate.js';
            const project = JSON.parse(readFileSync(0, 'utf8'));
            for (const composition of [project, ...project.compositions]) {
                if (!valid(composition) || !usableProject(composition)) {
                    throw new Error(`Electron rejected composition ${composition.projectId}`);
                }
            }
        "#,
        ])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    node.stdin.take().unwrap().write_all(json).unwrap();
    assert!(node.wait().unwrap().success());
}

#[test]
fn composition_uses_its_clip_relative_offset_speed_and_loop() {
    let mut child = scene("child");
    child.add_shape("block".into(), "rect");
    let shape = &mut child.shapes[0];
    shape.keys[0].w = 16.0;
    shape.keys[0].h = 16.0;
    shape.keys[0].x = 20.0;
    let mut moved: Key = shape.keys[0].clone();
    moved.t = child.duration;
    moved.x = 140.0;
    shape.keys.push(moved);
    shape.ease = "linear".into();
    let mut root = scene("root");
    root.duration = 4.0;
    root.compositions.push(child.clone());
    root.add_composition_layer("reference".into(), "child")
        .unwrap();
    let layer = &mut root.shapes[0];
    layer.in_time = Some(0.5);
    let mut playback = Playback::new("child".into());
    (playback.offset, playback.speed, playback.looping) = (0.25, 2.0, true);
    layer.composition = Some(playback);
    assert_eq!(
        picture(&root, 0.0).data(),
        Pixmap::new(160, 120).unwrap().data()
    );
    for (outer, inner) in [(0.5, 0.25), (0.875, 1.0), (1.625, 0.5), (2.875, 1.0)] {
        assert_eq!(
            picture(&root, outer).data(),
            picture(&child, inner).data(),
            "time {outer}"
        );
    }
}

#[test]
fn precompose_preserves_pixels_and_keys_and_refuses_dependency_breaks() {
    let mut root = scene("root");
    root.transparent_background = false;
    root.add_shape("block".into(), "rect");
    root.add_shape("dot".into(), "circle");
    root.shapes[0].keys[0].w = 100.0;
    root.shapes[0].keys[0].h = 60.0;
    root.shapes[1].keys[0].w = 27.0;
    root.shapes[1].keys[0].h = 27.0;
    root.shapes[1].keys[0].x = 60.0;
    root.shapes[1].keys[0].o = 62.0;
    root.shapes[1].in_time = Some(0.5);
    root.set_parent(1, Some(0), 0.0).unwrap();
    let before = root.clone();
    let failure = root.precompose(
        &["dot".into()],
        "child".into(),
        "ref".into(),
        "Selected layers".into(),
    );
    assert!(failure.unwrap_err().contains("Follow"));
    assert_eq!(root, before);

    root.precompose(
        &["block".into(), "dot".into()],
        "child".into(),
        "ref".into(),
        "Selected layers".into(),
    )
    .unwrap();
    let saved = serde_json::to_value(&root).unwrap();
    common::assert_electron_keys(&saved);
    common::assert_electron_keys(&saved["compositions"][0]);
    assert_electron_project(&serde_json::to_vec(&saved).unwrap());
    assert_eq!(saved["shapes"][0]["type"], "svg");
    assert_eq!(saved["compositions"][0]["shapes"][1]["inTime"], 0.5);
    let opened = Scene::read(&serde_json::to_vec(&root).unwrap()).unwrap();
    for time in [0.2, 0.75, 1.3] {
        let (old, new) = (picture(&before, time), picture(&opened, time));
        assert_eq!(
            old.data(),
            new.data(),
            "Precomposing changed pixels at {time}s"
        );
    }
}

fn phase_six_project() -> Scene {
    let mut title = scene("title");
    title.add_text("words".into());
    title.shapes[0].template_fields.push(template::Field {
        id: "headline".into(),
        label: "Headline".into(),
        target: template::Target::Text,
    });

    let mut main = scene("main");
    main.motion_blur = Some(Settings {
        shutter_angle: 240.0,
        samples: 12,
    });
    main.motion_blur_preview = true;
    main.compositions.push(title);
    main.add_composition_layer("title-layer".into(), "title")
        .unwrap();
    let layer = &mut main.shapes[0];
    let playback = layer.composition.as_mut().unwrap();
    (playback.offset, playback.speed, playback.looping) = (0.25, 1.5, true);
    layer.template_fields.push(template::Field {
        id: "speed".into(),
        label: "Title speed".into(),
        target: template::Target::PlaybackSpeed,
    });

    main.add_shape("artwork".into(), "rect");
    let artwork = &mut main.shapes[1];
    artwork.motion_blur = true;
    artwork.effects.push(Effect::new(1, Kind::GaussianBlur));
    let radius = Parameter::Radius;
    let lane = Lane::Effect(1, radius);
    artwork.add_lane_key(lane, 0.0);
    artwork.add_lane_key(lane, 1.0);
    let blur = artwork.effect_mut(1).unwrap();
    blur.edit(radius, 1.0, [24.0, 0.0, 0.0]);
    assert_eq!(blur.tracks[&radius].keys.len(), 2);

    main.add_shape("adjustment".into(), "adjustment");
    main.shapes[2].effects = vec![Effect::new(1, Kind::Tint), Effect::new(2, Kind::Invert)];
    main
}

#[test]
#[ignore = "requires Node.js to run Electron Forma's validator"]
fn electron_accepts_the_native_compositions_and_effects_project() {
    let main = phase_six_project();
    let place = place(&std::env::temp_dir());
    let path = place
        .exports_dir
        .join(format!("forma-phase-six-{}.forma", std::process::id()));
    archive::pack(&main, &place, &path).unwrap();
    let reopened = archive::unpack(&path, &place);
    let json = {
        let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        std::io::read_to_string(zip.by_name("project.json").unwrap()).unwrap()
    };
    fs::remove_file(&path).unwrap();

    assert_electron_project(json.as_bytes());
    assert_eq!(reopened.unwrap(), main);
}

#[test]
fn splitting_and_trimming_a_composition_keep_the_surviving_source_time() {
    let mut child = scene("child");
    child.add_shape("box".into(), "rect");
    child.shapes[0].keys[0].w = 20.0;
    child.shapes[0].keys[0].h = 20.0;
    let mut moved = child.shapes[0].keys[0].clone();
    moved.x += 50.0;
    moved.t = 1.5;
    child.shapes[0].keys.push(moved);
    let mut root = scene("root");
    root.compositions.push(child);
    root.add_composition_layer("layer".into(), "child").unwrap();
    let old_pixels = picture(&root, 1.3);
    root.split_at(0, 0.5, "second".into(), "track".into())
        .unwrap();
    assert_eq!(old_pixels.data(), picture(&root, 1.3).data());
    root.trim_clip(1, 0.9, None).unwrap();
    assert_eq!(old_pixels.data(), picture(&root, 1.3).data());
}

#[test]
fn composition_cycle_is_rejected_before_drawing_or_opening() {
    let mut root = scene("root");
    root.compositions.push(scene("child"));
    root.add_composition_layer("to-child".into(), "child")
        .unwrap();
    let child = &mut root.compositions[0];
    child.add_shape("to-root".into(), "composition");
    child.shapes[0].composition = Some(Playback::new("root".into()));
    assert!(root.validate_compositions().unwrap_err().contains("loop"));
    assert!(
        Scene::read(&serde_json::to_vec(&root).unwrap())
            .unwrap_err()
            .contains("loop")
    );
}

#[test]
fn a_template_carries_its_nested_text_and_applies_field_values() {
    let root =
        std::env::temp_dir().join(format!("forma-phase-six-template-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let place = place(&root);
    let mut design = scene("design");
    design.add_text("text".into());
    design.shapes[0].extra.insert("text".into(), "One".into());
    design.shapes[0].template_fields.push(template::Field {
        id: "words".into(),
        label: "Opening line".into(),
        target: template::Target::Text,
    });
    design
        .precompose(
            &["text".into()],
            "child".into(),
            "layer".into(),
            "Text comp".into(),
        )
        .unwrap();
    let path = root.join("Title.formatemplate");
    template::save(&design, &place, &path).unwrap();
    let loaded = template::load(&path, &place).unwrap();
    assert_eq!(loaded.fields()[0].field.label, "Opening line");
    let before = loaded
        .instantiate(&template::Values::new(), &place)
        .unwrap()
        .render_frame(0.0, (160, 120))
        .unwrap();
    let values = template::Values::from([("words".into(), template::Value::Text("Two".into()))]);
    let changed = loaded
        .instantiate(&values, &place)
        .unwrap()
        .render_frame(0.0, (160, 120))
        .unwrap();
    assert_ne!(before.data(), changed.data());
    assert_eq!(changed.pixel(0, 0).unwrap().alpha(), 0);
    fs::remove_dir_all(root).unwrap();
}
