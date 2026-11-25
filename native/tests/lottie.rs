mod common;

use forma_native::{
    compositing::{LayerMask, MaskMode, MaskPose, MaskShape},
    keyframes::{Lane, LaneKey, Property},
    lottie,
    model::{Pose, Scene},
    render::{Frames, Renderer},
    shape_style::{PaintMode, PaintStyle, StrokeStyle, Trim, VectorLayer},
};
use serde_json::{Value, json};

#[test]
fn lottie_structures_and_pixels_round_trip() {
    let mut scene = Scene::empty("root".into());
    (scene.width, scene.height, scene.duration) = (320.0, 240.0, 2.0);
    scene.bg = "#181d28".into();
    scene.add_shape("base".into(), "rect");
    let card = &mut scene.shapes[0];
    card.name = "Gradient card".into();
    (card.keys[0].w, card.keys[0].h, card.keys[0].x) = (145.0, 140.0, 112.0);
    card.vector = Some(VectorLayer {
        fill: Some(PaintStyle::new(PaintMode::Linear)),
        ..Default::default()
    });
    card.masks.push(LayerMask {
        id: 9,
        shape: MaskShape::Ellipse,
        mode: MaskMode::Add,
        inverted: false,
        reference_size: [145.0, 140.0],
        rest: MaskPose::new([0.0; 2], [105.0, 90.0]),
        keys: Vec::new(),
    });
    let mut pose = Pose::from(&card.keys[0]);
    pose.x += 38.0;
    card.key_property(1.0, Property::Position, pose);
    if let Some(LaneKey::Pose(property, mut key)) =
        card.lane_key(Lane::Pose(Property::Position), 0.0)
    {
        key.ease = Some("custom".into());
        key.curve = Some([0.25, 0.0, 0.5, 1.0]);
        card.put_lane_key(0.0, LaneKey::Pose(property, key));
    }

    scene.add_shape("stroke".into(), "circle");
    let outline = &mut scene.shapes[1];
    outline.name = "Trimmed outline".into();
    (outline.keys[0].w, outline.keys[0].h, outline.keys[0].x) = (52.0, 52.0, 130.0);
    outline.vector = Some(VectorLayer {
        fill: Some(PaintStyle::new(PaintMode::None)),
        stroke: Some(StrokeStyle::default()),
        trim: Some(Trim {
            end: 67.0,
            ..Default::default()
        }),
        ..Default::default()
    });
    scene.set_parent(1, Some(0), 0.0).unwrap();

    let mut child = Scene::empty("source".into());
    child.project_name = "Outlined text".into();
    (child.width, child.height, child.duration) = (320.0, 240.0, 2.0);
    child.transparent_background = true;
    child.add_text("caption".into());
    let text = &mut child.shapes[0];
    text.extra.insert("text".into(), "Forma".into());
    (
        text.keys[0].x,
        text.keys[0].y,
        text.keys[0].w,
        text.keys[0].h,
    ) = (234.0, 170.0, 105.0, 22.0);
    scene.compositions.push(child);
    scene
        .add_composition_layer("source-layer".into(), "source")
        .unwrap();

    let bytes = lottie::export(&scene).unwrap();
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(doc["fr"], 30.0);
    let layers = doc["layers"].as_array().unwrap();
    let layer = |name| layers.iter().find(|layer| layer["nm"] == name).unwrap();
    let has_shape = |layer: &Value, kind| {
        layer["shapes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["ty"] == kind)
    };
    let card = layer("Gradient card");
    assert_eq!(card["ks"]["p"]["a"], 1);
    assert_eq!(card["ks"]["p"]["k"][0]["o"]["x"], 0.25);
    assert_eq!(card["masksProperties"][0]["mode"], "a");
    assert!(has_shape(card, "gf"));
    let outline = layer("Trimmed outline");
    assert!(outline["parent"].is_number());
    assert!(has_shape(outline, "tm"));
    let text = &doc["assets"][0]["layers"][0];
    assert_eq!(text["ty"], 4);
    assert!(has_shape(text, "sh"));

    let imported = lottie::import(&bytes).unwrap();
    let saved = serde_json::to_value(&imported).unwrap();
    common::assert_electron_keys(&saved);
    common::assert_electron_keys(&saved["compositions"][0]);
    assert_eq!(imported.compositions.len(), 1);
    assert!(lottie::export(&imported).is_ok());
    let root = std::env::temp_dir();
    let place = canopy_ui::Place {
        space: None,
        data_dir: root.clone(),
        projects_dir: root.clone(),
        exports_dir: root,
    };
    let raster = |scene: &Scene, time| {
        Renderer::new(scene, &place)
            .frame(scene, time, (320, 240), &Frames::default())
            .unwrap()
    };
    for time in [0.0, 0.37, 1.3] {
        let (before, after) = (raster(&scene, time), raster(&imported, time));
        let differences = before
            .pixels()
            .iter()
            .zip(after.pixels())
            .filter(|(before, after)| {
                before.red().abs_diff(after.red()) > 3
                    || before.green().abs_diff(after.green()) > 3
                    || before.blue().abs_diff(after.blue()) > 3
            })
            .count();
        assert!(differences < 120, "{differences} pixels differ at {time}s");
    }
}

#[test]
fn lottie_reports_each_unsupported_layer() {
    let mut scene = Scene::empty("root".into());
    scene.add_shape("video".into(), "video");
    scene.shapes[0].name = "Footage".into();
    scene.add_shape("blurred".into(), "rect");
    scene.shapes[1].name = "Soft rectangle".into();
    scene.shapes[1].extra.insert("blur".into(), json!(15));
    let errors = lottie::export(&scene).unwrap_err();
    assert!(errors.contains("Footage") && errors.contains("video"));
    assert!(errors.contains("Soft rectangle") && errors.contains("layer blur"));
}
