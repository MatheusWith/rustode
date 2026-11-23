//! The disk-conflict prompt: saving a document whose file changed under its
//! unsaved edits asks what to do instead of failing. The workspace decides
//! the sentence and the choices
//! ([`Workspace::conflict_prompt`](rustode_workspace::Workspace::conflict_prompt));
//! each button runs a registered command, the same ones the palette lists.

use eframe::egui::{
    self, Align2, Color32, Context, Frame, Id, Key, Modifiers, Order, Stroke, Vec2,
};
use rustode_workspace::ConflictPrompt;

const BACKGROUND: Color32 = Color32::from_rgb(0x25, 0x25, 0x26);
const BORDER: Color32 = Color32::from_rgb(0x45, 0x45, 0x45);
const WIDTH: f32 = 360.0;

/// What the user did with the prompt this frame.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    /// Nothing yet.
    None,
    /// Run this command of the registry.
    Run(&'static str),
    /// Close the prompt and keep editing.
    Dismiss,
}

/// Draws the prompt and returns what the user chose.
pub(crate) fn show(ctx: &Context, prompt: &ConflictPrompt) -> Reply {
    let mut reply = Reply::None;
    egui::Area::new(Id::new("rustode-conflict-prompt"))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            Frame::new()
                .fill(BACKGROUND)
                .stroke(Stroke::new(1.0, BORDER))
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.set_width(WIDTH);
                    ui.label(prompt.message.as_str());
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        for choice in &prompt.choices {
                            if ui.button(choice.label).clicked() {
                                reply = Reply::Run(choice.command);
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            reply = Reply::Dismiss;
                        }
                    });
                });
        });
    if reply == Reply::None && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        reply = Reply::Dismiss;
    }
    reply
}
