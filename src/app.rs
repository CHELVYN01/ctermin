use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, pos2, Id, Key, Rect, UiBuilder, ViewportCommand};
use portable_pty::CommandBuilder;

use crate::ai::client::{AiClient, AiResult};
use crate::ai::danger::danger_reason;
use crate::ai::prompt::{clean_suggestion, explain_prompt, suggest_prompt, AiPrompt, PaneContext};
use crate::ai::settings::ProviderKind;
use crate::config::{
    ConfigFile, ServerRepository, SessionFile, CONFIG_FILE, SERVERS_FILE, SESSION_FILE,
};
use crate::layout::{Axis, PaneId, PaneSource, SavedSession, Tabs, DIVIDER_THICKNESS};
use crate::ssh::command::ssh_command;
use crate::ssh::model::{AuthMethod, FolderId, Server, ServerId};
use crate::ssh::{credentials, import};
use crate::terminal::{font_definitions, GridSize, ShellKind, TerminalSession, TerminalView};
use crate::ui::ai_bar::{AiBar, AiBarAction, AiPhase, RequestKind};
use crate::ui::dialogs::{
    self, AiSettingsForm, DialogResult, FolderForm, ServerForm, SettingsForm,
};
use crate::ui::shortcuts::{self, Keymap, Shortcut};
use crate::ui::sidebar::{ServerStatus, Sidebar, SidebarAction, SIDEBAR_WIDTH};
use crate::ui::split_view;
use crate::ui::tab_bar::{self, NewTabMenu, ServerGroup, TabAction, TabItem, BAR_HEIGHT};

const INITIAL_COLUMNS: usize = 80;
const INITIAL_LINES: usize = 24;
const APP_NAME: &str = "ctermin";
const FAILED_TAB_TITLE: &str = "Failed";
/// How many of the last lines are offered for the AI to explain.
const EXPLAIN_LINES: usize = 40;
const TARGET_CLOSED: &str = "The target pane is no longer active.";

struct Pane {
    source: PaneSource,
    /// The terminal session, or an error message if it failed to start.
    session: Result<TerminalSession, String>,
}

enum Dialog {
    Server(ServerForm),
    Folder(FolderForm),
    DeleteServer(ServerId, String),
    DeleteFolder(FolderId, String),
    AiSettings(AiSettingsForm),
    Settings(SettingsForm),
}

/// Main application state.
pub struct CterminApp {
    tabs: Tabs,
    panes: HashMap<PaneId, Pane>,
    view: TerminalView,
    window_title: String,
    repository: ServerRepository,
    sidebar: Sidebar,
    sidebar_visible: bool,
    dialog: Option<Dialog>,
    notice: Option<String>,
    config: ConfigFile,
    session_file: SessionFile,
    keymap: Keymap,
    ai: AiClient,
    ai_bar: Option<AiBar>,
}

impl CterminApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let (repository, server_warning) = ServerRepository::open_default(SERVERS_FILE);
        let (config, config_warning) = ConfigFile::open_default(CONFIG_FILE);
        let (session_file, session_warning) = SessionFile::open_default(SESSION_FILE);
        let mut app = Self::with_parts(ctx, repository, config, session_file);
        app.notice = server_warning
            .or(config_warning)
            .or(session_warning)
            .or(app.notice.take());
        app
    }

    fn with_parts(
        ctx: &egui::Context,
        repository: ServerRepository,
        config: ConfigFile,
        session_file: SessionFile,
    ) -> Self {
        ctx.set_theme(egui::Theme::Dark);
        crate::ui::theme::apply(ctx);
        // Ctrl+Plus and Ctrl+Minus must reach the terminal, not change egui's zoom.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        let (keymap, invalid_shortcuts) = Keymap::from_settings(&config.data.shortcuts);
        let font_size = config.data.appearance.clamped_font_size();
        let mut app = Self {
            tabs: Tabs::default(),
            panes: HashMap::new(),
            view: TerminalView::new(font_size),
            window_title: String::new(),
            repository,
            sidebar: Sidebar::default(),
            sidebar_visible: true,
            dialog: None,
            notice: None,
            config,
            session_file,
            keymap,
            ai: AiClient::new(),
            ai_bar: None,
        };
        app.apply_font(ctx);
        if !invalid_shortcuts.is_empty() {
            app.notice = Some(format!(
                "Invalid shortcuts ignored: {}",
                invalid_shortcuts.join(", ")
            ));
        }
        app.restore_tabs(ctx);
        app
    }

    fn default_source(&self) -> PaneSource {
        PaneSource::Shell {
            shell: self.config.data.terminal.default_shell,
        }
    }

    /// Brings back the last session, or opens the default shell.
    fn restore_tabs(&mut self, ctx: &egui::Context) {
        if self.config.data.terminal.restore_session {
            let saved = self.session_file.data.clone();
            for tab in &saved.tabs {
                for (pane, source) in self.tabs.open_saved(&tab.root) {
                    let source = self.usable_source(source);
                    let spawned = self.spawn_pane(ctx, source);
                    self.panes.insert(pane, spawned);
                }
            }
            self.tabs.select(saved.active);
        }
        if self.tabs.is_empty() {
            self.open_tab(ctx, self.default_source());
        }
    }

    /// A saved pane for a server that no longer exists opens the default shell instead.
    fn usable_source(&self, source: PaneSource) -> PaneSource {
        match source {
            PaneSource::Server { id } if self.repository.data.server(id).is_none() => {
                self.default_source()
            }
            source => source,
        }
    }

    fn session_snapshot(&self) -> SavedSession {
        let fallback = self.default_source();
        self.tabs
            .snapshot(|pane| self.panes.get(&pane).map_or(fallback, |pane| pane.source))
    }

    fn apply_font(&mut self, ctx: &egui::Context) {
        match font_definitions(self.config.data.appearance.font) {
            Ok(definitions) => ctx.set_fonts(definitions),
            Err(message) => self.notice = Some(message),
        }
    }

    /// Applies settings to everything already open, without a restart.
    fn apply_settings(&mut self, ctx: &egui::Context, form: SettingsForm) {
        let font_changed = form.appearance.font != self.config.data.appearance.font;
        self.config.data.appearance = form.appearance;
        self.config.data.terminal = form.terminal;
        if font_changed {
            self.apply_font(ctx);
        }
        let appearance = &self.config.data.appearance;
        self.view.set_font_size(appearance.clamped_font_size());
        let scheme = appearance.color_scheme;
        for session in self
            .panes
            .values_mut()
            .filter_map(|pane| pane.session.as_mut().ok())
        {
            session.set_color_scheme(scheme);
        }
        if let Err(err) = self.config.save() {
            self.notice = Some(format!("{err:#}"));
        }
    }

    fn open_settings(&mut self) {
        let form = SettingsForm::new(&self.config.data, self.config.path_label());
        self.dialog = Some(Dialog::Settings(form));
    }

    fn new_tab_menu(&self) -> NewTabMenu {
        NewTabMenu {
            shells: ShellKind::available(self.config.data.terminal.default_shell),
            groups: self.server_groups(),
        }
    }

    fn open_tab(&mut self, ctx: &egui::Context, source: PaneSource) {
        let pane = self.tabs.open();
        let spawned = self.spawn_pane(ctx, source);
        self.panes.insert(pane, spawned);
    }

    fn split_focused_pane(&mut self, ctx: &egui::Context, axis: Axis) {
        let source = self.focused_source();
        if let Some(pane) = self.tabs.split_focused(axis) {
            let spawned = self.spawn_pane(ctx, source);
            self.panes.insert(pane, spawned);
        }
    }

    /// Dropping a session kills the process and stops its PTY threads.
    fn close_tab(&mut self, index: usize) {
        for pane in self.tabs.close(index) {
            self.panes.remove(&pane);
        }
    }

    fn close_focused_pane(&mut self) {
        if let Some(pane) = self.tabs.close_focused() {
            self.panes.remove(&pane);
        }
    }

    fn focused_source(&self) -> PaneSource {
        self.tabs
            .active()
            .and_then(|pane| self.panes.get(&pane))
            .map_or(self.default_source(), |pane| pane.source)
    }

    fn spawn_pane(&self, ctx: &egui::Context, source: PaneSource) -> Pane {
        let session = match source {
            PaneSource::Shell { shell } => self.spawn_session(ctx, shell.command()),
            PaneSource::Server { id } => match self.repository.data.server(id) {
                Some(server) => self.spawn_server(ctx, server),
                None => Err("This server has been deleted.".to_owned()),
            },
        };
        Pane { source, session }
    }

    fn spawn_server(
        &self,
        ctx: &egui::Context,
        server: &Server,
    ) -> Result<TerminalSession, String> {
        let command = ssh_command(server, askpass_program(server));
        let mut session = self.spawn_session(ctx, command)?;
        session.set_server_name(&server.name);
        Ok(session)
    }

    fn spawn_session(
        &self,
        ctx: &egui::Context,
        command: CommandBuilder,
    ) -> Result<TerminalSession, String> {
        let ctx = ctx.clone();
        let notify = Arc::new(move || ctx.request_repaint());
        let size = GridSize::new(INITIAL_COLUMNS, INITIAL_LINES);
        let mut session =
            TerminalSession::spawn(command, size, notify).map_err(|err| format!("{err:#}"))?;
        session.set_color_scheme(self.config.data.appearance.color_scheme);
        Ok(session)
    }

    /// Every pane is processed, not just visible ones: an unread pane would fill its
    /// output queue and stall its process.
    fn pump_all(&mut self) -> Option<Duration> {
        self.panes
            .values_mut()
            .filter_map(|pane| pane.session.as_mut().ok())
            .filter_map(TerminalSession::pump)
            .min()
    }

    fn restart_active_if_requested(&mut self, ctx: &egui::Context) {
        let Some(pane) = self.tabs.active() else {
            return;
        };
        if self.dialog.is_none()
            && self.ai_bar.is_none()
            && self.pane_exited(pane)
            && ctx.input(|input| input.key_pressed(Key::Enter))
        {
            self.respawn(ctx, pane);
        }
    }

    fn pane_exited(&self, pane: PaneId) -> bool {
        matches!(self.panes.get(&pane), Some(Pane { session: Ok(session), .. }) if session.exited())
    }

    fn respawn(&mut self, ctx: &egui::Context, pane: PaneId) {
        if let Some(source) = self.panes.get(&pane).map(|pane| pane.source) {
            let spawned = self.spawn_pane(ctx, source);
            self.panes.insert(pane, spawned);
        }
    }

    fn reconnect_server(&mut self, ctx: &egui::Context, id: ServerId) {
        let exited: Vec<PaneId> = self
            .panes
            .iter()
            .filter(|(_, pane)| pane.source == PaneSource::Server { id })
            .map(|(pane, _)| *pane)
            .filter(|pane| self.pane_exited(*pane))
            .collect();
        for pane in exited {
            self.respawn(ctx, pane);
        }
    }

    fn connect_folder(&mut self, ctx: &egui::Context, folder: FolderId) {
        let servers: Vec<ServerId> = self
            .repository
            .data
            .servers_in(Some(folder))
            .map(|server| server.id)
            .collect();
        if servers.is_empty() {
            self.notice = Some("This folder has no servers yet.".to_owned());
            return;
        }
        for (pane, server) in self.tabs.open_group(servers.len()).into_iter().zip(servers) {
            let spawned = self.spawn_pane(ctx, PaneSource::Server { id: server });
            self.panes.insert(pane, spawned);
        }
    }

    fn server_statuses(&self) -> HashMap<ServerId, ServerStatus> {
        let mut statuses = HashMap::new();
        for (pane_id, pane) in &self.panes {
            let PaneSource::Server { id: server } = pane.source else {
                continue;
            };
            if self.pane_exited(*pane_id) {
                statuses.entry(server).or_insert(ServerStatus::Disconnected);
            } else {
                statuses.insert(server, ServerStatus::Active);
            }
        }
        statuses
    }

    fn apply_shortcuts(&mut self, ctx: &egui::Context) {
        let keymap = self.keymap.clone();
        for shortcut in shortcuts::take(ctx, &keymap) {
            match shortcut {
                Shortcut::Open => self.open_tab(ctx, self.default_source()),
                Shortcut::Close => self.close_focused_pane(),
                Shortcut::Next => self.tabs.next(),
                Shortcut::Previous => self.tabs.previous(),
                Shortcut::Split(axis) => self.split_focused_pane(ctx, axis),
                Shortcut::Focus(direction) => self.tabs.focus_direction(direction),
                Shortcut::ToggleSidebar => self.sidebar_visible = !self.sidebar_visible,
                Shortcut::AiBar => self.open_ai_bar(),
                Shortcut::Settings => self.open_settings(),
            }
        }
    }

    fn apply_tab_action(&mut self, ctx: &egui::Context, action: TabAction) {
        match action {
            TabAction::Select(index) => self.tabs.select(index),
            TabAction::Close(index) => self.close_tab(index),
            TabAction::New => self.open_tab(ctx, self.default_source()),
            TabAction::OpenShell(shell) => self.open_tab(ctx, PaneSource::Shell { shell }),
            TabAction::OpenSettings => self.open_settings(),
            TabAction::Connect(id) => self.open_tab(ctx, PaneSource::Server { id }),
            TabAction::Duplicate(index) => {
                let source = self
                    .tabs
                    .focused_panes()
                    .get(index)
                    .and_then(|pane| self.panes.get(pane))
                    .map_or(self.default_source(), |pane| pane.source);
                let pane = self.tabs.open_after(index);
                let spawned = self.spawn_pane(ctx, source);
                self.panes.insert(pane, spawned);
            }
            TabAction::Split(index, axis) => {
                self.tabs.select(index);
                self.split_focused_pane(ctx, axis);
            }
            TabAction::ClosePane(index) => {
                self.tabs.select(index);
                self.close_focused_pane();
            }
        }
    }

    fn apply_sidebar_action(&mut self, ctx: &egui::Context, action: SidebarAction) {
        let store = &self.repository.data;
        match action {
            SidebarAction::Connect(id) => self.open_tab(ctx, PaneSource::Server { id }),
            SidebarAction::Reconnect(id) => self.reconnect_server(ctx, id),
            SidebarAction::ConnectFolder(id) => self.connect_folder(ctx, id),
            SidebarAction::AddServer(folder) => {
                self.dialog = Some(Dialog::Server(ServerForm::new(folder)));
            }
            SidebarAction::EditServer(id) => {
                if let Some(server) = store.server(id) {
                    let form = ServerForm::edit(server, credentials::has_password(id));
                    self.dialog = Some(Dialog::Server(form));
                }
            }
            SidebarAction::DeleteServer(id) => {
                if let Some(server) = store.server(id) {
                    let message = format!("Delete server \"{}\"?", server.name);
                    self.dialog = Some(Dialog::DeleteServer(id, message));
                }
            }
            SidebarAction::AddFolder => {
                self.dialog = Some(Dialog::Folder(FolderForm::new()));
            }
            SidebarAction::RenameFolder(id) => {
                if let Some(folder) = store.folder(id) {
                    self.dialog = Some(Dialog::Folder(FolderForm::edit(folder)));
                }
            }
            SidebarAction::DeleteFolder(id) => {
                if let Some(folder) = store.folder(id) {
                    let message = format!(
                        "Delete folder \"{}\"? The servers inside are kept.",
                        folder.name
                    );
                    self.dialog = Some(Dialog::DeleteFolder(id, message));
                }
            }
            SidebarAction::Import => self.import_ssh_config(),
        }
    }

    fn import_ssh_config(&mut self) {
        let Some(path) = ssh_config_path() else {
            self.notice = Some("User folder not found.".to_owned());
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.notice = Some(format!("Could not read {}.", path.display()));
            return;
        };
        let hosts = import::parse_config(&text);
        let summary = import::merge_hosts(&mut self.repository.data, hosts);
        self.notice = Some(format!(
            "{} servers imported, {} already present.",
            summary.added, summary.skipped
        ));
        self.persist();
    }

    fn persist(&mut self) {
        if let Err(err) = self.repository.save() {
            self.notice = Some(format!("{err:#}"));
        }
    }

    fn save_server_form(&mut self, form: &ServerForm) -> Result<(), String> {
        let id = form
            .editing
            .unwrap_or_else(|| self.repository.data.new_server_id());
        let validated = form.validate(id)?;
        match (&validated.server.auth, &validated.new_password) {
            (AuthMethod::Password, Some(password)) => {
                credentials::save_password(id, password).map_err(|err| format!("{err:#}"))?;
            }
            (AuthMethod::Password, None) => {}
            (AuthMethod::Key { .. }, _) => {
                credentials::delete_password(id).map_err(|err| format!("{err:#}"))?;
            }
        }
        self.repository.data.upsert_server(validated.server);
        self.persist();
        Ok(())
    }

    fn save_folder_form(&mut self, form: &FolderForm) -> Result<(), String> {
        let name = form.validated_name()?;
        match form.editing {
            Some(id) => self.repository.data.rename_folder(id, name),
            None => {
                self.repository.data.add_folder(name);
            }
        }
        self.persist();
        Ok(())
    }

    fn delete_server(&mut self, id: ServerId) {
        if let Err(err) = credentials::delete_password(id) {
            self.notice = Some(format!("{err:#}"));
        }
        self.repository.data.remove_server(id);
        self.persist();
    }

    fn show_dialog(&mut self, ctx: &egui::Context, id: Id) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        self.dialog = match dialog {
            Dialog::Server(mut form) => {
                let folders = &self.repository.data.folders;
                match dialogs::show_server_form(ctx, id, &mut form, folders) {
                    DialogResult::Open => Some(Dialog::Server(form)),
                    DialogResult::Cancelled => None,
                    DialogResult::Confirmed => match self.save_server_form(&form) {
                        Ok(()) => None,
                        Err(error) => {
                            form.error = Some(error);
                            Some(Dialog::Server(form))
                        }
                    },
                }
            }
            Dialog::Folder(mut form) => match dialogs::show_folder_form(ctx, id, &mut form) {
                DialogResult::Open => Some(Dialog::Folder(form)),
                DialogResult::Cancelled => None,
                DialogResult::Confirmed => match self.save_folder_form(&form) {
                    Ok(()) => None,
                    Err(error) => {
                        form.error = Some(error);
                        Some(Dialog::Folder(form))
                    }
                },
            },
            Dialog::DeleteServer(server, message) => {
                match dialogs::show_confirmation(ctx, id, &message) {
                    DialogResult::Open => Some(Dialog::DeleteServer(server, message)),
                    DialogResult::Cancelled => None,
                    DialogResult::Confirmed => {
                        self.delete_server(server);
                        None
                    }
                }
            }
            Dialog::Settings(mut form) => match dialogs::show_settings(ctx, id, &mut form) {
                DialogResult::Open => Some(Dialog::Settings(form)),
                DialogResult::Cancelled => None,
                DialogResult::Confirmed => {
                    self.apply_settings(ctx, form);
                    None
                }
            },
            Dialog::AiSettings(mut form) => match dialogs::show_ai_settings(ctx, id, &mut form) {
                DialogResult::Open => Some(Dialog::AiSettings(form)),
                DialogResult::Cancelled => None,
                DialogResult::Confirmed => match self.save_ai_settings(&form) {
                    Ok(()) => None,
                    Err(error) => {
                        form.error = Some(error);
                        Some(Dialog::AiSettings(form))
                    }
                },
            },
            Dialog::DeleteFolder(folder, message) => {
                match dialogs::show_confirmation(ctx, id, &message) {
                    DialogResult::Open => Some(Dialog::DeleteFolder(folder, message)),
                    DialogResult::Cancelled => None,
                    DialogResult::Confirmed => {
                        self.repository.data.remove_folder(folder);
                        self.persist();
                        None
                    }
                }
            }
        };
    }

    fn open_ai_bar(&mut self) {
        if let Some(pane) = self.tabs.active() {
            self.ai_bar = Some(AiBar::new(pane));
        }
    }

    fn close_ai_bar(&mut self) {
        self.ai.cancel();
        self.ai_bar = None;
    }

    fn provider_label(&self) -> &'static str {
        match self.config.data.ai.provider {
            ProviderKind::ClaudeCode => "Claude Code",
            ProviderKind::ApiKey => "API key",
        }
    }

    fn pane_label(&self, pane: PaneId) -> String {
        match self.panes.get(&pane) {
            Some(Pane {
                session: Ok(session),
                ..
            }) => session.title().to_owned(),
            _ => "this pane".to_owned(),
        }
    }

    /// Context for the AI. Only the server name is sent, not its address.
    fn pane_context(&self, pane: PaneId) -> Option<PaneContext> {
        let pane = self.panes.get(&pane)?;
        let session = pane.session.as_ref().ok()?;
        let environment = match pane.source {
            PaneSource::Shell { shell } => shell.environment().to_owned(),
            PaneSource::Server { .. } => format!(
                "SSH session to server \"{}\". Judge the server's OS from the prompt line; it is \
                 usually Linux. This is not the local Windows machine.",
                session.title()
            ),
        };
        Some(PaneContext {
            environment,
            prompt_line: session.prompt_line(),
        })
    }

    fn submit_ai(&mut self, ctx: &egui::Context, prompt: AiPrompt, kind: RequestKind) {
        let settings = self.config.data.ai.clone();
        let api_key = match settings.provider {
            ProviderKind::ClaudeCode => Ok(None),
            ProviderKind::ApiKey => credentials::load_api_key().map_err(|err| format!("{err:#}")),
        };
        let repaint = ctx.clone();
        let notify = Arc::new(move || repaint.request_repaint());
        let submitted =
            api_key.and_then(|api_key| self.ai.submit(prompt, settings, api_key, notify));
        if let Some(bar) = &mut self.ai_bar {
            match submitted {
                Ok(id) => {
                    bar.request_id = Some(id);
                    bar.phase = AiPhase::Waiting(kind);
                }
                Err(message) => bar.phase = AiPhase::Failed(message),
            }
        }
    }

    fn apply_ai_bar_action(&mut self, ctx: &egui::Context, action: AiBarAction) {
        let Some(target) = self.ai_bar.as_ref().map(|bar| bar.target) else {
            return;
        };
        let context = self.pane_context(target);
        let Some(bar) = &mut self.ai_bar else {
            return;
        };
        let session = self
            .panes
            .get(&target)
            .and_then(|pane| pane.session.as_ref().ok());
        match action {
            AiBarAction::Close => self.close_ai_bar(),
            AiBarAction::Back => bar.phase = AiPhase::Input,
            AiBarAction::OpenSettings => {
                self.close_ai_bar();
                let form = AiSettingsForm::new(&self.config.data.ai, credentials::has_api_key());
                self.dialog = Some(Dialog::AiSettings(form));
            }
            AiBarAction::PreviewOutput => {
                let output = session
                    .map(|session| session.recent_output(EXPLAIN_LINES))
                    .unwrap_or_default();
                bar.phase = if output.is_empty() {
                    AiPhase::Failed("There is no output to explain yet.".to_owned())
                } else {
                    AiPhase::Preview(output)
                };
            }
            AiBarAction::Suggest => {
                let request = bar.input.trim().to_owned();
                match context {
                    Some(context) => self.submit_ai(
                        ctx,
                        suggest_prompt(&request, &context),
                        RequestKind::Suggest,
                    ),
                    None => bar.phase = AiPhase::Failed(TARGET_CLOSED.to_owned()),
                }
            }
            AiBarAction::SendOutput => {
                let AiPhase::Preview(output) = &bar.phase else {
                    return;
                };
                match context {
                    Some(context) => {
                        let prompt = explain_prompt(output, &context);
                        self.submit_ai(ctx, prompt, RequestKind::Explain);
                    }
                    None => bar.phase = AiPhase::Failed(TARGET_CLOSED.to_owned()),
                }
            }
            AiBarAction::InsertConfirmed => {
                if let AiPhase::Confirm { command, .. } = &bar.phase {
                    let command = command.clone();
                    self.insert_ai_command(target, &command);
                }
            }
        }
    }

    fn insert_ai_command(&mut self, target: PaneId, command: &str) {
        let session = self
            .panes
            .get_mut(&target)
            .and_then(|pane| pane.session.as_mut().ok());
        match session {
            Some(session) if !session.exited() => {
                session.insert_command(command);
                self.ai_bar = None;
            }
            _ => {
                if let Some(bar) = &mut self.ai_bar {
                    bar.phase = AiPhase::Failed(TARGET_CLOSED.to_owned());
                }
            }
        }
    }

    fn poll_ai(&mut self) {
        while let Some((id, result)) = self.ai.try_result() {
            self.apply_ai_result(id, result);
        }
    }

    /// Only the answer to the latest request is used.
    fn apply_ai_result(&mut self, id: u64, result: AiResult) {
        let Some(bar) = &mut self.ai_bar else {
            return;
        };
        let AiPhase::Waiting(kind) = bar.phase else {
            return;
        };
        if bar.request_id != Some(id) {
            return;
        }
        let target = bar.target;
        match (kind, result) {
            (_, Err(message)) => bar.phase = AiPhase::Failed(message),
            (RequestKind::Explain, Ok(text)) => bar.phase = AiPhase::Explanation(text),
            (RequestKind::Suggest, Ok(text)) => match clean_suggestion(&text) {
                Err(message) => bar.phase = AiPhase::Failed(message),
                Ok(command) => match danger_reason(&command) {
                    Some(reason) => bar.phase = AiPhase::Confirm { command, reason },
                    None => self.insert_ai_command(target, &command),
                },
            },
        }
    }

    fn show_ai_bar(&mut self, ctx: &egui::Context, id: Id) {
        let Some(target) = self.ai_bar.as_ref().map(|bar| bar.target) else {
            return;
        };
        let provider = self.provider_label();
        let label = self.pane_label(target);
        let action = self
            .ai_bar
            .as_mut()
            .and_then(|bar| bar.show(ctx, id, provider, &label));
        if let Some(action) = action {
            self.apply_ai_bar_action(ctx, action);
        }
    }

    fn save_ai_settings(&mut self, form: &AiSettingsForm) -> Result<(), String> {
        form.validate()?;
        let key = form.api_key.trim();
        if !key.is_empty() {
            credentials::save_api_key(key).map_err(|err| format!("{err:#}"))?;
        } else if form.delete_key {
            credentials::delete_api_key().map_err(|err| format!("{err:#}"))?;
        }
        self.config.data.ai = form.settings();
        self.config.save().map_err(|err| format!("{err:#}"))
    }

    fn tab_titles(&self) -> Vec<String> {
        self.tabs
            .focused_panes()
            .iter()
            .map(|pane| match self.panes.get(pane) {
                Some(Pane {
                    session: Ok(session),
                    ..
                }) => session.title().to_owned(),
                _ => FAILED_TAB_TITLE.to_owned(),
            })
            .collect()
    }

    fn tab_items(&self) -> Vec<TabItem> {
        self.tab_titles()
            .into_iter()
            .zip(self.tabs.pane_counts())
            .map(|(title, pane_count)| TabItem { title, pane_count })
            .collect()
    }

    /// The server list for the new tab menu: by folder, then those without one.
    fn server_groups(&self) -> Vec<ServerGroup> {
        let store = &self.repository.data;
        let names = |folder| {
            store
                .servers_in(folder)
                .map(|server| (server.id, server.name.clone()))
                .collect::<Vec<_>>()
        };
        let in_folders = store.folders.iter().map(|folder| ServerGroup {
            title: Some(folder.name.clone()),
            servers: names(Some(folder.id)),
        });
        let without_folder = ServerGroup {
            title: None,
            servers: names(None),
        };
        in_folders
            .chain(std::iter::once(without_folder))
            .filter(|group| !group.servers.is_empty())
            .collect()
    }

    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let pane = self.tabs.active().and_then(|pane| self.panes.get(&pane));
        let title = match pane {
            Some(Pane {
                session: Ok(session),
                ..
            }) => format!("{} - {APP_NAME}", session.title()),
            _ => APP_NAME.to_owned(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let overlay_open = self.dialog.is_some() || self.ai_bar.is_some();
        if !overlay_open {
            self.apply_shortcuts(&ctx);
        }

        let window = ui.max_rect();
        let mut main = window;
        if self.sidebar_visible {
            let width = SIDEBAR_WIDTH.min(window.width() / 2.0);
            let right = window.left() + width;
            let area = Rect::from_min_max(window.min, pos2(right, window.bottom()));
            main.set_left(right);

            let statuses = self.server_statuses();
            let store = &self.repository.data;
            let notice = self.notice.as_deref();
            let sidebar = &mut self.sidebar;
            let action = ui
                .scope_builder(UiBuilder::new().max_rect(area), |ui| {
                    sidebar.show(ui, store, &statuses, notice)
                })
                .inner;
            if let Some(action) = action {
                self.apply_sidebar_action(&ctx, action);
            }
        }

        let bar = Rect::from_min_max(main.min, pos2(main.right(), main.top() + BAR_HEIGHT));
        let content = Rect::from_min_max(pos2(main.left(), bar.bottom()), main.max);

        let items = self.tab_items();
        let menu = self.new_tab_menu();
        let active = self.tabs.active_index();
        let action = ui
            .scope_builder(UiBuilder::new().max_rect(bar), |ui| {
                tab_bar::show(ui, &items, active, &menu)
            })
            .inner;
        if let Some(action) = action {
            self.apply_tab_action(&ctx, action);
        }

        if self.tabs.is_empty() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        self.show_panes(ui, content, !overlay_open);
        self.sync_window_title(&ctx);
        self.show_dialog(&ctx, ui.scope_id().with("dialog"));
        self.show_ai_bar(&ctx, ui.scope_id().with("ai_bar"));
    }

    /// `keyboard_enabled` is off while a dialog is open, so typing goes to the form.
    fn show_panes(&mut self, ui: &mut egui::Ui, content: Rect, keyboard_enabled: bool) {
        let Some(layout) = self.tabs.active_layout() else {
            return;
        };
        let arranged = layout.arrange(content, DIVIDER_THICKNESS);
        let dividers = layout.dividers(content);
        let focused = self.tabs.active();

        let base = ui.scope_id();
        let mut pressed = None;
        for (pane, rect) in &arranged {
            let keyboard_focus = keyboard_enabled && Some(*pane) == focused;
            let id = base.with(pane);
            let session = self.panes.get_mut(pane).map(|pane| &mut pane.session);
            let view = &mut self.view;
            let scope = UiBuilder::new().id_salt(pane).max_rect(*rect);
            let response = ui.scope_builder(scope, |ui| match session {
                Some(Ok(session)) => view.show(ui, id, session, keyboard_focus),
                Some(Err(message)) => {
                    ui.colored_label(ui.visuals().error_fg_color, message.as_str());
                    false
                }
                None => false,
            });
            if response.inner {
                pressed = Some(*pane);
            }
        }

        if let Some(change) = split_view::show_dividers(ui, &dividers) {
            if let Some(layout) = self.tabs.active_layout_mut() {
                layout.set_ratio(&change.path, change.ratio);
            }
        }

        // The focus marker is only useful with more than one pane.
        if arranged.len() > 1 {
            let focused_rect = arranged.iter().find(|(pane, _)| Some(*pane) == focused);
            if let Some((_, rect)) = focused_rect {
                split_view::paint_focus(ui, *rect);
            }
        }

        if let Some(pane) = pressed {
            self.tabs.focus(pane);
        }
    }
}

/// The askpass program is set only if a password is saved. Otherwise ssh.exe asks in the
/// terminal as usual.
fn askpass_program(server: &Server) -> Option<OsString> {
    let uses_password = server.auth == AuthMethod::Password;
    if uses_password && credentials::has_password(server.id) {
        std::env::current_exe().ok().map(PathBuf::into_os_string)
    } else {
        None
    }
}

fn ssh_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")?;
    Some(PathBuf::from(home).join(".ssh").join("config"))
}

impl eframe::App for CterminApp {
    // Output is processed here, not in `ui`, because `ui` is not called while the window
    // is minimized.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(delay) = self.pump_all() {
            ctx.request_repaint_after(delay);
        }
        self.poll_ai();
        self.restart_active_if_requested(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.session_file.data = self.session_snapshot();
        // Nowhere left to show an error while closing; the next start just opens fresh.
        let _ = self.session_file.save();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;
    use std::thread;
    use std::time::Instant;

    use alacritty_terminal::grid::Dimensions;
    use eframe::egui::{
        vec2, Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, ViewportId,
    };

    use super::*;
    use crate::ai::settings::AiSettings;
    use crate::terminal::ColorScheme;
    use crate::ui::tab_bar::test_positions;

    const WIDTH: f32 = 800.0;
    const HEIGHT: f32 = 600.0;
    const TIMEOUT: Duration = Duration::from_secs(20);

    fn test_config() -> ConfigFile {
        let mut config = ConfigFile::default();
        config.data.terminal.default_shell = ShellKind::Cmd;
        config
    }

    /// Runs the app without a window, in the same order as eframe.
    struct Harness {
        ctx: egui::Context,
        app: CterminApp,
        time: f64,
        size: egui::Vec2,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let mut app = CterminApp::with_parts(
                &ctx,
                ServerRepository::default(),
                test_config(),
                SessionFile::default(),
            );
            app.sidebar_visible = false;
            let mut harness = Self {
                ctx,
                app,
                time: 0.0,
                size: vec2(WIDTH, HEIGHT),
            };
            harness.frame(Vec::new());
            harness
        }

        fn frame(&mut self, events: Vec<Event>) -> FullOutput {
            self.time += 0.05;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..RawInput::default()
            };
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(input, |ui| {
                app.pump_all();
                app.restart_active_if_requested(ui.ctx());
                app.show(ui);
            });
            output.textures_delta.clear();
            output
        }

        fn press(&mut self, key: Key, modifiers: Modifiers) -> FullOutput {
            let event = |pressed| Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            };
            self.frame(vec![event(true), event(false)])
        }

        fn click(&mut self, pos: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(pos)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.frame(Vec::new());
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![button(from, true)]);
            self.frame(vec![Event::PointerMoved(to)]);
            self.frame(vec![Event::PointerMoved(to)]);
            self.frame(vec![button(to, false)]);
        }

        fn right_click(&mut self, pos: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos,
                button: PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(pos)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.frame(Vec::new());
        }

        fn type_line(&mut self, line: &str) {
            self.frame(vec![Event::Paste(format!("{line}\n"))]);
        }

        fn wait_until(&mut self, done: impl Fn(&CterminApp) -> bool) {
            let start = Instant::now();
            while !done(&self.app) {
                assert!(start.elapsed() < TIMEOUT, "timed out");
                self.frame(Vec::new());
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn count(&self) -> usize {
            self.app.tabs.focused_panes().len()
        }

        fn active_session(&self) -> &TerminalSession {
            active_session(&self.app)
        }

        fn thread_tokens(&self) -> Vec<Weak<()>> {
            self.app
                .panes
                .values()
                .filter_map(|pane| pane.session.as_ref().ok())
                .map(TerminalSession::threads_alive)
                .collect()
        }
    }

    fn active_session(app: &CterminApp) -> &TerminalSession {
        app.tabs
            .active()
            .and_then(|pane| app.panes.get(&pane))
            .and_then(|pane| pane.session.as_ref().ok())
            .expect("the active tab must have a session")
    }

    fn screen(app: &CterminApp) -> String {
        active_session(app).screen_text()
    }

    fn wait_threads_stopped(tokens: &[Weak<()>]) {
        let start = Instant::now();
        while tokens.iter().any(|token| token.upgrade().is_some()) {
            assert!(start.elapsed() < TIMEOUT, "PTY threads are still alive");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn ctrl_t_opens_and_ctrl_w_closes() {
        let mut harness = Harness::new();
        assert_eq!(harness.count(), 1);

        harness.press(Key::T, Modifiers::CTRL);
        assert_eq!(harness.count(), 2);
        assert_eq!(harness.app.tabs.active_index(), 1);
        assert_eq!(harness.app.panes.len(), 2);

        harness.press(Key::W, Modifiers::CTRL);
        assert_eq!(harness.count(), 1);
        assert_eq!(harness.app.panes.len(), 1);
    }

    #[test]
    fn ctrl_tab_moves_and_wraps() {
        let mut harness = Harness::new();
        harness.press(Key::T, Modifiers::CTRL);
        harness.press(Key::T, Modifiers::CTRL);

        harness.press(Key::Tab, Modifiers::CTRL);
        assert_eq!(harness.app.tabs.active_index(), 0);
        harness.press(Key::Tab, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(harness.app.tabs.active_index(), 2);
    }

    #[test]
    fn each_tab_has_its_own_session() {
        let mut harness = Harness::new();
        harness.type_line("echo first-tab-content");
        harness.wait_until(|app| screen(app).matches("first-tab-content").count() >= 2);

        harness.press(Key::T, Modifiers::CTRL);
        harness.wait_until(|app| screen(app).contains('>'));
        assert!(!harness
            .active_session()
            .screen_text()
            .contains("first-tab-content"));

        harness.press(Key::Tab, Modifiers::CTRL);
        assert!(harness
            .active_session()
            .screen_text()
            .contains("first-tab-content"));
    }

    #[test]
    fn background_tabs_keep_running() {
        let mut harness = Harness::new();
        harness.type_line("ping -t 127.0.0.1");
        harness.wait_until(|app| screen(app).contains("127.0.0.1:"));
        let first = harness.app.tabs.focused_panes()[0];

        harness.press(Key::T, Modifiers::CTRL);
        let replies = move |app: &CterminApp| match app.panes.get(&first) {
            Some(Pane {
                session: Ok(session),
                ..
            }) => session.screen_text().matches("127.0.0.1:").count(),
            _ => 0,
        };
        let before = replies(&harness.app);
        harness.wait_until(move |app| replies(app) > before);
    }

    #[test]
    fn six_tabs_stop_cleanly_when_closed() {
        let mut harness = Harness::new();
        for _ in 0..5 {
            harness.press(Key::T, Modifiers::CTRL);
        }
        assert_eq!(harness.count(), 6);
        harness.wait_until(|app| screen(app).contains('>'));

        let tokens = harness.thread_tokens();
        assert_eq!(tokens.len(), 6);
        for _ in 0..6 {
            harness.press(Key::W, Modifiers::CTRL);
        }

        assert!(harness.app.panes.is_empty());
        wait_threads_stopped(&tokens);
    }

    #[test]
    fn closing_the_last_tab_closes_the_window() {
        let mut harness = Harness::new();
        let output = harness.press(Key::W, Modifiers::CTRL);
        let commands = &output.viewport_output[&ViewportId::ROOT].commands;
        assert!(commands.contains(&ViewportCommand::Close));
    }

    #[test]
    fn tab_bar_clicks() {
        let mut harness = Harness::new();

        harness.click(test_positions::new_button(WIDTH, 1));
        assert_eq!(harness.count(), 2);

        harness.click(test_positions::tab(WIDTH, 2, 0));
        assert_eq!(harness.app.tabs.active_index(), 0);

        harness.click(test_positions::close(WIDTH, 2, 1));
        assert_eq!(harness.count(), 1);
        assert_eq!(harness.app.tabs.active_index(), 0);
    }

    #[test]
    fn tab_key_does_not_move_focus_to_the_tab_bar() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        let focused = harness.ctx.memory(|memory| memory.focused());
        assert!(focused.is_some());

        for _ in 0..4 {
            harness.press(Key::Tab, Modifiers::NONE);
        }
        assert_eq!(harness.ctx.memory(|memory| memory.focused()), focused);
    }

    #[test]
    fn tab_and_window_titles_follow_the_session() {
        let mut harness = Harness::new();
        harness.type_line("title tab-title");
        harness.wait_until(|app| active_session(app).title().contains("tab-title"));

        // A new title is sent on the frame it is read, or on the next one.
        harness.frame(Vec::new());
        assert!(harness.app.tab_titles()[0].contains("tab-title"));
        assert!(harness.app.window_title.contains("tab-title"));
        assert!(harness.app.window_title.ends_with(APP_NAME));
    }

    #[test]
    fn enter_restarts_an_exited_shell() {
        let mut harness = Harness::new();
        harness.type_line("exit 3");
        harness.wait_until(|app| active_session(app).exited());
        assert!(harness.active_session().screen_text().contains("code 3"));

        let old = harness.thread_tokens();
        let pane = harness.app.tabs.active();
        harness.press(Key::Enter, Modifiers::NONE);

        assert_eq!(harness.app.tabs.active(), pane);
        assert!(!harness.active_session().exited());
        wait_threads_stopped(&old);
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers::CTRL | Modifiers::SHIFT
    }

    fn pane_count(app: &CterminApp) -> usize {
        app.tabs
            .active_layout()
            .map_or(0, |layout| layout.panes().len())
    }

    fn content() -> Rect {
        Rect::from_min_max(pos2(0.0, BAR_HEIGHT), pos2(WIDTH, HEIGHT))
    }

    fn pane_rect(app: &CterminApp, pane: PaneId) -> Rect {
        let layout = app.tabs.active_layout().expect("has an active tab");
        layout
            .arrange(content(), DIVIDER_THICKNESS)
            .into_iter()
            .find(|(id, _)| *id == pane)
            .map(|(_, rect)| rect)
            .expect("pane is in the layout")
    }

    fn session_of(app: &CterminApp, pane: PaneId) -> &TerminalSession {
        match app.panes.get(&pane) {
            Some(Pane {
                session: Ok(session),
                ..
            }) => session,
            _ => panic!("pane must have a session"),
        }
    }

    #[test]
    fn ctrl_shift_d_and_e_create_nested_panes() {
        let mut harness = Harness::new();
        harness.press(Key::D, ctrl_shift());
        harness.press(Key::E, ctrl_shift());
        harness.press(Key::D, ctrl_shift());

        assert_eq!(pane_count(&harness.app), 4);
        assert_eq!(harness.app.panes.len(), 4);
        assert_eq!(harness.count(), 1, "still one tab");
    }

    #[test]
    fn grid_size_follows_the_pane() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("has a pane");
        let full = harness.active_session().term().columns();

        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());
        let right = harness.app.tabs.active().expect("has a pane");

        let left_columns = session_of(&harness.app, left).term().columns();
        let right_columns = session_of(&harness.app, right).term().columns();
        assert!(left_columns < full * 6 / 10, "{left_columns} of {full}");
        assert!(right_columns < full * 6 / 10, "{right_columns} of {full}");
    }

    #[test]
    fn keyboard_only_reaches_the_focused_pane() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("has a pane");
        harness.press(Key::D, ctrl_shift());
        let right = harness.app.tabs.active().expect("has a pane");

        harness.type_line("echo right-only");
        harness.wait_until(move |app| {
            session_of(app, right)
                .screen_text()
                .matches("right-only")
                .count()
                >= 2
        });
        assert!(!session_of(&harness.app, left)
            .screen_text()
            .contains("right-only"));
    }

    #[test]
    fn click_moves_focus() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("has a pane");
        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());

        let target = pane_rect(&harness.app, left).center();
        harness.click(target);
        assert_eq!(harness.app.tabs.active(), Some(left));

        harness.type_line("echo to-left");
        harness.wait_until(move |app| {
            session_of(app, left)
                .screen_text()
                .matches("to-left")
                .count()
                >= 2
        });
    }

    #[test]
    fn alt_arrow_moves_focus() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active();
        harness.press(Key::D, ctrl_shift());
        let right = harness.app.tabs.active();

        harness.press(Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(harness.app.tabs.active(), left);
        harness.press(Key::ArrowRight, Modifiers::ALT);
        assert_eq!(harness.app.tabs.active(), right);
    }

    #[test]
    fn ctrl_w_closes_the_pane_not_the_tab() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active();
        harness.press(Key::D, ctrl_shift());

        harness.press(Key::W, Modifiers::CTRL);

        assert_eq!(harness.count(), 1);
        assert_eq!(pane_count(&harness.app), 1);
        assert_eq!(harness.app.panes.len(), 1);
        assert_eq!(harness.app.tabs.active(), left);
    }

    #[test]
    fn remaining_pane_reclaims_its_space() {
        let mut harness = Harness::new();
        let full = harness.active_session().term().columns();
        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());
        harness.press(Key::W, Modifiers::CTRL);
        harness.frame(Vec::new());
        assert_eq!(harness.active_session().term().columns(), full);
    }

    #[test]
    fn closing_a_split_tab_stops_all_sessions() {
        let mut harness = Harness::new();
        let keep = harness.thread_tokens();
        harness.press(Key::T, Modifiers::CTRL);
        harness.press(Key::D, ctrl_shift());
        harness.press(Key::E, ctrl_shift());
        harness.press(Key::D, ctrl_shift());
        assert_eq!(harness.app.panes.len(), 5);

        let all = harness.thread_tokens();
        harness.app.close_tab(1);
        assert_eq!(harness.app.panes.len(), 1);

        let closed: Vec<_> = all
            .into_iter()
            .filter(|token| !token.ptr_eq(&keep[0]))
            .collect();
        assert_eq!(closed.len(), 4);
        wait_threads_stopped(&closed);
        assert!(keep[0].upgrade().is_some(), "other tabs stay alive");
    }

    #[test]
    fn dragging_a_divider_resizes_panes() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("has a pane");
        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());
        let before = session_of(&harness.app, left).term().columns();

        let edge = pane_rect(&harness.app, left).right() + 1.0;
        harness.drag(pos2(edge, 300.0), pos2(edge - 200.0, 300.0));
        harness.frame(Vec::new());

        let after = session_of(&harness.app, left).term().columns();
        assert!(after + 10 < before, "columns went from {before} to {after}");
    }

    #[test]
    fn mouse_wheel_follows_the_pointer_not_focus() {
        let mut harness = Harness::new();
        harness.type_line("for /L %i in (1,1,100) do @echo line-%i");
        harness.wait_until(|app| screen(app).contains("line-100"));
        let left = harness.app.tabs.active().expect("has a pane");
        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());
        let right = harness.app.tabs.active().expect("has a pane");

        let target = pane_rect(&harness.app, left).center();
        harness.frame(vec![Event::PointerMoved(target)]);
        harness.frame(vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        }]);

        let offset = |pane| {
            session_of(&harness.app, pane)
                .term()
                .grid()
                .display_offset()
        };
        assert!(offset(left) > 0);
        assert_eq!(offset(right), 0);
        assert_eq!(harness.app.tabs.active(), Some(right));
    }

    #[test]
    fn enter_only_restarts_the_focused_exited_pane() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("has a pane");
        harness.press(Key::D, ctrl_shift());
        harness.type_line("exit 0");
        harness.wait_until(|app| active_session(app).exited());

        harness.press(Key::Enter, Modifiers::NONE);
        assert!(!harness.active_session().exited());
        assert!(!session_of(&harness.app, left).exited());
        assert_eq!(pane_count(&harness.app), 2);
    }

    #[test]
    fn tiny_window_does_not_panic() {
        let mut harness = Harness::new();
        for _ in 0..4 {
            harness.press(Key::D, ctrl_shift());
            harness.press(Key::E, ctrl_shift());
        }
        harness.size = vec2(60.0, 40.0);
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        assert_eq!(pane_count(&harness.app), 9);
    }

    /// A server on a port that refuses connections: real ssh.exe runs and fails at once.
    fn add_refused_server(harness: &mut Harness, name: &str, folder: Option<FolderId>) -> ServerId {
        let store = &mut harness.app.repository.data;
        let id = store.new_server_id();
        store.upsert_server(Server {
            id,
            name: name.to_owned(),
            host: "127.0.0.1".to_owned(),
            port: 1,
            username: "tester".to_owned(),
            folder,
            auth: AuthMethod::Key {
                identity_file: None,
            },
        });
        id
    }

    fn sidebar_action(harness: &mut Harness, action: SidebarAction) {
        let ctx = harness.ctx.clone();
        harness.app.apply_sidebar_action(&ctx, action);
        harness.frame(Vec::new());
    }

    #[test]
    fn clicking_a_server_opens_a_tab_named_after_it() {
        let mut harness = Harness::new();
        let id = add_refused_server(&mut harness, "test-server", None);

        sidebar_action(&mut harness, SidebarAction::Connect(id));

        assert_eq!(harness.count(), 2);
        assert_eq!(harness.active_session().title(), "test-server");
        harness.wait_until(|app| active_session(app).exited());
        assert!(screen(&harness.app).contains("Disconnected"));
        assert!(screen(&harness.app).contains("code 255"));
    }

    #[test]
    fn server_status_follows_panes() {
        let mut harness = Harness::new();
        let id = add_refused_server(&mut harness, "test-server", None);
        assert_eq!(harness.app.server_statuses().get(&id), None);

        sidebar_action(&mut harness, SidebarAction::Connect(id));
        harness.wait_until(|app| active_session(app).exited());
        assert_eq!(
            harness.app.server_statuses().get(&id),
            Some(&ServerStatus::Disconnected)
        );

        sidebar_action(&mut harness, SidebarAction::Reconnect(id));
        assert!(!harness.active_session().exited());
    }

    #[test]
    fn enter_reconnects_a_disconnected_server() {
        let mut harness = Harness::new();
        let id = add_refused_server(&mut harness, "test-server", None);
        sidebar_action(&mut harness, SidebarAction::Connect(id));
        harness.wait_until(|app| active_session(app).exited());

        harness.press(Key::Enter, Modifiers::NONE);

        assert!(!harness.active_session().exited());
        assert_eq!(harness.active_session().title(), "test-server");
    }

    #[test]
    fn clicking_a_folder_opens_all_servers_in_splits() {
        let mut harness = Harness::new();
        let folder = harness.app.repository.data.add_folder("cj".to_owned());
        add_refused_server(&mut harness, "one", Some(folder));
        add_refused_server(&mut harness, "two", Some(folder));
        add_refused_server(&mut harness, "three", Some(folder));
        add_refused_server(&mut harness, "outside-folder", None);

        sidebar_action(&mut harness, SidebarAction::ConnectFolder(folder));

        assert_eq!(harness.count(), 2);
        assert_eq!(harness.app.tabs.pane_counts(), vec![1, 3]);
        assert_eq!(harness.app.panes.len(), 4);
    }

    #[test]
    fn empty_folder_shows_a_message() {
        let mut harness = Harness::new();
        let folder = harness.app.repository.data.add_folder("empty".to_owned());
        sidebar_action(&mut harness, SidebarAction::ConnectFolder(folder));

        assert_eq!(harness.count(), 1);
        assert!(harness.app.notice.is_some());
    }

    #[test]
    fn split_and_duplicate_of_ssh_tab_use_the_same_server() {
        let mut harness = Harness::new();
        let id = add_refused_server(&mut harness, "test-server", None);
        sidebar_action(&mut harness, SidebarAction::Connect(id));

        harness.press(Key::D, ctrl_shift());
        assert_eq!(harness.active_session().title(), "test-server");

        let ctx = harness.ctx.clone();
        harness.app.apply_tab_action(&ctx, TabAction::Duplicate(1));
        assert_eq!(harness.count(), 3);
        assert_eq!(harness.active_session().title(), "test-server");
    }

    #[test]
    fn ctrl_t_from_an_ssh_tab_still_opens_a_shell() {
        let mut harness = Harness::new();
        let id = add_refused_server(&mut harness, "test-server", None);
        sidebar_action(&mut harness, SidebarAction::Connect(id));

        harness.press(Key::T, Modifiers::CTRL);
        assert_eq!(harness.active_session().title(), "cmd");
    }

    #[test]
    fn deleted_server_cannot_reconnect() {
        let mut harness = Harness::new();
        let id = add_refused_server(&mut harness, "test-server", None);
        sidebar_action(&mut harness, SidebarAction::Connect(id));
        harness.wait_until(|app| active_session(app).exited());

        harness.app.delete_server(id);
        harness.press(Key::Enter, Modifiers::NONE);

        let pane = harness.app.tabs.active().expect("has a pane");
        assert!(matches!(
            harness.app.panes.get(&pane),
            Some(Pane {
                session: Err(_),
                ..
            })
        ));
    }

    #[test]
    fn dialog_keeps_keyboard_and_shortcuts_from_the_terminal() {
        let mut harness = Harness::new();
        sidebar_action(&mut harness, SidebarAction::AddFolder);

        harness.press(Key::T, Modifiers::CTRL);
        assert_eq!(
            harness.count(),
            1,
            "Ctrl+T must not open a tab while a dialog is open"
        );

        harness.frame(vec![Event::Paste("echo leaked-to-terminal\n".to_owned())]);
        thread::sleep(Duration::from_millis(200));
        harness.frame(Vec::new());
        assert!(!screen(&harness.app).contains("leaked-to-terminal"));
    }

    #[test]
    fn enter_in_a_dialog_does_not_restart_the_pane() {
        let mut harness = Harness::new();
        harness.type_line("exit 0");
        harness.wait_until(|app| active_session(app).exited());
        sidebar_action(&mut harness, SidebarAction::AddFolder);

        harness.press(Key::Enter, Modifiers::NONE);
        assert!(harness.active_session().exited());
    }

    #[test]
    fn saving_the_server_form_updates_the_data() {
        let mut harness = Harness::new();
        let mut form = ServerForm::new(None);
        assert!(harness.app.save_server_form(&form).is_err());
        assert!(harness.app.repository.data.servers.is_empty());

        form = ServerForm::filled("new", "10.0.0.9");
        harness.app.save_server_form(&form).expect("tersimpan");
        assert_eq!(harness.app.repository.data.servers.len(), 1);
    }

    #[test]
    fn clicking_a_sidebar_server_opens_a_tab() {
        let mut harness = Harness::new();
        add_refused_server(&mut harness, "test-server", None);
        harness.app.sidebar_visible = true;
        harness.frame(Vec::new());

        // The server row sits below the header and buttons; search for it from the top down.
        let mut y = 120.0;
        while harness.count() == 1 && y < 300.0 {
            harness.click(pos2(60.0, y));
            y += 6.0;
        }

        assert_eq!(harness.count(), 2, "clicking a server row opens a tab");
        assert_eq!(harness.active_session().title(), "test-server");
    }

    #[test]
    fn new_tab_dropdown_opens_with_powershell_first() {
        let mut harness = Harness::new();
        let button = test_positions::dropdown(WIDTH, 1);

        harness.click(button);
        assert!(egui::Popup::is_any_open(&harness.ctx));

        harness.click(button + vec2(0.0, 28.0));

        assert_eq!(harness.count(), 2);
        assert_eq!(harness.active_session().title(), "cmd");
        assert!(!egui::Popup::is_any_open(&harness.ctx));
    }

    #[test]
    fn clicking_a_dropdown_server_opens_an_ssh_tab() {
        let mut harness = Harness::new();
        add_refused_server(&mut harness, "test-server", None);
        harness.frame(Vec::new());

        // The server row sits below PowerShell; search for it from the top down.
        let mut offset = 44.0;
        while harness.active_session().title() != "test-server" && offset < 320.0 {
            let button = test_positions::dropdown(WIDTH, harness.count());
            harness.click(button);
            assert!(egui::Popup::is_any_open(&harness.ctx));
            harness.click(button + vec2(0.0, offset));
            if egui::Popup::is_any_open(&harness.ctx) {
                harness.click(pos2(WIDTH / 2.0, HEIGHT / 2.0));
            }
            offset += 4.0;
        }

        assert_eq!(harness.active_session().title(), "test-server");
    }

    #[test]
    fn menu_server_list_follows_folders() {
        let mut harness = Harness::new();
        let folder = harness.app.repository.data.add_folder("cj".to_owned());
        harness.app.repository.data.add_folder("empty".to_owned());
        add_refused_server(&mut harness, "one", Some(folder));
        add_refused_server(&mut harness, "free", None);

        let groups = harness.app.server_groups();

        assert_eq!(groups.len(), 2, "empty folders are not shown");
        assert_eq!(groups[0].title.as_deref(), Some("cj"));
        assert_eq!(groups[0].servers[0].1, "one");
        assert_eq!(groups[1].title, None);
        assert_eq!(groups[1].servers[0].1, "free");
    }

    fn open_bar(harness: &mut Harness) {
        harness.press(Key::K, Modifiers::CTRL);
        assert!(harness.app.ai_bar.is_some());
    }

    /// Simulates the AI answering the waiting request.
    fn ai_answers(harness: &mut Harness, kind: RequestKind, answer: AiResult) {
        const ID: u64 = 77;
        let bar = harness.app.ai_bar.as_mut().expect("bar is open");
        bar.phase = AiPhase::Waiting(kind);
        bar.request_id = Some(ID);
        harness.app.apply_ai_result(ID, answer);
        harness.frame(Vec::new());
    }

    fn wait_prompt(harness: &mut Harness) {
        harness.wait_until(|app| screen(app).contains('>'));
    }

    #[test]
    fn ctrl_k_opens_the_bar_and_escape_closes_it() {
        let mut harness = Harness::new();
        open_bar(&mut harness);
        harness.press(Key::Escape, Modifiers::NONE);
        assert!(harness.app.ai_bar.is_none());
    }

    #[test]
    fn safe_suggestion_is_inserted_without_running() {
        let mut harness = Harness::new();
        wait_prompt(&mut harness);
        open_bar(&mut harness);

        ai_answers(
            &mut harness,
            RequestKind::Suggest,
            Ok("echo ai-suggestion-%OS%".to_owned()),
        );
        assert!(harness.app.ai_bar.is_none(), "bar closes after inserting");

        harness.wait_until(|app| screen(app).contains("ai-suggestion-%OS%"));
        thread::sleep(Duration::from_millis(400));
        harness.frame(Vec::new());
        assert!(
            !screen(&harness.app).contains("ai-suggestion-Windows_NT"),
            "the command must not run before Enter"
        );

        harness.press(Key::Enter, Modifiers::NONE);
        harness.wait_until(|app| screen(app).contains("ai-suggestion-Windows_NT"));
    }

    #[test]
    fn dangerous_command_waits_for_confirmation() {
        let mut harness = Harness::new();
        wait_prompt(&mut harness);
        open_bar(&mut harness);

        ai_answers(
            &mut harness,
            RequestKind::Suggest,
            Ok("rm -rf /var/test-marker".to_owned()),
        );
        thread::sleep(Duration::from_millis(300));
        harness.frame(Vec::new());
        assert!(
            !screen(&harness.app).contains("test-marker"),
            "not inserted yet"
        );
        let bar = harness.app.ai_bar.as_ref().expect("bar is still open");
        assert!(matches!(bar.phase, AiPhase::Confirm { .. }));

        let ctx = harness.ctx.clone();
        harness
            .app
            .apply_ai_bar_action(&ctx, AiBarAction::InsertConfirmed);
        harness.wait_until(|app| screen(app).contains("test-marker"));
        assert!(harness.app.ai_bar.is_none());
    }

    #[test]
    fn enter_does_not_confirm_a_dangerous_command() {
        let mut harness = Harness::new();
        wait_prompt(&mut harness);
        open_bar(&mut harness);
        ai_answers(&mut harness, RequestKind::Suggest, Ok("reboot".to_owned()));

        harness.press(Key::Enter, Modifiers::NONE);
        thread::sleep(Duration::from_millis(300));
        harness.frame(Vec::new());

        assert!(!screen(&harness.app).contains("reboot"));
        assert!(harness.app.ai_bar.is_some());
    }

    #[test]
    fn multiline_answers_do_not_reach_the_terminal() {
        let mut harness = Harness::new();
        wait_prompt(&mut harness);
        open_bar(&mut harness);
        ai_answers(
            &mut harness,
            RequestKind::Suggest,
            Ok("echo first\necho second".to_owned()),
        );

        thread::sleep(Duration::from_millis(300));
        harness.frame(Vec::new());
        assert!(!screen(&harness.app).contains("first"));
        let bar = harness.app.ai_bar.as_ref().expect("bar is still open");
        assert!(matches!(bar.phase, AiPhase::Failed(_)));
    }

    #[test]
    fn answer_goes_to_the_original_pane_after_focus_moves() {
        let mut harness = Harness::new();
        let first = harness.app.tabs.active().expect("has a pane");
        wait_prompt(&mut harness);
        open_bar(&mut harness);
        harness.app.tabs.open();
        let other = harness.app.tabs.active().expect("has a pane");
        let spawned = harness.app.spawn_pane(&harness.ctx, saved_shell());
        harness.app.panes.insert(other, spawned);

        ai_answers(
            &mut harness,
            RequestKind::Suggest,
            Ok("echo to-target".to_owned()),
        );

        harness.wait_until(move |app| session_of(app, first).screen_text().contains("to-target"));
        assert!(!session_of(&harness.app, other)
            .screen_text()
            .contains("to-target"));
    }

    #[test]
    fn closed_target_pane_shows_a_message() {
        let mut harness = Harness::new();
        open_bar(&mut harness);
        harness.press(Key::D, ctrl_shift());
        let bar = harness.app.ai_bar.as_ref().expect("bar is open");
        let target = bar.target;
        harness.app.panes.remove(&target);

        ai_answers(&mut harness, RequestKind::Suggest, Ok("df -h".to_owned()));

        let bar = harness.app.ai_bar.as_ref().expect("bar is still open");
        assert!(matches!(bar.phase, AiPhase::Failed(_)));
    }

    #[test]
    fn stale_answers_are_ignored() {
        let mut harness = Harness::new();
        wait_prompt(&mut harness);
        open_bar(&mut harness);
        let bar = harness.app.ai_bar.as_mut().expect("bar is open");
        bar.phase = AiPhase::Waiting(RequestKind::Suggest);
        bar.request_id = Some(2);

        harness
            .app
            .apply_ai_result(1, Ok("echo stale-answer".to_owned()));
        harness.frame(Vec::new());

        assert!(!screen(&harness.app).contains("stale-answer"));
        let bar = harness.app.ai_bar.as_ref().expect("bar is still open");
        assert_eq!(bar.phase, AiPhase::Waiting(RequestKind::Suggest));
    }

    #[test]
    fn explain_shows_the_text_before_sending() {
        let mut harness = Harness::new();
        harness.type_line("echo test-error-message");
        harness.wait_until(|app| screen(app).matches("test-error-message").count() >= 2);
        open_bar(&mut harness);

        let ctx = harness.ctx.clone();
        harness
            .app
            .apply_ai_bar_action(&ctx, AiBarAction::PreviewOutput);

        let bar = harness.app.ai_bar.as_ref().expect("bar is open");
        let AiPhase::Preview(output) = &bar.phase else {
            panic!("must show a preview");
        };
        assert!(output.contains("test-error-message"));
        assert_eq!(bar.request_id, None, "nothing was sent yet");

        ai_answers(
            &mut harness,
            RequestKind::Explain,
            Ok("Penjelasan".to_owned()),
        );
        let bar = harness.app.ai_bar.as_ref().expect("bar is open");
        assert_eq!(bar.phase, AiPhase::Explanation("Penjelasan".to_owned()));
    }

    #[test]
    fn keyboard_stays_out_of_the_terminal_while_bar_is_open() {
        let mut harness = Harness::new();
        wait_prompt(&mut harness);
        open_bar(&mut harness);
        harness.frame(vec![Event::Paste("echo leaked-to-terminal\n".to_owned())]);
        thread::sleep(Duration::from_millis(200));
        harness.frame(Vec::new());
        assert!(!screen(&harness.app).contains("leaked-to-terminal"));
    }

    #[test]
    fn context_holds_environment_and_prompt() {
        let mut harness = Harness::new();
        wait_prompt(&mut harness);
        let pane = harness.app.tabs.active().expect("has a pane");
        let context = harness.app.pane_context(pane).expect("has context");
        assert!(context.environment.contains("Windows"));
        assert!(context.prompt_line.contains('>'));

        let id = add_refused_server(&mut harness, "test-server", None);
        sidebar_action(&mut harness, SidebarAction::Connect(id));
        let pane = harness.app.tabs.active().expect("has a pane");
        let context = harness.app.pane_context(pane).expect("has context");
        assert!(context.environment.contains("test-server"));
        assert!(
            !context.environment.contains("127.0.0.1"),
            "the address is not sent"
        );
    }

    #[test]
    fn provider_settings_are_saved() {
        let mut harness = Harness::new();
        let mut form = AiSettingsForm::new(&AiSettings::default(), false);
        form.provider = ProviderKind::ClaudeCode;
        form.model = " sonnet ".to_owned();
        harness.app.save_ai_settings(&form).expect("tersimpan");
        assert_eq!(harness.app.config.data.ai.model, "sonnet");

        form.provider = ProviderKind::ApiKey;
        assert!(
            harness.app.save_ai_settings(&form).is_err(),
            "an API key is required"
        );
        assert_eq!(
            harness.app.config.data.ai.provider,
            ProviderKind::ClaudeCode,
            "failed settings change nothing"
        );
    }

    fn saved_shell() -> PaneSource {
        PaneSource::Shell {
            shell: ShellKind::Cmd,
        }
    }

    fn harness_with_session(session: SavedSession, restore: bool) -> Harness {
        let mut config = ConfigFile::default();
        config.data.terminal.default_shell = ShellKind::Cmd;
        config.data.terminal.restore_session = restore;
        let session_file = SessionFile::in_memory(session);
        let ctx = egui::Context::default();
        let mut app =
            CterminApp::with_parts(&ctx, ServerRepository::default(), config, session_file);
        app.sidebar_visible = false;
        let mut harness = Harness {
            ctx,
            app,
            time: 0.0,
            size: vec2(WIDTH, HEIGHT),
        };
        harness.frame(Vec::new());
        harness
    }

    #[test]
    fn session_is_restored_with_tabs_and_splits() {
        let mut first = Harness::new();
        first.press(Key::D, ctrl_shift());
        first.press(Key::T, Modifiers::CTRL);
        first.press(Key::E, ctrl_shift());
        first.press(Key::Tab, Modifiers::CTRL);
        let saved = first.app.session_snapshot();

        let restored = harness_with_session(saved.clone(), true);

        assert_eq!(restored.app.tabs.pane_counts(), vec![2, 2]);
        assert_eq!(restored.app.panes.len(), 4);
        assert_eq!(restored.app.tabs.active_index(), saved.active);
    }

    #[test]
    fn empty_session_opens_the_default_shell() {
        let harness = harness_with_session(SavedSession::default(), true);
        assert_eq!(harness.count(), 1);
        assert_eq!(harness.active_session().title(), "cmd");
    }

    #[test]
    fn disabled_restore_ignores_the_saved_session() {
        let mut first = Harness::new();
        first.press(Key::T, Modifiers::CTRL);
        first.press(Key::T, Modifiers::CTRL);
        let harness = harness_with_session(first.app.session_snapshot(), false);
        assert_eq!(harness.count(), 1);
    }

    #[test]
    fn saved_server_that_was_deleted_opens_the_default_shell() {
        let mut first = Harness::new();
        let id = add_refused_server(&mut first, "test-server", None);
        sidebar_action(&mut first, SidebarAction::Connect(id));
        let saved = first.app.session_snapshot();
        assert_eq!(saved.tabs.len(), 2);

        let harness = harness_with_session(saved, true);

        assert_eq!(harness.count(), 2, "the server is gone in this repository");
        assert_eq!(harness.active_session().title(), "cmd");
    }

    #[test]
    fn saved_server_reconnects_when_it_still_exists() {
        let mut first = Harness::new();
        let id = add_refused_server(&mut first, "test-server", None);
        sidebar_action(&mut first, SidebarAction::Connect(id));
        let source = first.app.focused_source();
        assert_eq!(source, PaneSource::Server { id });
        assert_eq!(first.app.usable_source(source), source);
        assert_eq!(first.app.usable_source(saved_shell()), saved_shell());
    }

    #[test]
    fn settings_apply_to_open_panes_and_new_tabs() {
        let mut harness = Harness::new();
        let before = harness.active_session().term().columns();
        let mut form = SettingsForm::new(&harness.app.config.data, String::new());
        form.appearance.font_size = 24.0;
        form.appearance.color_scheme = ColorScheme::Dracula;
        form.terminal.default_shell = ShellKind::PowerShell;

        let ctx = harness.ctx.clone();
        harness.app.apply_settings(&ctx, form);
        harness.frame(Vec::new());

        assert!(harness.active_session().term().columns() < before);
        assert_eq!(
            harness.app.default_source(),
            PaneSource::Shell {
                shell: ShellKind::PowerShell
            }
        );
    }

    #[test]
    fn opening_a_shell_from_the_menu_uses_that_shell() {
        let mut harness = Harness::new();
        let ctx = harness.ctx.clone();
        harness
            .app
            .apply_tab_action(&ctx, TabAction::OpenShell(ShellKind::PowerShell));

        assert_eq!(harness.count(), 2);
        assert_eq!(harness.active_session().title(), "powershell");
    }

    #[test]
    fn custom_shortcut_replaces_the_default() {
        let mut config = ConfigFile::default();
        config.data.terminal.default_shell = ShellKind::Cmd;
        config.data.shortcuts.close_pane = "Ctrl+Shift+W".to_owned();
        let ctx = egui::Context::default();
        let app = CterminApp::with_parts(
            &ctx,
            ServerRepository::default(),
            config,
            SessionFile::default(),
        );
        let mut harness = Harness {
            ctx,
            app,
            time: 0.0,
            size: vec2(WIDTH, HEIGHT),
        };
        harness.press(Key::T, Modifiers::CTRL);

        harness.press(Key::W, Modifiers::CTRL);
        assert_eq!(
            harness.count(),
            2,
            "plain Ctrl+W now belongs to the terminal"
        );

        harness.press(Key::W, ctrl_shift());
        assert_eq!(harness.count(), 1);
    }

    #[test]
    fn invalid_shortcut_falls_back_and_warns() {
        let mut config = ConfigFile::default();
        config.data.terminal.default_shell = ShellKind::Cmd;
        config.data.shortcuts.new_tab = "Ctrl+Banana".to_owned();
        let ctx = egui::Context::default();
        let app = CterminApp::with_parts(
            &ctx,
            ServerRepository::default(),
            config,
            SessionFile::default(),
        );
        assert!(app
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("Ctrl+Banana")));
        let mut harness = Harness {
            ctx,
            app,
            time: 0.0,
            size: vec2(WIDTH, HEIGHT),
        };
        harness.press(Key::T, Modifiers::CTRL);
        assert_eq!(harness.count(), 2);
    }

    #[test]
    fn ctrl_comma_opens_settings() {
        let mut harness = Harness::new();
        harness.press(Key::Comma, Modifiers::CTRL);
        assert!(matches!(harness.app.dialog, Some(Dialog::Settings(_))));
    }

    #[test]
    fn sidebar_can_be_shown_and_hidden() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        let wide = harness.active_session().term().columns();

        harness.press(Key::B, ctrl_shift());
        harness.frame(Vec::new());

        assert!(harness.active_session().term().columns() < wide);
    }

    #[test]
    fn right_clicking_a_tab_opens_the_menu_and_duplicates() {
        let mut harness = Harness::new();
        let source = harness.app.tabs.active();
        let position = test_positions::tab(WIDTH, 1, 0);
        harness.right_click(position);
        assert!(egui::Popup::is_any_open(&harness.ctx));

        // The first menu row right below the pointer is "Duplicate tab".
        harness.click(position + vec2(30.0, 14.0));

        assert_eq!(harness.count(), 2);
        assert_eq!(harness.app.panes.len(), 2);
        assert_eq!(harness.app.tabs.active_index(), 1);
        assert_ne!(harness.app.tabs.active(), source);
        assert!(!egui::Popup::is_any_open(&harness.ctx));
    }

    #[test]
    fn split_menu_applies_to_the_clicked_tab() {
        let mut harness = Harness::new();
        harness.press(Key::T, Modifiers::CTRL);
        assert_eq!(harness.app.tabs.active_index(), 1);

        let ctx = harness.ctx.clone();
        harness
            .app
            .apply_tab_action(&ctx, TabAction::Split(0, Axis::Horizontal));

        assert_eq!(harness.app.tabs.active_index(), 0);
        assert_eq!(harness.app.tabs.pane_counts(), vec![2, 1]);
        assert_eq!(harness.app.panes.len(), 3);
    }

    #[test]
    fn close_split_menu_closes_only_the_focused_pane() {
        let mut harness = Harness::new();
        harness.press(Key::D, ctrl_shift());
        harness.press(Key::T, Modifiers::CTRL);

        let ctx = harness.ctx.clone();
        harness.app.apply_tab_action(&ctx, TabAction::ClosePane(0));

        assert_eq!(harness.app.tabs.pane_counts(), vec![1, 1]);
        assert_eq!(harness.app.panes.len(), 2);
    }

    #[test]
    fn duplicate_menu_places_the_tab_next_to_its_source() {
        let mut harness = Harness::new();
        harness.press(Key::T, Modifiers::CTRL);
        let last = harness.app.tabs.active();

        let ctx = harness.ctx.clone();
        harness.app.apply_tab_action(&ctx, TabAction::Duplicate(0));

        assert_eq!(harness.count(), 3);
        assert_eq!(harness.app.tabs.active_index(), 1);
        assert_eq!(harness.app.tabs.focused_panes().last().copied(), last);
    }
}
