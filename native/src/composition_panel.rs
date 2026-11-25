use super::*;
use crate::{lottie, template};

impl Forma {
    pub(super) fn main_id(&self) -> &str {
        self.composition_path
            .first()
            .map_or(&self.scene.project_id, |(id, _)| id)
    }

    pub(super) fn can_paste_composition(&self, layer: &Shape) -> bool {
        layer.composition.as_ref().is_none_or(|playback| {
            playback.source != self.main_id() && self.scene.can_paste_source(&playback.source)
        })
    }

    pub(super) fn project_snapshot(&self) -> Scene {
        let mut project = self.scene.clone();
        if project.project_id != self.main_id() {
            project.switch_composition(self.main_id());
        }
        project
    }

    pub(super) fn project_differs(&self, before: &Scene) -> bool {
        if self.composition_path.is_empty() {
            self.scene != *before
        } else {
            self.project_snapshot() != *before
        }
    }

    pub(super) fn restore_snapshot(&mut self, mut project: Scene) {
        let current = self.scene.project_id.clone();
        if current != project.project_id && project.switch_composition(&current) {
            self.composition_path
                .retain(|(id, _)| project.composition(id).is_some());
        } else {
            self.composition_path.clear();
        }
        if self.scene.project_id != project.project_id {
            self.reset_composition_view();
        }
        self.scene = project;
        self.time = self.time.min(self.scene.duration);
    }

    fn reset_composition_view(&mut self) {
        self.sound.pause();
        self.playing = false;
        self.preview_end = None;
        self.pending_auto_preview = None;
        self.renderer = None;
        self.preview = None;
        self.frame_cache.invalidate();
        self.camera_tool = false;
        self.videos = Videos::default();
        self.selected = None;
        self.multi_selected.clear();
        self.layer_drag = None;
        self.timeline_drag = None;
        self.gesture = None;
        self.key_editor = KeyEditor::default();
        self.mask_editor = MaskEditor::default();
        self.vector_tools = VectorTools::default();
        self.motion_mode = None;
        self.motion_draft = None;
        self.selected_motion = None;
        (self.pan, self.zoom) = (Vec2::ZERO, 1.0);
    }

    fn go_to_composition(&mut self, id: &str, time: f64) {
        if id == self.scene.project_id {
            return;
        }
        let current = (self.scene.project_id.clone(), self.time);
        if !self.scene.switch_composition(id) {
            return;
        }
        if let Some(at) = self
            .composition_path
            .iter()
            .position(|(ancestor, _)| ancestor == id)
        {
            self.time = self.composition_path[at].1.min(self.scene.duration);
            self.composition_path.truncate(at);
        } else {
            self.composition_path.push(current);
            self.time = time.clamp(0.0, self.scene.duration);
        }
        self.reset_composition_view();
    }

    pub(super) fn queue_composition(&mut self, layer_id: &str) {
        let Some(layer) = self.scene.shapes.iter().find(|layer| layer.id == layer_id) else {
            return;
        };
        if let Some(playback) = &layer.composition
            && let Some(source) = self.scene.composition(&playback.source)
        {
            self.pending_composition = Some((
                playback.source.clone(),
                playback.time(self.time - layer.clip_start(), source.duration),
            ));
        }
    }

    pub(super) fn composition_toolbar(&mut self, ui: &mut Ui) {
        if quiet_button(ui, "Compositions", None).clicked() {
            self.show_compositions = true;
        }
    }

    pub(super) fn composition_breadcrumbs(&mut self, ui: &mut Ui) {
        if self.composition_path.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            let path = &self.composition_path;
            let parents = if path.len() > 2 {
                &path[path.len() - 2..]
            } else {
                path
            };
            for (id, time) in parents {
                let name = self
                    .scene
                    .composition(id)
                    .map_or("Main", |item| item.project_name.as_str());
                if quiet_button(ui, &abbreviated(name), None)
                    .on_hover_text(name)
                    .clicked()
                {
                    self.pending_composition = Some((id.clone(), *time));
                }
                ui.label("›");
            }
            ui.label(abbreviated(&self.scene.project_name));
        });
    }

    pub(super) fn precompose_selection(&mut self) {
        let selected: Vec<_> = self
            .selection()
            .into_iter()
            .filter_map(|index| self.scene.shapes.get(index))
            .map(|layer| layer.id.clone())
            .collect();
        let comp_id = new_id();
        let layer_id = new_id();
        match self.scene.precompose(
            &selected,
            comp_id,
            layer_id.clone(),
            next_name(&self.scene, "Composition"),
        ) {
            Ok(()) => self.select_layer(Some(layer_id), false),
            Err(error) => self.notice = Some(error),
        }
    }

    pub(super) fn compositions_window(&mut self, ui: &mut Ui) {
        if let Some((id, time)) = self.pending_composition.take() {
            self.go_to_composition(&id, time);
        }
        if !self.show_compositions {
            return;
        }
        let mut open = None;
        let mut add = None;
        let mut duplicate = None;
        let mut delete = None;
        let mut create = false;
        let mut import = false;
        let mut done = false;
        let current = self.scene.project_id.clone();
        let main = self.main_id().to_owned();
        let breadcrumbs = self.composition_path.clone();
        let pane = self.pane;
        let modal = pane_dialog(ui.ctx(), pane, ui.scope_id().with("compositions"), |ui| {
            ui.set_width(500.0_f32.min(ui.available_width()));
            section_label(ui, "Compositions");
            ui.horizontal_wrapped(|ui| {
                for (id, time) in breadcrumbs {
                    let name = self
                        .scene
                        .composition(&id)
                        .map_or("Main", |item| item.project_name.as_str());
                    if quiet_button(ui, &abbreviated(name), None)
                        .on_hover_text(name)
                        .clicked()
                    {
                        open = Some((id, time));
                    }
                    ui.label("›");
                }
                ui.label(&self.scene.project_name);
            });
            ui.horizontal_wrapped(|ui| {
                create = quiet_button(ui, "New composition", None).clicked();
                import = quiet_button(ui, "Import Lottie", None).clicked();
            });
            ui.separator();
            ui.spacing_mut().scroll.floating = false;
            egui::ScrollArea::vertical()
                .max_height((pane.height() - 196.0).clamp(0.0, 360.0))
                .show(ui, |ui| {
                    composition_row(
                        ui,
                        &mut self.scene,
                        &main,
                        &current,
                        &mut open,
                        &mut add,
                        &mut duplicate,
                        &mut delete,
                    );
                    for item in &mut self.scene.compositions {
                        composition_row(
                            ui,
                            item,
                            &main,
                            &current,
                            &mut open,
                            &mut add,
                            &mut duplicate,
                            &mut delete,
                        );
                    }
                });
            done = quiet_button(ui, "Done", None).clicked();
        });
        self.show_compositions = !(done || modal.close || open.is_some() || add.is_some());
        if let Some((id, time)) = open {
            self.go_to_composition(&id, time);
        }
        if let Some(id) = add {
            let layer = new_id();
            match self.scene.add_composition_layer(layer.clone(), &id) {
                Ok(()) => self.select_layer(Some(layer), false),
                Err(error) => self.notice = Some(error),
            }
        }
        if create {
            let mut child = Scene::empty(new_id());
            (child.width, child.height) = (self.scene.width, self.scene.height);
            child.transparent_background = true;
            child.project_name = next_name(&self.scene, "Composition");
            self.scene.compositions.push(child);
        }
        if let Some(id) = duplicate {
            let mut copy = if id == current {
                Some(self.scene.clone())
            } else {
                self.scene.composition(&id).cloned()
            };
            if let Some(copy) = &mut copy {
                copy.compositions.clear();
                copy.project_id = new_id();
                copy.project_name = next_name(&self.scene, &format!("{} copy", copy.project_name));
                for field in copy
                    .shapes
                    .iter_mut()
                    .flat_map(|layer| &mut layer.template_fields)
                {
                    field.id = new_id();
                }
            }
            self.scene.compositions.extend(copy);
        }
        if let Some(id) = delete {
            if let Err(error) = self.scene.delete_composition(&id) {
                self.notice = Some(error);
            }
        }
        self.scene
            .compositions
            .sort_by(|a, b| a.project_id.cmp(&b.project_id));
        if import
            && let Some(path) = rfd::FileDialog::new()
                .set_title("Import Lottie")
                .add_filter("Lottie animation", &["json"])
                .pick_file()
        {
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|data| lottie::import(&data))
            {
                Ok(scene) => {
                    self.open(scene);
                    self.suppress_history = true;
                    self.edited();
                }
                Err(error) => self.notice = Some(error),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn composition_row(
    ui: &mut Ui,
    item: &mut Scene,
    main: &str,
    current: &str,
    open: &mut Option<(String, f64)>,
    add: &mut Option<String>,
    duplicate: &mut Option<String>,
    delete: &mut Option<String>,
) {
    let id = &item.project_id;
    ui.push_id(id, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut item.project_name)
                    .desired_width(150.0)
                    .char_limit(74)
                    .hint_text("Composition name"),
            )
            .on_hover_text("Rename composition");
            if id == main {
                ui.label("Main");
            }
            if id != current && quiet_button(ui, "Open", None).clicked() {
                *open = Some((id.clone(), 0.0));
            }
            if id != main && id != current && quiet_button(ui, "Add layer", None).clicked() {
                *add = Some(id.clone());
            }
            if quiet_button(ui, "Duplicate", None).clicked() {
                *duplicate = Some(id.clone());
            }
            if id != main && id != current && quiet_button(ui, "Delete", None).clicked() {
                *delete = Some(id.clone());
            }
        });
    });
}

fn next_name(scene: &Scene, base: &str) -> String {
    let base: String = base.chars().take(65).collect();
    let exists = |name: &str| {
        scene.project_name == name
            || scene
                .compositions
                .iter()
                .any(|item| item.project_name == name)
    };
    if !exists(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !exists(name))
        .unwrap_or(base)
}

fn abbreviated(name: &str) -> String {
    let mut chars = name.chars();
    let mut label: String = chars.by_ref().take(12).collect();
    if chars.next().is_some() {
        label.push('…');
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_keeps_the_main_project_and_child_edits_undo_together() {
        let data =
            std::env::temp_dir().join(format!("forma-composition-history-{}", std::process::id()));
        let place = Place {
            space: None,
            data_dir: data.clone(),
            projects_dir: data.clone(),
            exports_dir: data,
        };
        let mut forma = Forma::new(place);
        let main = forma.scene.project_id.clone();
        forma.scene.add_shape("shape".into(), "rect");
        forma
            .scene
            .precompose(
                &["shape".into()],
                "child".into(),
                "reference".into(),
                "Child".into(),
            )
            .unwrap();
        let snapshot = forma.project_snapshot();
        forma.go_to_composition("child", 0.5);
        assert_eq!(forma.scene.project_id, "child");
        assert_eq!(forma.project_snapshot(), snapshot);
        forma.undo.push(snapshot);
        forma.scene.shapes[0].name = "Edit while inside".into();
        forma.undo();
        assert_eq!(forma.scene.project_id, "child");
        assert_eq!(forma.scene.shapes[0].name, "Rectangle");
        forma.redo();
        assert_eq!(forma.scene.shapes[0].name, "Edit while inside");
        forma.go_to_composition(&main, 0.0);
        assert_eq!(forma.scene.project_id, main);
        assert_eq!(
            forma.scene.compositions[0].shapes[0].name,
            "Edit while inside"
        );
    }
}

pub(super) fn layer_settings(ui: &mut Ui, shape: &mut Shape) {
    if let Some(playback) = &mut shape.composition {
        crate::panel_widgets::section(
            ui,
            egui::CollapsingHeader::new("Composition playback").default_open(true),
            |ui| {
                ui.add(
                    egui::DragValue::new(&mut playback.offset)
                        .range(0.0..=86400.0)
                        .speed(0.05)
                        .prefix("Start offset ")
                        .suffix(" s"),
                );
                ui.add(
                    egui::DragValue::new(&mut playback.speed)
                        .range(0.0..=100.0)
                        .speed(0.01)
                        .prefix("Speed ")
                        .suffix("×"),
                );
                ui.checkbox(&mut playback.looping, "Loop source");
            },
        );
    }
    crate::panel_widgets::section(
        ui,
        egui::CollapsingHeader::new("Editable in StreamCuts"),
        |ui| {
            for target in template::Target::ALL {
                let index = shape
                    .template_fields
                    .iter()
                    .position(|field| field.target == target);
                if index.is_none() && !target.applies(shape) {
                    continue;
                }
                let mut enabled = index.is_some();
                if ui.checkbox(&mut enabled, target.label()).changed() {
                    if let Some(index) = index {
                        shape.template_fields.remove(index);
                    } else {
                        shape.template_fields.push(template::Field {
                            id: new_id(),
                            label: format!("{} {}", shape.name, target.label()),
                            target,
                        });
                    }
                }
            }
            if !shape.template_fields.is_empty() {
                ui.separator();
                for field in &mut shape.template_fields {
                    ui.push_id(&field.id, |ui| {
                        ui.label(format!("{} field name", field.target.label()));
                        ui.add(
                            egui::TextEdit::singleline(&mut field.label)
                                .char_limit(120)
                                .desired_width(f32::INFINITY),
                        );
                    });
                }
            }
        },
    );
}
