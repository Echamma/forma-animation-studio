use crate::{
    archive,
    model::{Scene, Shape},
    render::Renderer,
};
use canopy_ui::Place;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};
use tiny_skia::Pixmap;

pub const EXTENSION: &str = "formatemplate";
pub const OPEN_PREFIX: &str = "forma-template:";
const VERSION_FIELD: &str = "formaTemplateVersion";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    Text,
    Color,
    WidthScale,
    HeightScale,
    PlaybackSpeed,
}

impl Target {
    pub const ALL: [Self; 5] = [
        Self::Text,
        Self::Color,
        Self::WidthScale,
        Self::HeightScale,
        Self::PlaybackSpeed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text content",
            Self::Color => "Fill color",
            Self::WidthScale => "Width scale (%)",
            Self::HeightScale => "Height scale (%)",
            Self::PlaybackSpeed => "Playback speed",
        }
    }

    pub fn applies(self, shape: &Shape) -> bool {
        match self {
            Self::Text => shape.kind == "text",
            Self::Color => {
                shape.kind == "text"
                    || (shape.is_vector()
                        && shape
                            .vector
                            .as_ref()
                            .and_then(|vector| vector.fill.as_ref())
                            .is_none_or(|fill| fill.mode == crate::shape_style::PaintMode::Solid))
            }
            Self::WidthScale | Self::HeightScale => shape.kind != "adjustment",
            Self::PlaybackSpeed => shape.composition.is_some(),
        }
    }

    fn default_value(self, shape: &Shape) -> Value {
        match self {
            Self::Text => Value::Text(crate::text::content(shape).into()),
            Self::Color => Value::Color(shape.color.clone()),
            Self::WidthScale | Self::HeightScale => Value::Number(100.0),
            Self::PlaybackSpeed => Value::Number(
                shape
                    .composition
                    .as_ref()
                    .map_or(1.0, |playback| playback.speed),
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub id: String,
    pub label: String,
    pub target: Target,
}

impl Shape {
    pub(crate) fn rekey_template_fields(&mut self) {
        for (index, field) in self.template_fields.iter_mut().enumerate() {
            field.id = format!("{}:{index}", self.id);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Value {
    Text(String),
    Color(String),
    Number(f64),
}

pub type Values = BTreeMap<String, Value>;

#[derive(Clone, Debug)]
pub struct Editable {
    pub field: Field,
    pub default: Value,
}

pub struct Template {
    scene: Scene,
    fields: Vec<Editable>,
}

pub struct Instance {
    scene: Scene,
    renderer: Renderer,
}

pub fn save(scene: &Scene, place: &Place, path: &Path) -> Result<(), String> {
    scene.validate_compositions()?;
    let mut reachable = HashSet::new();
    let mut pending = vec![scene];
    while let Some(item) = pending.pop() {
        for playback in item
            .shapes
            .iter()
            .filter_map(|shape| shape.composition.as_ref())
        {
            if reachable.insert(playback.source.as_str()) {
                if let Some(child) = scene.composition(&playback.source) {
                    pending.push(child);
                }
            }
        }
    }
    let mut scene = scene.clone();
    scene
        .compositions
        .retain(|child| reachable.contains(child.project_id.as_str()));
    fields(&scene)?;
    scene.extra.insert(VERSION_FIELD.into(), 1.into());
    archive::pack(&scene, place, path)
}

pub fn load(path: &Path, place: &Place) -> Result<Template, String> {
    let scene = archive::unpack_checked(path, place, |project| {
        match project[VERSION_FIELD].as_u64() {
            Some(1) => Ok(()),
            Some(version) if version > 1 => {
                Err("This template needs a newer version of Forma.".into())
            }
            Some(_) => Err("This template version is unsupported.".into()),
            None => Err("This file is missing its Forma template version.".into()),
        }
    })?;
    let fields = fields(&scene)?;
    Ok(Template { scene, fields })
}

fn fields(scene: &Scene) -> Result<Vec<Editable>, String> {
    let mut ids = HashSet::new();
    let mut editable = Vec::new();
    for layer in std::iter::once(scene)
        .chain(&scene.compositions)
        .flat_map(|item| &item.shapes)
    {
        for field in &layer.template_fields {
            if field.id.is_empty()
                || field.label.trim().is_empty()
                || !ids.insert(&field.id)
                || !field.target.applies(layer)
            {
                return Err(format!(
                    "{}: invalid editable field '{}'.",
                    layer.name, field.label
                ));
            }
            editable.push(Editable {
                field: field.clone(),
                default: field.target.default_value(layer),
            });
        }
    }
    Ok(editable)
}

impl Template {
    pub fn name(&self) -> &str {
        &self.scene.project_name
    }
    pub fn fields(&self) -> &[Editable] {
        &self.fields
    }
    pub fn duration(&self) -> f64 {
        self.scene.duration
    }
    pub fn size(&self) -> (u32, u32) {
        crate::render::scene_size(&self.scene)
    }

    pub fn instantiate(&self, values: &Values, place: &Place) -> Result<Instance, String> {
        let mut scene = self.scene.clone();
        scene.motion_blur_preview = scene.motion_blur.is_some();
        for composition in &mut scene.compositions {
            composition.motion_blur_preview = composition.motion_blur.is_some();
        }
        for layer in &mut scene.shapes {
            apply_fields(layer, values)?;
        }
        for layer in scene
            .compositions
            .iter_mut()
            .flat_map(|item| &mut item.shapes)
        {
            apply_fields(layer, values)?;
        }
        let renderer = Renderer::new(&scene, place);
        renderer.check(&scene)?;
        Ok(Instance { scene, renderer })
    }
}

impl Instance {
    pub fn duration(&self) -> f64 {
        self.scene.duration
    }
    pub fn size(&self) -> (u32, u32) {
        crate::render::scene_size(&self.scene)
    }

    pub fn render_frame(&self, time: f64, size: (u32, u32)) -> Result<Pixmap, String> {
        let time = self.frame_time(time, size)?;
        self.renderer.cached_frame(&self.scene, time, size, true)
    }

    /// Returns the preview and whether a video decode is pending.
    pub fn preview_frame(&self, time: f64, size: (u32, u32)) -> Result<(Pixmap, bool), String> {
        let time = self.frame_time(time, size)?;
        self.renderer.template_preview(&self.scene, time, size)
    }

    fn frame_time(&self, time: f64, size: (u32, u32)) -> Result<f64, String> {
        if !time.is_finite()
            || size.0 == 0
            || size.1 == 0
            || u64::from(size.0) * u64::from(size.1) > 64_000_000
        {
            return Err("Invalid template frame time or size.".into());
        }
        Ok(time.clamp(0.0, self.scene.duration))
    }
}

fn apply_fields(shape: &mut Shape, values: &Values) -> Result<(), String> {
    for field in &shape.template_fields {
        let Some(value) = values.get(&field.id) else {
            continue;
        };
        match (field.target, value) {
            (Target::Text, Value::Text(text)) if text.encode_utf16().count() <= 120 => {
                shape.extra.insert("text".into(), text.clone().into());
            }
            (Target::Color, Value::Color(color))
                if color.len() == 7
                    && color.starts_with('#')
                    && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit) =>
            {
                shape.color.clone_from(color);
                shape.color_keys.clear();
            }
            (Target::WidthScale | Target::HeightScale, Value::Number(scale))
                if (1.0..=500.0).contains(scale) =>
            {
                let axis = field.target;
                let factor = (*scale / 100.0) as f32;
                let resize = |w: &mut f32, h: &mut f32| {
                    let length = if axis == Target::WidthScale { w } else { h };
                    *length = (*length * factor).clamp(0.001, 10000.0);
                };
                for key in &mut shape.keys {
                    resize(&mut key.w, &mut key.h);
                }
                for pose in [&mut shape.rest_pose, &mut shape.linked_rest]
                    .into_iter()
                    .flatten()
                {
                    resize(&mut pose.w, &mut pose.h);
                }
            }
            (Target::PlaybackSpeed, Value::Number(speed)) if (0.0..=100.0).contains(speed) => {
                if let Some(playback) = &mut shape.composition {
                    playback.speed = *speed;
                }
            }
            _ => {
                return Err(format!(
                    "{}: invalid value for {}.",
                    shape.name, field.label
                ));
            }
        }
    }
    Ok(())
}
