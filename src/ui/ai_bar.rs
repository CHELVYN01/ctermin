//! The AI bar, opened with Ctrl+K.

use eframe::egui::{self, Align2, Area, Frame, Id, Key, Order, ScrollArea, TextEdit};

use crate::layout::PaneId;

const BAR_WIDTH: f32 = 620.0;
const BOTTOM_GAP: f32 = 16.0;
const MAX_TEXT_HEIGHT: f32 = 260.0;

/// The kind of request waiting for an answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RequestKind {
    Suggest,
    Explain,
}

#[derive(PartialEq, Eq, Debug)]
pub enum AiPhase {
    Input,
    /// Terminal output about to be sent, waiting for the user's approval.
    Preview(String),
    Waiting(RequestKind),
    /// A dangerous command waiting for explicit confirmation.
    Confirm {
        command: String,
        reason: &'static str,
    },
    Explanation(String),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiBarAction {
    Close,
    Back,
    Suggest,
    PreviewOutput,
    SendOutput,
    InsertConfirmed,
    OpenSettings,
}

pub struct AiBar {
    /// The target pane, set when the bar opens. Moving focus does not change it.
    pub target: PaneId,
    pub input: String,
    pub phase: AiPhase,
    pub request_id: Option<u64>,
    wants_focus: bool,
}

impl AiBar {
    pub fn new(target: PaneId) -> Self {
        Self {
            target,
            input: String::new(),
            phase: AiPhase::Input,
            request_id: None,
            wants_focus: true,
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        id: Id,
        provider: &str,
        target_label: &str,
    ) -> Option<AiBarAction> {
        let mut action = None;
        Area::new(id)
            .order(Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, -BOTTOM_GAP))
            .show(ctx, |ui| {
                Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(BAR_WIDTH.min(ctx.content_rect().width() - 48.0));
                    action = self.contents(ui, provider, target_label);
                });
            });

        // Escape always closes, even while waiting: that also cancels the request.
        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            action = Some(AiBarAction::Close);
        }
        action
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        provider: &str,
        target_label: &str,
    ) -> Option<AiBarAction> {
        match &self.phase {
            AiPhase::Input => self.input_view(ui, provider, target_label),
            AiPhase::Preview(output) => preview_view(ui, output),
            AiPhase::Waiting(_) => waiting_view(ui, provider),
            AiPhase::Confirm { command, reason } => confirm_view(ui, command, reason),
            AiPhase::Explanation(text) => result_view(ui, text),
            AiPhase::Failed(message) => {
                ui.colored_label(ui.visuals().error_fg_color, message.as_str());
                back_or_close(ui)
            }
        }
    }

    fn input_view(
        &mut self,
        ui: &mut egui::Ui,
        provider: &str,
        target_label: &str,
    ) -> Option<AiBarAction> {
        let mut action = None;
        ui.small(format!(
            "Ask AI about {target_label}. Commands never run automatically."
        ));
        let response = ui.add(
            TextEdit::singleline(&mut self.input)
                .hint_text("Example: check disk usage")
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut self.wants_focus) {
            response.request_focus();
        }
        let submitted = response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        if submitted && !self.input.trim().is_empty() {
            action = Some(AiBarAction::Suggest);
        }
        ui.horizontal(|ui| {
            if ui.small_button("Explain last output").clicked() {
                action = Some(AiBarAction::PreviewOutput);
            }
            if ui.small_button("AI Settings").clicked() {
                action = Some(AiBarAction::OpenSettings);
            }
            ui.small(provider);
        });
        action
    }
}

fn scrolling_text(ui: &mut egui::Ui, text: &str) {
    ScrollArea::vertical()
        .max_height(MAX_TEXT_HEIGHT)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(text).selectable(true));
        });
}

fn preview_view(ui: &mut egui::Ui, output: &str) -> Option<AiBarAction> {
    let mut action = None;
    ui.label("This text will be sent to the AI. Check that it contains no secrets.");
    ui.separator();
    ScrollArea::vertical()
        .max_height(MAX_TEXT_HEIGHT)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(egui::RichText::new(output).monospace()).selectable(true));
        });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Send to AI").clicked() {
            action = Some(AiBarAction::SendOutput);
        }
        if ui.button("Cancel").clicked() {
            action = Some(AiBarAction::Back);
        }
    });
    action
}

fn waiting_view(ui: &mut egui::Ui, provider: &str) -> Option<AiBarAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(format!("AI is thinking ({provider})"));
        if ui.small_button("Cancel").clicked() {
            action = Some(AiBarAction::Close);
        }
    });
    action
}

fn confirm_view(ui: &mut egui::Ui, command: &str, reason: &str) -> Option<AiBarAction> {
    let mut action = None;
    ui.colored_label(ui.visuals().warn_fg_color, "This command is dangerous");
    ui.label(reason);
    ui.add(egui::Label::new(egui::RichText::new(command).monospace()).selectable(true));
    ui.small(
        "The command is only inserted into the terminal and still waits for you to press Enter.",
    );
    ui.horizontal(|ui| {
        if ui.button("Insert anyway").clicked() {
            action = Some(AiBarAction::InsertConfirmed);
        }
        if ui.button("Cancel").clicked() {
            action = Some(AiBarAction::Back);
        }
    });
    action
}

fn result_view(ui: &mut egui::Ui, text: &str) -> Option<AiBarAction> {
    scrolling_text(ui, text);
    back_or_close(ui)
}

fn back_or_close(ui: &mut egui::Ui) -> Option<AiBarAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        if ui.button("Ask again").clicked() {
            action = Some(AiBarAction::Back);
        }
        if ui.button("Close").clicked() {
            action = Some(AiBarAction::Close);
        }
    });
    action
}
