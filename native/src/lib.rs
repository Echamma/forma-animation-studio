//! Forma's native editor. The model reads the same version-one scene JSON that Electron Forma saves.

pub mod model;
pub mod storage;
pub mod app;

use canopy_ui::{Accent, AppDescriptor, CanopyApp, Place, SharedFile};

fn create(place: Place, _ctx: &egui::Context) -> Box<dyn CanopyApp> { Box::new(app::Forma::new(place)) }

pub static DESCRIPTOR: AppDescriptor = AppDescriptor {
    id: "forma", name: "Forma", description: "Animate shapes and pictures", accent: Accent::Clay,
    initial: "Fo", folder: "Forma", icon_png: &[],
    accepts: |file: &SharedFile| file.name.to_lowercase().ends_with(".json"),
    documents: storage::list, create,
};
