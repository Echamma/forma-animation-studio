use crate::effects::{Effect, Kind, Parameter};
use crate::model::Shape;
use canopy_ui::color;
use egui::{Button, CollapsingHeader, DragValue, RichText, Sense, Ui, vec2};

#[derive(Clone)]
struct DragEffect {
    layer: String,
    effect: u64,
}

enum Edit {
    Move {
        source: u64,
        target: u64,
        after: bool,
    },
    Nudge(u64, isize),
    Duplicate(u64),
    Delete(u64),
}

pub(crate) fn layer(ui: &mut Ui, shape: &mut Shape, time: f64, lanes_shown: bool) -> bool {
    ui.push_id(&shape.id.clone(), |ui| {
        CollapsingHeader::new("Effects")
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 32.0;
                ui.checkbox(&mut shape.motion_blur, "Motion blur");
                let mut reveal = false;
                if add_menu(ui, shape) {
                    reveal = true;
                }
                if !shape.effects.is_empty()
                    && ui
                        .add(Button::selectable(
                            lanes_shown,
                            "Show effect animation lanes",
                        ))
                        .clicked()
                {
                    reveal = true;
                }
                let actions = draw_stack(ui, shape, time);
                for action in actions {
                    apply_edit(shape, action);
                }
                reveal
            })
            .body_returned
            .unwrap_or(false)
    })
    .inner
}

fn add_menu(ui: &mut Ui, shape: &mut Shape) -> bool {
    let id = ui.scope_id().with("effect-search");
    let mut search = ui
        .data_mut(|data| data.get_temp::<String>(id))
        .unwrap_or_default();
    let mut picked = None;
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new("Add effect…")
        .config(config)
        .ui(ui, |ui| {
            ui.set_min_width(210.0);
            ui.add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text("Search effects")
                    .desired_width(205.0),
            );
            let query = search.trim().to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    let mut group = "";
                    for &kind in Kind::ALL {
                        if !kind.label().to_lowercase().contains(&query)
                            && !kind.group().to_lowercase().contains(&query)
                            && !(kind == Kind::ZoomBlur && "radial".contains(&query))
                        {
                            continue;
                        }
                        if group != kind.group() {
                            group = kind.group();
                            canopy_ui::section_label(ui, group);
                        }
                        if ui
                            .add_sized([ui.available_width(), 32.0], Button::new(kind.label()))
                            .clicked()
                        {
                            picked = Some(kind);
                            ui.close();
                        }
                    }
                    if group.is_empty() {
                        ui.label(RichText::new("No matching effects").color(color::INK_MUTED));
                    }
                });
        });
    if let Some(kind) = picked {
        shape
            .effects
            .push(Effect::new(shape.next_effect_id(), kind));
        search.clear();
    }
    ui.data_mut(|data| data.insert_temp(id, search));
    picked.is_some()
}

fn draw_stack(ui: &mut Ui, shape: &mut Shape, time: f64) -> Vec<Edit> {
    let mut actions = Vec::new();
    let count = shape.effects.len();
    for (index, effect) in shape.effects.iter_mut().enumerate() {
        ui.push_id(effect.id, |ui| {
            let folded = CollapsingHeader::new(effect.kind.label()).show(ui, |ui| {
                controls(ui, effect, &shape.ease, time);
            });
            let header = ui
                .horizontal_wrapped(|ui| {
                    ui.checkbox(&mut effect.enabled, "On")
                        .on_hover_text("Enable this effect");
                    let handle = ui.add(
                        Button::new("Drag")
                            .min_size(vec2(42.0, 32.0))
                            .sense(Sense::drag()),
                    );
                    canopy_ui::drag_source(
                        ui,
                        &handle,
                        || DragEffect {
                            layer: shape.id.clone(),
                            effect: effect.id,
                        },
                        effect.kind.label(),
                    );
                    ui.menu_button("Options", |ui| {
                        for (label, step, available) in [
                            ("Move up", -1, index > 0),
                            ("Move down", 1, index + 1 < count),
                        ] {
                            if ui.add_enabled(available, Button::new(label)).clicked() {
                                actions.push(Edit::Nudge(effect.id, step));
                                ui.close();
                            }
                        }
                        if ui.button("Duplicate").clicked() {
                            actions.push(Edit::Duplicate(effect.id));
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            actions.push(Edit::Delete(effect.id));
                            ui.close();
                        }
                    });
                })
                .response;
            let target = effect.id;
            let drop = header.rect.union(folded.header_response.rect);
            if ui.rect_contains_pointer(drop)
                && let Some(payload) = egui::DragAndDrop::payload::<DragEffect>(ui.ctx())
                && payload.layer == shape.id
                && payload.effect != target
                && let Some(pointer) = ui.ctx().pointer_interact_pos()
            {
                let after = pointer.y >= drop.center().y;
                let y = if after { drop.bottom() } else { drop.top() };
                ui.painter()
                    .hline(drop.x_range(), y, egui::Stroke::new(2.0, color::SUN));
                if ui.input(|input| input.pointer.any_released())
                    && egui::DragAndDrop::take_payload::<DragEffect>(ui.ctx()).is_some()
                {
                    actions.push(Edit::Move {
                        source: payload.effect,
                        target,
                        after,
                    });
                }
            }
            ui.separator();
        });
    }
    actions
}

fn controls(ui: &mut Ui, effect: &mut Effect, ease: &str, time: f64) {
    if effect.kind == Kind::GradientRamp {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut effect.radial, false, "Linear");
            ui.selectable_value(&mut effect.radial, true, "Radial");
        });
    }
    for control in effect.kind.controls() {
        let parameter = control.parameter;
        let mut value = effect.values(parameter, time, ease);
        let animated = effect
            .tracks
            .get(&parameter)
            .is_some_and(|track| !track.keys.is_empty());
        let response = ui
            .horizontal_wrapped(|ui| {
                ui.label(parameter.label());
                if parameter.is_color() {
                    let mut rgb = value.map(|v| v.round() as u8);
                    let response = egui::color_picker::color_edit_button_srgb(ui, &mut rgb);
                    if response.changed() {
                        value = rgb.map(f32::from);
                    }
                    response
                } else {
                    let speed = match parameter {
                        Parameter::Gamma | Parameter::Speed => 0.02,
                        _ => 0.25,
                    };
                    ui.add_sized(
                        [68.0, 32.0],
                        DragValue::new(&mut value[0])
                            .range(parameter.range())
                            .speed(speed),
                    )
                }
            })
            .inner;
        if animated {
            response
                .clone()
                .on_hover_text("Editing adds or updates a key at the playhead");
        }
        if response.changed() {
            effect.edit(parameter, time, value);
        }
    }
}

fn apply_edit(shape: &mut Shape, action: Edit) {
    let index = |shape: &Shape, id| shape.effects.iter().position(|effect| effect.id == id);
    match action {
        Edit::Move {
            source,
            target,
            after,
        } => {
            if source == target {
                return;
            }
            let (Some(from), Some(to)) = (index(shape, source), index(shape, target)) else {
                return;
            };
            let slot = to + usize::from(after) - usize::from(from < to);
            let effect = shape.effects.remove(from);
            shape.effects.insert(slot, effect);
        }
        Edit::Nudge(id, delta) => {
            if let Some(from) = index(shape, id)
                && let Some(to) = from
                    .checked_add_signed(delta)
                    .filter(|&i| i < shape.effects.len())
            {
                shape.effects.swap(from, to);
            }
        }
        Edit::Duplicate(id) => {
            if let Some(from) = index(shape, id) {
                let mut effect = shape.effects[from].clone();
                effect.id = shape.next_effect_id();
                shape.effects.insert(from + 1, effect);
            }
        }
        Edit::Delete(id) => shape.effects.retain(|effect| effect.id != id),
    }
}
