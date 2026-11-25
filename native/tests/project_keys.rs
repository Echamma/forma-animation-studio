mod common;

use forma_native::keyframes::{Lane, Property};
use forma_native::model::{Key, Pose, Scene};
use serde_json::{Value, json};

fn scene() -> Scene {
    let mut scene = Scene::empty("keys".into());
    scene.add_shape("plain".into(), "rect");
    scene
}

fn keyed_scene() -> Scene {
    let mut scene = scene();
    let shape = &mut scene.shapes[0];
    let pose = Pose {
        x: 157.26,
        y: 303.43,
        w: 80.0,
        r: 60.0,
        ..shape.pose(0.0)
    };
    shape.key_property(0.25, Property::Size, pose);
    shape.key_property(1.0, Property::Position, pose);
    shape.key_property(1.0, Property::Rotation, pose);
    scene
}

fn save_and_reopen(scene: &Scene) -> Value {
    let bytes = serde_json::to_vec(scene).unwrap();
    let project = serde_json::from_slice(&bytes).unwrap();
    common::assert_electron_keys(&project);
    assert_eq!(&Scene::read(&bytes).unwrap(), scene);

    let value = serde_json::to_value(scene).unwrap();
    common::assert_electron_keys(&value);
    assert_eq!(
        &Scene::read(&serde_json::to_vec(&value).unwrap()).unwrap(),
        scene
    );
    project
}

#[test]
fn simultaneous_property_keys_save_as_one_full_pose_per_time() {
    let scene = keyed_scene();
    let shape = &scene.shapes[0];
    assert!(shape.vector.is_none());
    assert_eq!(shape.keys.len(), 4);
    let saved = save_and_reopen(&scene);
    let baked: Vec<Key> = serde_json::from_value(saved["shapes"][0]["keys"].clone()).unwrap();
    assert_eq!(
        baked.iter().map(|k| k.t).collect::<Vec<_>>(),
        [0.0, 0.25, 1.0]
    );
    for key in &baked {
        assert!(key.props.is_none());
        assert_eq!(Pose::from(key), shape.pose(key.t));
    }
}

#[test]
fn a_layer_with_no_keys_saves_its_rest_pose_at_zero() {
    let mut scene = scene();
    let shape = &mut scene.shapes[0];
    shape.keys[0].x = 115.5;
    shape.keys[0].r = 16.0;
    for property in Property::ALL {
        shape.remove_lane_key(Lane::Pose(property), 0.0);
    }
    assert!(shape.keys.is_empty());
    let saved = save_and_reopen(&scene);
    let keys: Vec<Key> = serde_json::from_value(saved["shapes"][0]["keys"].clone()).unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].t, 0.0);
    assert_eq!(Pose::from(&keys[0]), scene.shapes[0].rest_pose.unwrap());
}

#[test]
fn a_layer_with_over_a_thousand_keys_keeps_its_endpoints_in_electron() {
    let mut scene = scene();
    let first = scene.shapes[0].keys[0].clone();
    scene.shapes[0].keys = (0..=1001)
        .map(|i| Key {
            t: f64::from(i) * scene.duration / 1001.0,
            x: i as f32,
            ..first.clone()
        })
        .collect();
    let saved = save_and_reopen(&scene);
    let keys = saved["shapes"][0]["keys"].as_array().unwrap();
    assert_eq!(keys.len(), 1000);
    assert_eq!(keys[0]["x"], json!(0.0));
    assert_eq!(keys[999]["t"], json!(scene.duration));
    assert_eq!(keys[999]["x"], json!(1001.0));
}

#[test]
fn out_of_range_times_poses_and_easing_save_as_valid_electron_keys() {
    let mut scene = scene();
    let first = Key {
        x: -11_000.0,
        y: 23_000.0,
        w: 0.0,
        h: 14_000.0,
        r: -20_000.0,
        o: -15.0,
        ease: Some("not-an-easing".into()),
        curve: Some([1.5, -5.5, -0.1, 6.0]),
        ..scene.shapes[0].keys[0].clone()
    };
    let last = Key {
        t: scene.duration,
        w: 0.25,
        h: -2.0,
        o: 180.0,
        ..first.clone()
    };
    scene.shapes[0].keys = vec![
        Key {
            t: -0.5,
            ..first.clone()
        },
        first,
        last.clone(),
        Key {
            t: scene.duration + 0.001,
            ..last
        },
    ];
    let saved = save_and_reopen(&scene);
    let baked: Vec<Key> = serde_json::from_value(saved["shapes"][0]["keys"].clone()).unwrap();
    assert_eq!(baked.len(), 2);
    assert_eq!(baked[0].t, 0.0);
    assert_eq!(baked[1].t, scene.duration);
    assert_eq!(
        Pose::from(&baked[0]),
        Pose {
            x: -10_000.0,
            y: 10_000.0,
            w: 1.0,
            h: 10_000.0,
            r: -10_000.0,
            o: 0.0,
        }
    );
    assert_eq!(
        Pose::from(&baked[1]),
        Pose {
            w: 0.25,
            h: 1.0,
            o: 100.0,
            ..Pose::from(&baked[0])
        }
    );
}

#[test]
fn electron_key_edits_win_and_stale_native_keys_are_dropped() {
    let mut project = save_and_reopen(&keyed_scene());
    let keys = project["shapes"][0]["keys"].as_array_mut().unwrap();
    keys[2]["x"] = json!(777.0);
    keys[2]["ease"] = json!("hold");
    let edited: Vec<Key> = serde_json::from_value(json!(keys)).unwrap();
    common::assert_electron_keys(&project);
    let loaded = Scene::read(&serde_json::to_vec(&project).unwrap()).unwrap();
    assert_eq!(loaded.shapes[0].keys, edited);
    assert_eq!(loaded.shapes[0].pose(1.0).x, 777.0);
    assert!(
        serde_json::to_value(&loaded).unwrap()["shapes"][0]
            .get("nativeKeyframes")
            .is_none()
    );
}

#[test]
fn an_electron_edit_smaller_than_native_pose_precision_still_wins() {
    let scene = keyed_scene();
    let mut project = save_and_reopen(&scene);
    let key = &mut project["shapes"][0]["keys"][2]["x"];
    let x = key.as_f64().unwrap();
    let edited = x + 0.000_001;
    assert_eq!(x as f32, edited as f32);
    *key = json!(edited);
    common::assert_electron_keys(&project);
    let loaded = Scene::read(&serde_json::to_vec(&project).unwrap()).unwrap();
    assert_eq!(loaded.shapes[0].keys.len(), 3);
    assert!(loaded.shapes[0].keys.iter().all(|key| key.props.is_none()));
}
