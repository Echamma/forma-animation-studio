//! Forma's native editor. The model reads the same version-one scene JSON that Electron Forma saves.

pub mod animated_svg;
pub mod app;
pub mod archive;
pub mod assets;
pub mod automation;
pub mod bridge;
pub mod compositing;
pub mod contract;
pub mod effects;
mod effect_render;
mod effect_panel;
mod effect_svg;
mod key_editor;
mod mask_editor;
mod mask_render;
pub mod keyframes;
pub mod mcp;
pub mod media;
pub mod model;
pub mod motion;
pub mod motion_blur;
pub mod paths;
mod path_editor;
mod project_keys;
pub mod shape_style;
mod shape_panel;
pub mod render;
pub mod sound;
pub mod storage;
pub mod svg;
pub mod text;
pub mod text_animator;
mod text_layout;
mod text_panel;
mod text_presets;
mod text_render;
#[cfg(test)]
mod text_tests;
pub mod transform;
pub mod video;
pub mod videos;
mod vector_render;
mod vector_svg;
mod value_track;

use canopy_ui::{Accent, AppDescriptor, CanopyApp, Place, SharedFile};

fn create(place: Place, _ctx: &egui::Context) -> Box<dyn CanopyApp> {
    Box::new(app::Forma::new(place))
}

pub static DESCRIPTOR: AppDescriptor = AppDescriptor {
    id: "forma",
    name: "Forma",
    description: "Animate shapes and pictures",
    accent: Accent::Clay,
    initial: "Fo",
    folder: "Forma",
    icon_png: &[],
    accepts: |file: &SharedFile| {
        [
            ".json", ".forma", ".png", ".jpg", ".jpeg", ".webp", ".svg", ".mp4", ".webm", ".ogv",
        ]
        .iter()
        .any(|ext| file.name.to_lowercase().ends_with(ext))
    },
    documents: storage::list,
    create,
};
