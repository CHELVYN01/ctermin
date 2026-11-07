//! Sidebar server manager.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, pos2, vec2, Color32, RichText, ScrollArea, Sense, Shape, Stroke};

use crate::ssh::model::{Folder, FolderId, Server, ServerId, ServerStore};

pub const SIDEBAR_WIDTH: f32 = 230.0;

const BACKGROUND: Color32 = Color32::from_rgb(0x16, 0x16, 0x16);
const ACTIVE_COLOR: Color32 = Color32::from_rgb(0x16, 0xc6, 0x0c);
const DISCONNECTED_COLOR: Color32 = Color32::from_rgb(0xe7, 0x48, 0x56);
const IDLE_COLOR: Color32 = Color32::from_rgb(0x66, 0x66, 0x66);
const MARGIN: f32 = 8.0;
const ICON_SIZE: f32 = 14.0;
const ARROW_RADIUS: f32 = 4.0;
const DOT_RADIUS: f32 = 4.0;
const ICON_COLOR: Color32 = Color32::from_rgb(0xaa, 0xaa, 0xaa);
const ICON_HOVER_COLOR: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);

/// Server status based on the panes connected to it.
///
/// `Active` means ssh.exe is still running, which includes waiting for login.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ServerStatus {
    Active,
    Disconnected,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SidebarAction {
    Connect(ServerId),
    Reconnect(ServerId),
    ConnectFolder(FolderId),
    AddServer(Option<FolderId>),
    EditServer(ServerId),
    DeleteServer(ServerId),
    AddFolder,
    RenameFolder(FolderId),
    DeleteFolder(FolderId),
    Import,
}

#[derive(Default)]
pub struct Sidebar {
    collapsed: HashSet<FolderId>,
}

impl Sidebar {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &ServerStore,
        statuses: &HashMap<ServerId, ServerStatus>,
        notice: Option<&str>,
    ) -> Option<SidebarAction> {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, BACKGROUND);
        let mut action = None;

        let inner = rect.shrink(MARGIN);
        let builder = egui::UiBuilder::new().max_rect(inner);
        ui.scope_builder(builder, |ui| {
            ui.add_space(2.0);
            ui.label(RichText::new("SERVERS").small().strong().weak());
            ui.add_space(2.0);
            let button_width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            ui.horizontal(|ui| {
                let size = vec2(button_width, 26.0);
                if ui.add_sized(size, egui::Button::new("+ Server")).clicked() {
                    action = Some(SidebarAction::AddServer(None));
                }
                if ui.add_sized(size, egui::Button::new("+ Folder")).clicked() {
                    action = Some(SidebarAction::AddFolder);
                }
            });
            if ui
                .add_sized(
                    vec2(ui.available_width(), 24.0),
                    egui::Button::new("Import from ~/.ssh/config"),
                )
                .clicked()
            {
                action = Some(SidebarAction::Import);
            }
            if let Some(notice) = notice {
                ui.small(notice);
            }
            ui.separator();

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for folder in &store.folders {
                        self.folder_section(ui, store, folder, statuses, &mut action);
                    }
                    for server in store.servers_in(None) {
                        server_row(ui, server, statuses, &mut action);
                    }
                    if store.folders.is_empty() && store.servers.is_empty() {
                        ui.weak("No servers yet.");
                    }
                });
        });
        action
    }

    fn folder_section(
        &mut self,
        ui: &mut egui::Ui,
        store: &ServerStore,
        folder: &Folder,
        statuses: &HashMap<ServerId, ServerStatus>,
        action: &mut Option<SidebarAction>,
    ) {
        let collapsed = self.collapsed.contains(&folder.id);
        ui.horizontal(|ui| {
            if arrow_button(ui, collapsed) {
                if collapsed {
                    self.collapsed.remove(&folder.id);
                } else {
                    self.collapsed.insert(folder.id);
                }
            }
            let response = ui
                .selectable_label(false, &folder.name)
                .on_hover_text("Click to open every server in this folder");
            if response.clicked() {
                *action = Some(SidebarAction::ConnectFolder(folder.id));
            }
            response.context_menu(|ui| {
                let mut choose = |ui: &mut egui::Ui, label: &str, chosen: SidebarAction| {
                    if ui.button(label).clicked() {
                        *action = Some(chosen);
                        ui.close();
                    }
                };
                choose(ui, "Open all", SidebarAction::ConnectFolder(folder.id));
                choose(
                    ui,
                    "Add server here",
                    SidebarAction::AddServer(Some(folder.id)),
                );
                choose(ui, "Rename", SidebarAction::RenameFolder(folder.id));
                choose(ui, "Delete folder", SidebarAction::DeleteFolder(folder.id));
            });
        });

        if !collapsed {
            ui.indent(folder.id, |ui| {
                for server in store.servers_in(Some(folder.id)) {
                    server_row(ui, server, statuses, action);
                }
            });
        }
    }
}

fn server_row(
    ui: &mut egui::Ui,
    server: &Server,
    statuses: &HashMap<ServerId, ServerStatus>,
    action: &mut Option<SidebarAction>,
) {
    let status = statuses.get(&server.id).copied();
    let color = match status {
        Some(ServerStatus::Active) => ACTIVE_COLOR,
        Some(ServerStatus::Disconnected) => DISCONNECTED_COLOR,
        None => IDLE_COLOR,
    };
    ui.horizontal(|ui| {
        status_dot(ui, color);
        let response = ui
            .selectable_label(false, &server.name)
            .on_hover_text(&server.host);
        if response.clicked() {
            *action = Some(SidebarAction::Connect(server.id));
        }
        response.context_menu(|ui| {
            let mut choose = |ui: &mut egui::Ui, label: &str, chosen: SidebarAction| {
                if ui.button(label).clicked() {
                    *action = Some(chosen);
                    ui.close();
                }
            };
            choose(ui, "Connect", SidebarAction::Connect(server.id));
            if status == Some(ServerStatus::Disconnected) {
                choose(ui, "Reconnect", SidebarAction::Reconnect(server.id));
            }
            choose(ui, "Edit", SidebarAction::EditServer(server.id));
            choose(ui, "Delete", SidebarAction::DeleteServer(server.id));
        });
    });
}

/// Icons are drawn as shapes because the default font has no arrow or circle glyphs.
fn arrow_button(ui: &mut egui::Ui, collapsed: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ICON_SIZE, ICON_SIZE), Sense::click());
    let center = rect.center();
    let radius = ARROW_RADIUS;
    let points = if collapsed {
        vec![
            pos2(center.x - radius / 2.0, center.y - radius),
            pos2(center.x + radius, center.y),
            pos2(center.x - radius / 2.0, center.y + radius),
        ]
    } else {
        vec![
            pos2(center.x - radius, center.y - radius / 2.0),
            pos2(center.x + radius, center.y - radius / 2.0),
            pos2(center.x, center.y + radius),
        ]
    };
    let color = if response.hovered() {
        ICON_HOVER_COLOR
    } else {
        ICON_COLOR
    };
    ui.painter()
        .add(Shape::convex_polygon(points, color, Stroke::NONE));
    response.clicked()
}

fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ICON_SIZE, ICON_SIZE), Sense::hover());
    ui.painter().circle_filled(rect.center(), DOT_RADIUS, color);
}
