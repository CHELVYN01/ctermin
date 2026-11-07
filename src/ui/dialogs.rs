//! Modal dialogs: server form, folder form, and delete confirmation.

use eframe::egui::{self, ComboBox, Grid, Id, Modal, TextEdit};

use crate::ai::settings::{model_options, AiSettings, ProviderKind, DEFAULT_API_MODEL};
use crate::ssh::command::is_safe_ssh_value;
use crate::ssh::model::{AuthMethod, Folder, FolderId, Server, ServerId, DEFAULT_PORT};

const FIELD_WIDTH: f32 = 260.0;
const CUSTOM_MODEL_LABEL: &str = "Other...";

/// The outcome of interacting with a dialog this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogResult {
    Open,
    Confirmed,
    Cancelled,
}

pub struct ServerForm {
    pub editing: Option<ServerId>,
    name: String,
    host: String,
    port: String,
    username: String,
    use_password: bool,
    identity_file: String,
    password: String,
    folder: Option<FolderId>,
    has_saved_password: bool,
    pub error: Option<String>,
}

/// Form contents that passed validation.
pub struct ValidatedServer {
    pub server: Server,
    /// `None` means the saved password is unchanged.
    pub new_password: Option<String>,
}

impl ServerForm {
    pub fn new(folder: Option<FolderId>) -> Self {
        Self {
            editing: None,
            name: String::new(),
            host: String::new(),
            port: DEFAULT_PORT.to_string(),
            username: String::new(),
            use_password: false,
            identity_file: String::new(),
            password: String::new(),
            folder,
            has_saved_password: false,
            error: None,
        }
    }

    /// A saved password is never loaded into the form.
    pub fn edit(server: &Server, has_saved_password: bool) -> Self {
        let (use_password, identity_file) = match &server.auth {
            AuthMethod::Password => (true, String::new()),
            AuthMethod::Key { identity_file } => (false, identity_file.clone().unwrap_or_default()),
        };
        Self {
            editing: Some(server.id),
            name: server.name.clone(),
            host: server.host.clone(),
            port: server.port.to_string(),
            username: server.username.clone(),
            use_password,
            identity_file,
            password: String::new(),
            folder: server.folder,
            has_saved_password,
            error: None,
        }
    }

    #[cfg(test)]
    pub fn filled(name: &str, host: &str) -> Self {
        let mut form = Self::new(None);
        form.name = name.to_owned();
        form.host = host.to_owned();
        form
    }

    pub fn validate(&self, id: ServerId) -> Result<ValidatedServer, String> {
        let name = self.name.trim();
        let host = self.host.trim();
        let username = self.username.trim();
        let identity_file = self.identity_file.trim();

        if name.is_empty() {
            return Err("Server name is required.".to_owned());
        }
        if host.is_empty() {
            return Err("Host is required.".to_owned());
        }
        if !is_safe_ssh_value(host) {
            return Err("Host must not contain spaces or start with a dash.".to_owned());
        }
        if !username.is_empty() && !is_safe_ssh_value(username) {
            return Err("Username must not contain spaces or start with a dash.".to_owned());
        }
        let port = match self.port.trim().parse::<u16>() {
            Ok(port) if port > 0 => port,
            _ => return Err("Port must be a number from 1 to 65535.".to_owned()),
        };

        let auth = if self.use_password {
            AuthMethod::Password
        } else {
            AuthMethod::Key {
                identity_file: (!identity_file.is_empty()).then(|| identity_file.to_owned()),
            }
        };
        let new_password =
            (self.use_password && !self.password.is_empty()).then(|| self.password.clone());

        Ok(ValidatedServer {
            server: Server {
                id,
                name: name.to_owned(),
                host: host.to_owned(),
                port,
                username: username.to_owned(),
                folder: self.folder,
                auth,
            },
            new_password,
        })
    }
}

pub struct FolderForm {
    pub editing: Option<FolderId>,
    pub name: String,
    pub error: Option<String>,
}

impl FolderForm {
    pub fn new() -> Self {
        Self {
            editing: None,
            name: String::new(),
            error: None,
        }
    }

    pub fn edit(folder: &Folder) -> Self {
        Self {
            editing: Some(folder.id),
            name: folder.name.clone(),
            error: None,
        }
    }

    pub fn validated_name(&self) -> Result<String, String> {
        let name = self.name.trim();
        if name.is_empty() {
            Err("Folder name is required.".to_owned())
        } else {
            Ok(name.to_owned())
        }
    }
}

fn buttons(ui: &mut egui::Ui, confirm_label: &str) -> DialogResult {
    let mut result = DialogResult::Open;
    ui.horizontal(|ui| {
        if ui.button(confirm_label).clicked() {
            result = DialogResult::Confirmed;
        }
        if ui.button("Cancel").clicked() {
            result = DialogResult::Cancelled;
        }
    });
    result
}

fn show_error(ui: &mut egui::Ui, error: Option<&str>) {
    if let Some(error) = error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
}

fn text_field(ui: &mut egui::Ui, value: &mut String) {
    ui.add(TextEdit::singleline(value).desired_width(FIELD_WIDTH));
}

pub fn show_server_form(
    ctx: &egui::Context,
    id: Id,
    form: &mut ServerForm,
    folders: &[Folder],
) -> DialogResult {
    let response = Modal::new(id).show(ctx, |ui| {
        let title = if form.editing.is_some() {
            "Edit server"
        } else {
            "Add server"
        };
        ui.heading(title);
        ui.add_space(6.0);

        Grid::new("server_form").num_columns(2).show(ui, |ui| {
            ui.label("Name");
            text_field(ui, &mut form.name);
            ui.end_row();

            ui.label("Host");
            text_field(ui, &mut form.host);
            ui.end_row();

            ui.label("Port");
            text_field(ui, &mut form.port);
            ui.end_row();

            ui.label("Username");
            text_field(ui, &mut form.username);
            ui.end_row();

            ui.label("Folder");
            let selected = form
                .folder
                .and_then(|id| folders.iter().find(|folder| folder.id == id))
                .map_or("No folder", |folder| folder.name.as_str());
            ComboBox::from_id_salt("server_folder")
                .width(FIELD_WIDTH)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut form.folder, None, "No folder");
                    for folder in folders {
                        ui.selectable_value(&mut form.folder, Some(folder.id), &folder.name);
                    }
                });
            ui.end_row();

            ui.label("Login");
            ui.horizontal(|ui| {
                ui.radio_value(&mut form.use_password, false, "SSH key");
                ui.radio_value(&mut form.use_password, true, "Password");
            });
            ui.end_row();

            if form.use_password {
                ui.label("Password");
                let hint = if form.has_saved_password {
                    "Leave empty to keep it"
                } else {
                    "Leave empty to type it at login"
                };
                ui.add(
                    TextEdit::singleline(&mut form.password)
                        .password(true)
                        .hint_text(hint)
                        .desired_width(FIELD_WIDTH),
                );
            } else {
                ui.label("Key file");
                ui.add(
                    TextEdit::singleline(&mut form.identity_file)
                        .hint_text("Leave empty for the default key")
                        .desired_width(FIELD_WIDTH),
                );
            }
            ui.end_row();
        });

        ui.add_space(6.0);
        show_error(ui, form.error.as_deref());
        buttons(ui, "Save")
    });
    finish(response.inner, response.should_close())
}

pub fn show_folder_form(ctx: &egui::Context, id: Id, form: &mut FolderForm) -> DialogResult {
    let response = Modal::new(id).show(ctx, |ui| {
        let title = if form.editing.is_some() {
            "Rename folder"
        } else {
            "Add folder"
        };
        ui.heading(title);
        ui.add_space(6.0);
        text_field(ui, &mut form.name);
        ui.add_space(6.0);
        show_error(ui, form.error.as_deref());
        buttons(ui, "Save")
    });
    finish(response.inner, response.should_close())
}

pub fn show_confirmation(ctx: &egui::Context, id: Id, message: &str) -> DialogResult {
    let response = Modal::new(id).show(ctx, |ui| {
        ui.label(message);
        ui.add_space(6.0);
        buttons(ui, "Delete")
    });
    finish(response.inner, response.should_close())
}

pub struct AiSettingsForm {
    pub provider: ProviderKind,
    pub model: String,
    custom_model: bool,
    shown_provider: ProviderKind,
    pub api_key: String,
    has_saved_key: bool,
    pub delete_key: bool,
    pub error: Option<String>,
}

impl AiSettingsForm {
    /// A saved API key is never loaded into the form.
    pub fn new(settings: &AiSettings, has_saved_key: bool) -> Self {
        let model = if settings.model.is_empty() {
            Self::default_model(settings.provider)
        } else {
            settings.model.clone()
        };
        let custom_model = !model_options(settings.provider)
            .iter()
            .any(|option| option.id == model);
        Self {
            provider: settings.provider,
            model,
            custom_model,
            shown_provider: settings.provider,
            api_key: String::new(),
            has_saved_key,
            delete_key: false,
            error: None,
        }
    }

    fn default_model(provider: ProviderKind) -> String {
        match provider {
            ProviderKind::ClaudeCode => String::new(),
            ProviderKind::ApiKey => DEFAULT_API_MODEL.to_owned(),
        }
    }

    fn choose_model(&mut self, id: &str) {
        self.model = id.to_owned();
        self.custom_model = false;
    }

    fn choose_custom_model(&mut self) {
        self.model.clear();
        self.custom_model = true;
    }

    /// Model names differ between providers, so the old choice must not carry over.
    fn sync_provider(&mut self) {
        if self.provider != self.shown_provider {
            self.shown_provider = self.provider;
            self.choose_model(&Self::default_model(self.provider));
        }
    }

    pub fn settings(&self) -> AiSettings {
        AiSettings {
            provider: self.provider,
            model: self.model.trim().to_owned(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !is_safe_ssh_value(self.model.trim()) {
            return Err("Model name must not contain spaces or start with a dash.".to_owned());
        }
        let key_available =
            !self.api_key.trim().is_empty() || (self.has_saved_key && !self.delete_key);
        if self.provider == ProviderKind::ApiKey && !key_available {
            return Err("Enter an API key to use this provider.".to_owned());
        }
        Ok(())
    }
}

fn model_picker(ui: &mut egui::Ui, form: &mut AiSettingsForm) {
    let options = model_options(form.provider);
    let selected = if form.custom_model {
        CUSTOM_MODEL_LABEL
    } else {
        options
            .iter()
            .find(|option| option.id == form.model)
            .map_or(CUSTOM_MODEL_LABEL, |option| option.label)
    };
    ComboBox::from_id_salt("ai_model")
        .width(FIELD_WIDTH)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for option in options {
                let chosen = !form.custom_model && option.id == form.model;
                if ui.selectable_label(chosen, option.label).clicked() {
                    form.choose_model(option.id);
                }
            }
            if ui
                .selectable_label(form.custom_model, CUSTOM_MODEL_LABEL)
                .clicked()
            {
                form.choose_custom_model();
            }
        });
}

pub fn show_ai_settings(ctx: &egui::Context, id: Id, form: &mut AiSettingsForm) -> DialogResult {
    let response = Modal::new(id).show(ctx, |ui| {
        ui.heading("AI Settings");
        ui.add_space(6.0);
        ui.radio_value(
            &mut form.provider,
            ProviderKind::ClaudeCode,
            "Claude Code (subscription quota)",
        );
        ui.small("Uses the Claude Code already signed in on this computer.");
        ui.radio_value(&mut form.provider, ProviderKind::ApiKey, "Claude API key");
        ui.small("Pay per use, separate from your subscription.");
        form.sync_provider();
        ui.add_space(6.0);

        Grid::new("ai_settings").num_columns(2).show(ui, |ui| {
            ui.label("Model");
            model_picker(ui, form);
            ui.end_row();

            if form.custom_model {
                ui.label("Model name");
                ui.add(
                    TextEdit::singleline(&mut form.model)
                        .hint_text("Model name or ID")
                        .desired_width(FIELD_WIDTH),
                );
                ui.end_row();
            }

            if form.provider == ProviderKind::ApiKey {
                ui.label("API key");
                let hint = if form.has_saved_key {
                    "Leave empty to keep it"
                } else {
                    "Stored in Credential Manager"
                };
                ui.add(
                    TextEdit::singleline(&mut form.api_key)
                        .password(true)
                        .hint_text(hint)
                        .desired_width(FIELD_WIDTH),
                );
                ui.end_row();

                if form.has_saved_key {
                    ui.label("");
                    ui.checkbox(&mut form.delete_key, "Delete saved API key");
                    ui.end_row();
                }
            }
        });

        ui.add_space(6.0);
        show_error(ui, form.error.as_deref());
        buttons(ui, "Save")
    });
    finish(response.inner, response.should_close())
}

fn finish(result: DialogResult, should_close: bool) -> DialogResult {
    if result == DialogResult::Open && should_close {
        DialogResult::Cancelled
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_form() -> ServerForm {
        let mut form = ServerForm::new(None);
        form.name = " cj ".to_owned();
        form.host = "10.0.0.5".to_owned();
        form.username = "root".to_owned();
        form
    }

    impl ServerForm {
        /// `ValidatedServer` has no `Debug` on purpose, because it holds a password.
        fn validated(&self, id: u64) -> ValidatedServer {
            match self.validate(ServerId(id)) {
                Ok(validated) => validated,
                Err(error) => panic!("form must be valid: {error}"),
            }
        }
    }

    fn error_of(form: &ServerForm) -> String {
        form.validate(ServerId(1)).err().expect("must be rejected")
    }

    #[test]
    fn valid_form_produces_a_server() {
        let result = valid_form().validated(1);
        assert_eq!(result.server.name, "cj", "surrounding spaces are trimmed");
        assert_eq!(result.server.port, DEFAULT_PORT);
        assert_eq!(
            result.server.auth,
            AuthMethod::Key {
                identity_file: None
            }
        );
        assert!(result.new_password.is_none());
    }

    #[test]
    fn required_fields_and_formats_are_validated() {
        let mut form = valid_form();
        form.name.clear();
        assert!(error_of(&form).contains("name"));

        let mut form = valid_form();
        form.host.clear();
        assert!(error_of(&form).contains("Host"));

        for port in ["0", "70000", "abc", ""] {
            let mut form = valid_form();
            form.port = port.to_owned();
            assert!(error_of(&form).contains("Port"), "port {port:?}");
        }
    }

    #[test]
    fn ssh_option_injection_is_rejected() {
        let mut form = valid_form();
        form.host = "-oProxyCommand=calc".to_owned();
        assert!(error_of(&form).contains("Host"));

        let mut form = valid_form();
        form.username = "-oProxyCommand=calc".to_owned();
        assert!(error_of(&form).contains("Username"));
    }

    #[test]
    fn password_is_kept_only_for_password_login() {
        let mut form = valid_form();
        form.password = "value".to_owned();
        let result = form.validated(1);
        assert!(result.new_password.is_none());

        form.use_password = true;
        let result = form.validated(1);
        assert_eq!(result.server.auth, AuthMethod::Password);
        assert_eq!(result.new_password.as_deref(), Some("value"));
    }

    #[test]
    fn edit_does_not_load_the_password() {
        let mut server = valid_form().validated(3).server;
        server.auth = AuthMethod::Password;
        let form = ServerForm::edit(&server, true);

        assert!(form.password.is_empty());
        assert!(form.has_saved_password);
        assert_eq!(form.editing, Some(ServerId(3)));
        let again = form.validated(3);
        assert_eq!(again.server, server);
        assert!(
            again.new_password.is_none(),
            "the old password is unchanged"
        );
    }

    fn ai_form(provider: ProviderKind, model: &str) -> AiSettingsForm {
        let settings = AiSettings {
            provider,
            model: model.to_owned(),
        };
        AiSettingsForm::new(&settings, true)
    }

    #[test]
    fn default_model_is_selected_in_the_dropdown() {
        let form = ai_form(ProviderKind::ClaudeCode, "");
        assert!(!form.custom_model);
        assert_eq!(form.settings().model, "");

        let form = ai_form(ProviderKind::ApiKey, "");
        assert!(!form.custom_model);
        assert_eq!(form.settings().model, DEFAULT_API_MODEL);
    }

    #[test]
    fn unlisted_model_becomes_other() {
        let form = ai_form(ProviderKind::ApiKey, "claude-opus-4-8");
        assert!(form.custom_model);
        assert_eq!(form.settings().model, "claude-opus-4-8");
    }

    #[test]
    fn choosing_a_model_changes_the_settings() {
        let mut form = ai_form(ProviderKind::ClaudeCode, "");
        form.choose_model("haiku");
        assert_eq!(form.settings().model, "haiku");

        form.choose_custom_model();
        assert!(form.custom_model);
        form.model = "claude-opus-4-8".to_owned();
        assert_eq!(form.settings().model, "claude-opus-4-8");

        form.choose_model("sonnet");
        assert!(!form.custom_model);
    }

    #[test]
    fn switching_provider_resets_the_model() {
        let mut form = ai_form(ProviderKind::ClaudeCode, "sonnet");
        form.provider = ProviderKind::ApiKey;
        form.sync_provider();
        assert_eq!(form.settings().model, DEFAULT_API_MODEL);

        form.provider = ProviderKind::ClaudeCode;
        form.sync_provider();
        assert_eq!(form.settings().model, "");
    }

    #[test]
    fn sync_without_provider_change_keeps_the_choice() {
        let mut form = ai_form(ProviderKind::ClaudeCode, "haiku");
        form.sync_provider();
        assert_eq!(form.settings().model, "haiku");
    }
    #[test]
    fn folder_name_is_required() {
        let mut form = FolderForm::new();
        assert!(form.validated_name().is_err());
        form.name = " cj ".to_owned();
        assert_eq!(form.validated_name().as_deref(), Ok("cj"));
    }
}
