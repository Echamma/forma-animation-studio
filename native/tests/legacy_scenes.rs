use forma_native::model::Scene;

// Fingerprints captured with the pre-property-keyframe evaluator and serializer.
fn fingerprint(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn legacy_projects_keep_the_same_poses_and_saved_bytes() {
    let scenes = [
        {
            use std::io::{Cursor, Read};
            let mut archive =
                zip::ZipArchive::new(Cursor::new(include_bytes!("fixtures/electron.forma")))
                    .unwrap();
            let mut json = Vec::new();
            archive
                .by_name("project.json")
                .unwrap()
                .read_to_end(&mut json)
                .unwrap();
            Scene::read(&json).unwrap()
        },
        Scene::read(include_bytes!("fixtures/animated.json")).unwrap(),
        Scene::example("compatibility".into()),
    ];
    let mut actual = Vec::new();
    for scene in scenes {
        let mut poses = Vec::new();
        for step in 0..=600 {
            let t = scene.duration * f64::from(step) / 600.0;
            for shape in &scene.shapes {
                let p = scene.render_pose(shape, t);
                for component in [p.x, p.y, p.w, p.h, p.r, p.o] {
                    poses.extend(component.to_le_bytes());
                }
                poses.extend(shape.color_at(t).bytes());
            }
        }
        actual.push((
            fingerprint(poses),
            fingerprint(serde_json::to_vec(&scene).unwrap()),
        ));
    }
    assert_eq!(
        actual,
        [
            (8560338539251353994, 14475041811208111560),
            (2326580108565226465, 12045580417129233484),
            (2393567991165441168, 16321556286396617917),
        ],
    );
}
