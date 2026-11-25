use egui::AsIdSalt;
use egui::{
    CollapsingHeader, CollapsingResponse, ComboBox, ScrollArea, Ui, WidgetText,
    scroll_area::ScrollBarVisibility,
};

pub(crate) fn combo(
    ui: &mut Ui,
    id: impl AsIdSalt,
    label: &str,
    selected: impl Into<WidgetText>,
) -> ComboBox<'static> {
    ui.label(label);
    ComboBox::from_id_salt(id)
        .width(ui.available_width())
        .height(280.0)
        .truncate()
        .selected_text(selected)
}

#[cfg(test)]
pub(crate) fn record_rect(ui: &Ui, name: &'static str, rect: egui::Rect) {
    ui.data_mut(|data| data.insert_temp(egui::Id::unique(("workspace bounds", name)), rect));
}

pub(crate) fn scroll<R>(id: impl AsIdSalt, ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 32.0;
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.spacing_mut().scroll.floating = false;
        let area = ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink(false)
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible);
        #[cfg(test)]
        let area = match ui.data(|data| data.get_temp(egui::Id::unique("forma-test-scroll"))) {
            Some(offset) => area.vertical_scroll_offset(offset),
            None => area,
        };
        let output = area.show(ui, content);
        #[cfg(test)]
        ui.data_mut(|data| {
            let id = egui::Id::unique("forma-test-scroll-extent");
            let extent = (output.content_size.y - output.inner_rect.height()).max(0.0);
            let max = data.get_temp::<f32>(id).unwrap_or(0.0).max(extent);
            data.insert_temp(id, max);
        });
        output.inner
    })
    .inner
}

pub(crate) fn section<R>(
    ui: &mut Ui,
    header: CollapsingHeader,
    body: impl FnOnce(&mut Ui) -> R,
) -> CollapsingResponse<R> {
    #[cfg(test)]
    let header = if ui
        .data(|data| data.get_temp::<bool>(egui::Id::unique("forma-test-open-sections")))
        .unwrap_or(false)
    {
        header.open(Some(true))
    } else {
        header
    };
    header.show(ui, body)
}
