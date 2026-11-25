use crate::model::{Scene, Shape};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub(crate) const MAX_DEPTH: usize = 16;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playback {
    pub source: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub speed: f64,
    #[serde(default, skip_serializing_if = "crate::effects::is_false")]
    pub looping: bool,
    #[serde(default, skip_serializing_if = "crate::effects::is_false")]
    pub(crate) preserve_stack: bool,
}

fn one() -> f64 {
    1.0
}
fn is_one(value: &f64) -> bool {
    *value == 1.0
}
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

impl Playback {
    pub fn new(source: String) -> Self {
        Self {
            source,
            offset: 0.0,
            speed: 1.0,
            looping: false,
            preserve_stack: false,
        }
    }

    pub fn time(&self, elapsed: f64, duration: f64) -> f64 {
        let time = self.offset + elapsed * self.speed;
        if !time.is_finite() || !duration.is_finite() || duration <= 0.0 {
            return 0.0;
        }
        if self.looping {
            time.rem_euclid(duration)
        } else {
            time.clamp(0.0, duration)
        }
    }

    fn valid(&self) -> bool {
        self.offset.is_finite() && self.offset >= 0.0 && (0.0..=100.0).contains(&self.speed)
    }
}

impl Scene {
    pub fn composition(&self, id: &str) -> Option<&Scene> {
        self.compositions
            .iter()
            .find(|composition| composition.project_id == id)
    }

    pub(crate) fn switch_composition(&mut self, id: &str) -> bool {
        let Some(index) = self
            .compositions
            .iter()
            .position(|item| item.project_id == id)
        else {
            return false;
        };
        let mut catalog = std::mem::take(&mut self.compositions);
        std::mem::swap(self, &mut catalog[index]);
        catalog.sort_by(|a, b| a.project_id.cmp(&b.project_id));
        self.compositions = catalog;
        true
    }

    pub fn validate_compositions(&self) -> Result<(), String> {
        let mut scenes = HashMap::from([(self.project_id.as_str(), self)]);
        for composition in &self.compositions {
            if !composition.compositions.is_empty()
                || composition.project_id.is_empty()
                || scenes
                    .insert(&composition.project_id, composition)
                    .is_some()
            {
                return Err("The project has an invalid composition list.".into());
            }
        }
        let mut depths = HashMap::new();
        for scene in scenes.values() {
            depth(scene, &scenes, &mut HashSet::new(), &mut depths, 0)?;
        }
        Ok(())
    }

    pub fn add_composition_layer(&mut self, id: String, source: &str) -> Result<(), String> {
        if self.shapes.len() >= 500 {
            return Err("This composition already has 500 layers.".into());
        }
        let composition = self.composition(source).ok_or("Composition is missing.")?;
        let (width, height) = (composition.width, composition.height);
        let mut length = 0;
        let name: String = composition
            .project_name
            .chars()
            .take_while(|c| {
                length += c.len_utf16();
                length < 100
            })
            .collect();
        self.add_shape(id, "composition");
        if let Some(layer) = self.shapes.last_mut() {
            layer.name = name;
            layer.keys[0].w = width;
            layer.keys[0].h = height;
            layer.composition = Some(Playback::new(source.into()));
        }
        if let Err(error) = self.validate_compositions() {
            self.shapes.pop();
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn can_paste_source(&self, source: &str) -> bool {
        let mut pending = vec![source];
        let mut seen = HashSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if id == self.project_id {
                return false;
            }
            let Some(child) = self.composition(id) else {
                return false;
            };
            pending.extend(
                child
                    .shapes
                    .iter()
                    .filter_map(|layer| layer.composition.as_ref())
                    .map(|playback| playback.source.as_str()),
            );
        }
        true
    }

    pub fn precompose(
        &mut self,
        selected: &[String],
        comp_id: String,
        layer_id: String,
        name: String,
    ) -> Result<(), String> {
        if comp_id == self.project_id
            || self.composition(&comp_id).is_some()
            || self.shapes.iter().any(|shape| shape.id == layer_id)
        {
            return Err("This composition or layer ID is already in use.".into());
        }
        let selected: HashSet<&str> = selected.iter().map(String::as_str).collect();
        let chosen = |shape: &Shape| selected.contains(shape.id.as_str());
        let first = self
            .shapes
            .iter()
            .position(chosen)
            .ok_or("Select layers to precompose.")?;
        let last = self.shapes.iter().rposition(chosen).unwrap_or(first);
        if self.shapes[first..=last].iter().any(|shape| !chosen(shape)) {
            return Err(
                "Select adjacent layers so precomposing keeps their stacking order.".into(),
            );
        }

        let sampled = self.motion_blur.is_some()
            && self.shapes[first..=last]
                .iter()
                .any(|shape| shape.motion_blur);
        for shape in &self.shapes {
            if dependencies(shape).any(|id| {
                self.shapes
                    .iter()
                    .find(|other| other.id == id)
                    .is_some_and(|other| chosen(other) != chosen(shape))
            }) {
                return Err(format!(
                    "{}: include both layers in each Follow, text link or track matte.",
                    shape.name
                ));
            }
            if sampled
                && chosen(shape)
                && (shape.kind == "adjustment"
                    || shape.blend() != crate::compositing::Blend::Normal)
            {
                return Err(format!(
                    "{}: turn off motion blur to precompose layers that use the original backdrop.",
                    shape.name
                ));
            }
        }

        let mut child = Scene::empty(comp_id.clone());
        child.project_name = name;
        child.width = self.width;
        child.height = self.height;
        child.duration = self.duration;
        child.bg = self.bg.clone();
        child.transparent_background = true;
        child.motion_blur = self.motion_blur;
        child.motion_blur_preview = self.motion_blur_preview;
        child.shapes.extend(self.shapes.drain(first..=last));
        self.compositions.push(child);
        if let Err(error) = self.add_composition_layer(layer_id, &comp_id) {
            if let Some(child) = self.compositions.pop() {
                self.shapes.splice(first..first, child.shapes);
            }
            return Err(error);
        }
        if let Some(layer) = self.shapes.pop() {
            let mut layer = layer;
            if let Some(playback) = &mut layer.composition {
                playback.preserve_stack = true;
            }
            self.shapes.insert(first, layer);
        }
        self.compositions
            .sort_by(|a, b| a.project_id.cmp(&b.project_id));
        Ok(())
    }

    pub fn delete_composition(&mut self, id: &str) -> Result<(), String> {
        if std::iter::once(&*self)
            .chain(&self.compositions)
            .any(|scene| {
                scene.shapes.iter().any(|shape| {
                    shape
                        .composition
                        .as_ref()
                        .is_some_and(|playback| playback.source == id)
                })
            })
        {
            return Err("Remove the layers that use this composition before deleting it.".into());
        }
        self.compositions.retain(|item| item.project_id != id);
        Ok(())
    }
}

fn dependencies(shape: &Shape) -> impl Iterator<Item = &str> {
    [
        shape.parent.as_deref(),
        shape
            .track_matte
            .as_ref()
            .map(|matte| matte.source.as_str()),
        shape
            .extra
            .get("linkId")
            .and_then(serde_json::Value::as_str),
        shape
            .text_layout
            .as_ref()
            .and_then(|text| text.path.as_deref()),
        shape
            .disappear
            .as_ref()
            .and_then(|fade| fade.trigger_shape_id.as_deref()),
    ]
    .into_iter()
    .flatten()
}

fn depth<'a>(
    scene: &'a Scene,
    scenes: &HashMap<&'a str, &'a Scene>,
    visiting: &mut HashSet<&'a str>,
    depths: &mut HashMap<&'a str, usize>,
    level: usize,
) -> Result<usize, String> {
    let id = scene.project_id.as_str();
    if level >= MAX_DEPTH {
        return Err(format!(
            "{}: compositions are nested too deeply.",
            scene.project_name
        ));
    }
    if let Some(&depth) = depths.get(id) {
        return Ok(depth);
    }
    if !visiting.insert(id) {
        return Err(format!(
            "{}: compositions cannot form a loop.",
            scene.project_name
        ));
    }
    let mut result = 1;
    for shape in &scene.shapes {
        let Some(playback) = &shape.composition else {
            if shape.kind == "composition" {
                return Err(format!(
                    "{}: this layer has no source composition.",
                    shape.name
                ));
            }
            continue;
        };
        let child = scenes
            .get(playback.source.as_str())
            .filter(|_| playback.valid() && shape.kind == "composition")
            .ok_or_else(|| format!("{}: invalid composition source or playback.", shape.name))?;
        result = result.max(1 + depth(child, scenes, visiting, depths, level + 1)?);
    }
    if result > MAX_DEPTH {
        return Err(format!(
            "{}: compositions are nested too deeply.",
            scene.project_name
        ));
    }
    visiting.remove(id);
    depths.insert(id, result);
    Ok(result)
}
