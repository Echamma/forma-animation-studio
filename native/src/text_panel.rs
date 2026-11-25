use crate::keyframes::Lane;
use crate::model::{Shape, hundredths};
use crate::panel_widgets::combo;
use crate::text_animator::{Alignment, Animator, RangeShape, TextLane, TextProperty, Units};
use crate::text_layout::{BUNDLED_FAMILY, available_styles, font_families};
use crate::text_presets::Preset;
use egui::{Button, DragValue, RichText, Ui, vec2};

pub(crate) fn content(ui: &mut Ui, shape: &mut Shape) -> bool {
    let mut words = shape
        .extra
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    ui.label("Text");
    if !ui
        .add(
            egui::TextEdit::multiline(&mut words)
                .desired_rows(3)
                .char_limit(120),
        )
        .changed()
    {
        return false;
    }
    // Electron validates JavaScript string length, measured in UTF-16 code units.
    let mut units = 0;
    let max = words
        .char_indices()
        .find_map(|(at, c)| {
            units += c.len_utf16();
            (units > 120).then_some(at)
        })
        .unwrap_or(words.len());
    words.truncate(max);
    shape.name = words
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Text")
        .chars()
        .take(32)
        .collect();
    shape.text_layout.get_or_insert_with(Default::default);
    shape.extra.insert("text".into(), words.into());
    true
}

pub(crate) fn layout(
    ui: &mut Ui,
    shape: &mut Shape,
    paths: &[(String, String)],
    time: f64,
) -> bool {
    let mut show_lanes = false;
    ui.push_id(("text-layout", shape.id.clone()), |ui| {
        ui.spacing_mut().interact_size.y = 32.0;
        let family = shape
            .text_layout
            .as_ref()
            .and_then(|text| text.font.clone());
        let mut picked = None;
        combo(
            ui,
            "font",
            "Font",
            family.as_deref().unwrap_or(BUNDLED_FAMILY),
        )
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let id = ui.scope_id().with("font-filter");
            let mut search: String = ui.data_mut(|data| data.get_temp(id)).unwrap_or_default();
            if ui
                .add(egui::TextEdit::singleline(&mut search).hint_text("Find a font"))
                .changed()
            {
                ui.data_mut(|data| data.insert_temp(id, search.clone()));
            }
            let filter = search.trim().to_lowercase();
            if BUNDLED_FAMILY.to_lowercase().contains(&filter)
                && ui
                    .selectable_label(family.is_none(), BUNDLED_FAMILY)
                    .clicked()
            {
                picked = Some(None);
            }
            for name in font_families()
                .iter()
                .filter(|name| name.to_lowercase().contains(&filter))
            {
                if ui
                    .selectable_label(family.as_ref() == Some(name), name)
                    .clicked()
                {
                    picked = Some(Some(name.clone()));
                }
            }
            if picked.is_some() {
                ui.data_mut(|data| data.remove::<String>(id));
                ui.close();
            }
        });
        if let Some(picked) = picked
            && picked != family
        {
            let text = shape.text_layout.get_or_insert_with(Default::default);
            let (bold, italic) = picked.as_deref().map_or((false, false), available_styles);
            text.font = picked;
            text.bold &= bold;
            text.italic &= italic;
        }
        if let Some(text) = shape.text_layout.as_mut()
            && let Some(family) = &text.font
        {
            let (bold, italic) = available_styles(family);
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled(bold, egui::Checkbox::new(&mut text.bold, "Bold"));
                ui.add_enabled(italic, egui::Checkbox::new(&mut text.italic, "Italic"));
            });
        }
        let mut alignment = shape
            .text_layout
            .as_ref()
            .map_or(Alignment::Center, |text| text.alignment);
        if choice(
            ui,
            "Alignment",
            &mut alignment,
            &[
                (Alignment::Left, "Left"),
                (Alignment::Center, "Center"),
                (Alignment::Right, "Right"),
            ],
        ) {
            shape
                .text_layout
                .get_or_insert_with(Default::default)
                .alignment = alignment;
        }
        crate::panel_widgets::section(ui, egui::CollapsingHeader::new("Spacing"), |ui| {
            for p in [TextProperty::LineHeight, TextProperty::Tracking] {
                show_lanes |= field(ui, shape, TextLane::Style(p), time);
            }
        });
        crate::panel_widgets::section(ui, egui::CollapsingHeader::new("Text along a path"), |ui| {
            let path = shape
                .text_layout
                .as_ref()
                .and_then(|text| text.path.clone());
            let selected = paths
                .iter()
                .find(|(id, _)| Some(id) == path.as_ref())
                .map_or(
                    if path.is_some() {
                        "Missing path"
                    } else {
                        "None"
                    },
                    |(_, name)| name.as_str(),
                );
            let mut pick = path.clone();
            combo(ui, "path-layer", "Path layer", selected).show_ui(ui, |ui| {
                ui.selectable_value(&mut pick, None, "None");
                for (id, name) in paths {
                    ui.selectable_value(&mut pick, Some(id.clone()), name);
                }
            });
            if pick != path {
                let text = shape.text_layout.get_or_insert_with(Default::default);
                text.path = pick;
                if text.path.is_none() {
                    text.style.remove(&TextProperty::PathOffset);
                }
            }
            if shape
                .text_layout
                .as_ref()
                .is_some_and(|text| text.path.is_some())
            {
                show_lanes |= field(ui, shape, TextLane::Style(TextProperty::PathOffset), time);
            } else if paths.is_empty() {
                ui.label(
                    RichText::new("Draw a path with the Pen tool, then choose its layer here.")
                        .color(canopy_ui::color::INK_MUTED),
                );
            }
        });
    });
    show_lanes
}

#[derive(Default)]
pub(crate) struct AnimateResult {
    pub reveal: Option<u64>,
    pub end: Option<f64>,
    pub full: bool,
}

pub(crate) fn animate(
    ui: &mut Ui,
    shape: &mut Shape,
    time: f64,
    scene_duration: f64,
) -> AnimateResult {
    let mut result = AnimateResult::default();
    ui.push_id(("text-animators", shape.id.clone()), |ui| {
        ui.spacing_mut().interact_size.y = 32.0;
        ui.heading("Text animators");
        let mut preset = None;
        ui.horizontal_wrapped(|ui| {
            ui.menu_button("Text presets", |ui| {
                for choice in Preset::ALL {
                    if ui
                        .add_sized(vec2(166.0, 32.0), Button::new(choice.label()))
                        .clicked()
                    {
                        preset = Some(choice);
                        ui.close();
                    }
                }
            });
            if ui
                .add(Button::new("Add animator").min_size(vec2(0.0, 32.0)))
                .clicked()
            {
                result.reveal = shape
                    .text_layout
                    .get_or_insert_with(Default::default)
                    .add(Animator::default());
                result.full = result.reveal.is_none();
            }
        });
        if let Some(preset) = preset {
            let clip_start = shape.clip_start();
            let duration = preset.length();
            let (start, end) = if let Some(clip_end) = shape.out_time {
                let start = time.clamp(clip_start, (clip_end - 0.05).max(clip_start));
                (start, (start + duration).min(clip_end))
            } else {
                let start = time.min(scene_duration).max(clip_start);
                (start, start + duration)
            };
            let em = shape.lane_pose(time).h;
            let text = shape.text_layout.get_or_insert_with(Default::default);
            result.reveal = preset.apply(text, start, end, em);
            result.full = result.reveal.is_none();
            result.end = result.reveal.map(|_| hundredths(end));
        }
        let animators: Vec<_> = shape
            .text_layout
            .iter()
            .flat_map(|text| &text.animators)
            .map(|a| (a.id, a.name.clone()))
            .collect();
        for (id, name) in animators {
            crate::panel_widgets::section(
                ui,
                egui::CollapsingHeader::new(name)
                    .id_salt(id)
                    .open((result.reveal == Some(id)).then_some(true)),
                |ui| {
                    if animator(ui, shape, id, time) {
                        result.reveal = Some(id);
                    }
                },
            );
        }
    });
    result
}

fn animator(ui: &mut Ui, shape: &mut Shape, id: u64, time: f64) -> bool {
    let (mut reveal, remove) = ui
        .horizontal_wrapped(|ui| {
            (
                ui.button("Show animator lanes").clicked(),
                ui.button("Remove animator").clicked(),
            )
        })
        .inner;
    let Some(text) = &mut shape.text_layout else {
        return reveal;
    };
    let Some(animator) = text.animator_mut(id) else {
        return reveal;
    };
    if remove {
        text.animators.retain(|a| a.id != id);
        return false;
    }
    ui.label("Animator name");
    ui.add(egui::TextEdit::singleline(&mut animator.name).char_limit(60));
    choice(
        ui,
        "Based on",
        &mut animator.units,
        &[
            (Units::Characters, "Characters"),
            (Units::VisibleCharacters, "Characters excluding spaces"),
            (Units::Words, "Words"),
            (Units::Lines, "Lines"),
        ],
    );
    choice(
        ui,
        "Range shape",
        &mut animator.range_shape,
        &[
            (RangeShape::Square, "Square"),
            (RangeShape::RampUp, "Ramp up"),
            (RangeShape::RampDown, "Ramp down"),
            (RangeShape::Triangle, "Triangle"),
            (RangeShape::Round, "Round"),
            (RangeShape::Smooth, "Smooth"),
        ],
    );
    for property in [TextProperty::Start, TextProperty::End, TextProperty::Offset] {
        reveal |= field(ui, shape, TextLane::Animator(id, property), time);
    }
    crate::panel_widgets::section(
        ui,
        egui::CollapsingHeader::new("Range easing and order"),
        |ui| {
            for property in [TextProperty::EaseLow, TextProperty::EaseHigh] {
                reveal |= field(ui, shape, TextLane::Animator(id, property), time);
            }
            let Some(animator) = shape
                .text_layout
                .as_mut()
                .and_then(|text| text.animator_mut(id))
            else {
                return;
            };
            let mut shuffled = animator.seed.is_some();
            if ui.checkbox(&mut shuffled, "Randomize order").changed() {
                animator.seed = shuffled.then_some(0);
            }
            if let Some(seed) = &mut animator.seed {
                ui.horizontal(|ui| {
                    ui.label("Seed");
                    ui.add(DragValue::new(seed));
                });
            }
        },
    );
    crate::panel_widgets::section(
        ui,
        egui::CollapsingHeader::new("What changes").default_open(true),
        |ui| {
            for property in TextProperty::EFFECTS {
                let Some(animator) = shape
                    .text_layout
                    .as_mut()
                    .and_then(|text| text.animator_mut(id))
                else {
                    return;
                };
                let mut active = animator.tracks.contains_key(&property);
                let mut response = ui.checkbox(&mut active, property.label());
                if property == TextProperty::Stroke
                    && shape
                        .vector
                        .as_ref()
                        .is_none_or(|style| style.stroke.is_none())
                {
                    response =
                        response.on_hover_text("Add a stroke in the Layer tab to see this color.");
                }
                if response.changed() {
                    if active {
                        animator.enable(property, property.initial());
                    } else {
                        animator.tracks.remove(&property);
                    }
                }
                if active {
                    ui.indent(property, |ui| {
                        reveal |= field(ui, shape, TextLane::Animator(id, property), time);
                    });
                }
            }
        },
    );
    reveal
}

fn choice<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    current: &mut T,
    options: &[(T, &str)],
) -> bool {
    let old = *current;
    let name = options
        .iter()
        .find(|(v, _)| *v == old)
        .map_or("", |(_, name)| name);
    combo(ui, label, label, name).wrap().show_ui(ui, |ui| {
        for &(value, name) in options {
            ui.selectable_value(current, value, name);
        }
    });
    old != *current
}

fn field(ui: &mut Ui, shape: &mut Shape, lane: TextLane, time: f64) -> bool {
    let property = lane.property();
    ui.push_id(lane, |ui| {
        let mut value = lane.values(shape, time);
        let mut changed = false;
        let mut key = false;
        if !matches!(property, TextProperty::Fill | TextProperty::Stroke) {
            ui.label(property.label());
        }
        ui.horizontal_wrapped(|ui| {
            if matches!(property, TextProperty::Fill | TextProperty::Stroke) {
                let mut rgb = value.map(|v| v.round() as u8);
                changed = ui.color_edit_button_srgb(&mut rgb).changed();
                value = rgb.map(f32::from);
            } else {
                let channels = property.channels();
                for (i, label) in channels.iter().enumerate() {
                    let prefix = if channels.len() > 1 {
                        format!("{label} ")
                    } else {
                        String::new()
                    };
                    changed |= ui
                        .add(
                            DragValue::new(&mut value[i])
                                .speed(0.5)
                                .range(property.limits())
                                .prefix(prefix)
                                .suffix(property.suffix()),
                        )
                        .changed();
                }
            }
            let on_key = shape.lane_key(Lane::Text(lane), hundredths(time)).is_some();
            key = ui
                .add(Button::selectable(
                    on_key,
                    if on_key { "On key" } else { "Add key" },
                ))
                .on_hover_text(format!("{} at the playhead", property.label()))
                .clicked();
        });
        if changed || key {
            shape.text_layout.get_or_insert_with(Default::default);
        }
        if changed {
            lane.set(shape, time, value);
        }
        if key {
            shape.add_lane_key(Lane::Text(lane), time);
        }
        key
    })
    .inner
}
