//! The find widget's buttons and toggles: drawn here, exposed to screen
//! readers as buttons with their label, enabled, toggled and expanded
//! states, as `find.rs` describes.

use super::{COUNT_SIZE, DISABLED, HOVER, OPTION_BACKGROUND, OPTION_BORDER};
use crate::a11y;
use crate::input::FOREGROUND;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2, accesskit, vec2,
};

/// What a button shows.
#[derive(Debug, Clone, Copy)]
pub(super) enum Icon {
    Text(&'static str),
    Up,
    Down,
    Close,
    /// Pointing right, or down when expanded.
    Chevron(bool),
    Replace,
    ReplaceAll,
}

/// One button of the widget.
pub(super) struct Button<'a> {
    pub(super) id: Id,
    pub(super) rect: Rect,
    pub(super) label: &'a str,
    pub(super) icon: Icon,
    pub(super) enabled: bool,
    /// A toggle's state.
    pub(super) toggled: Option<bool>,
    /// Toggle Replace's state.
    pub(super) expanded: Option<bool>,
}

/// Draws and exposes `button`; returns whether it was clicked.
pub(super) fn button(ui: &mut Ui, button: &Button<'_>) -> bool {
    // Not focusable, as VS Code's are not in the tab order: a click
    // leaves the focus in the field.
    let sense = if button.enabled {
        Sense::CLICK
    } else {
        Sense::hover()
    };
    let response = ui.interact(button.rect, button.id, sense);
    ui.ctx().accesskit_node_builder(button.id, |node| {
        node.set_role(accesskit::Role::Button);
        node.set_label(button.label);
        node.set_bounds(a11y::bounds(button.rect));
        if let Some(on) = button.toggled {
            node.set_toggled(accesskit::Toggled::from(on));
        }
        if let Some(expanded) = button.expanded {
            node.set_expanded(expanded);
        }
        if button.enabled {
            node.add_action(accesskit::Action::Click);
        } else {
            node.set_disabled();
        }
    });
    let painter = ui.painter_at(button.rect.expand(1.0));
    let on = button.toggled == Some(true);
    if on {
        painter.rect(
            button.rect,
            CornerRadius::same(3),
            OPTION_BACKGROUND,
            Stroke::new(1.0, OPTION_BORDER),
            StrokeKind::Inside,
        );
    } else if button.enabled && response.hovered() {
        painter.rect_filled(button.rect, CornerRadius::same(5), HOVER);
    }
    let color = match (button.enabled, on) {
        (false, _) => DISABLED,
        (true, true) => Color32::WHITE,
        (true, false) => FOREGROUND,
    };
    paint_icon(&painter, button.icon, button.rect.center(), color);
    button.enabled && response.clicked()
}

fn paint_icon(painter: &egui::Painter, icon: Icon, center: Pos2, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let line = |points: &[Vec2]| {
        painter.add(Shape::line(
            points.iter().map(|&point| center + point).collect(),
            stroke,
        ));
    };
    match icon {
        Icon::Text(text) => {
            painter.text(
                center,
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(COUNT_SIZE),
                color,
            );
        }
        Icon::Up | Icon::Down => {
            let sign = if matches!(icon, Icon::Up) { -1.0 } else { 1.0 };
            line(&[vec2(0.0, -5.0 * sign), vec2(0.0, 5.0 * sign)]);
            line(&[vec2(-4.0, sign), vec2(0.0, 5.0 * sign), vec2(4.0, sign)]);
        }
        Icon::Close => {
            line(&[vec2(-4.0, -4.0), vec2(4.0, 4.0)]);
            line(&[vec2(-4.0, 4.0), vec2(4.0, -4.0)]);
        }
        Icon::Chevron(false) => line(&[vec2(-2.0, -4.0), vec2(2.0, 0.0), vec2(-2.0, 4.0)]),
        Icon::Chevron(true) => line(&[vec2(-4.0, -2.0), vec2(0.0, 2.0), vec2(4.0, -2.0)]),
        Icon::Replace => {
            line(&[vec2(-5.0, -5.0), vec2(-5.0, 2.0), vec2(5.0, 2.0)]);
            line(&[vec2(2.0, -1.0), vec2(5.0, 2.0), vec2(2.0, 5.0)]);
        }
        Icon::ReplaceAll => {
            line(&[vec2(-6.0, -6.0), vec2(-6.0, -1.0), vec2(5.0, -1.0)]);
            line(&[vec2(-3.0, 1.0), vec2(-3.0, 4.0), vec2(5.0, 4.0)]);
            line(&[vec2(2.0, 1.0), vec2(5.0, 4.0), vec2(2.0, 7.0)]);
        }
    }
}
