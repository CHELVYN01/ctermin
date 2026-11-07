//! Dialog modal: form server, form folder, dan konfirmasi hapus.

use eframe::egui::{self, ComboBox, Grid, Id, Modal, TextEdit};

use crate::ssh::command::is_safe_ssh_value;
use crate::ssh::model::{AuthMethod, Folder, FolderId, Server, ServerId, DEFAULT_PORT};

const FIELD_WIDTH: f32 = 260.0;

/// Hasil interaksi dengan dialog pada frame ini.
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

/// Isi form yang sudah lolos validasi.
pub struct ValidatedServer {
    pub server: Server,
    /// `None` berarti password tersimpan tidak diubah.
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

    /// Password tersimpan tidak pernah dimuat ke form.
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
            return Err("Nama server wajib diisi.".to_owned());
        }
        if host.is_empty() {
            return Err("Host wajib diisi.".to_owned());
        }
        if !is_safe_ssh_value(host) {
            return Err("Host tidak boleh berisi spasi atau diawali tanda minus.".to_owned());
        }
        if !username.is_empty() && !is_safe_ssh_value(username) {
            return Err("Username tidak boleh berisi spasi atau diawali tanda minus.".to_owned());
        }
        let port = match self.port.trim().parse::<u16>() {
            Ok(port) if port > 0 => port,
            _ => return Err("Port harus angka 1 sampai 65535.".to_owned()),
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
            Err("Nama folder wajib diisi.".to_owned())
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
        if ui.button("Batal").clicked() {
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
            "Tambah server"
        };
        ui.heading(title);
        ui.add_space(6.0);

        Grid::new("server_form").num_columns(2).show(ui, |ui| {
            ui.label("Nama");
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
                .map_or("Tanpa folder", |folder| folder.name.as_str());
            ComboBox::from_id_salt("server_folder")
                .width(FIELD_WIDTH)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut form.folder, None, "Tanpa folder");
                    for folder in folders {
                        ui.selectable_value(&mut form.folder, Some(folder.id), &folder.name);
                    }
                });
            ui.end_row();

            ui.label("Login");
            ui.horizontal(|ui| {
                ui.radio_value(&mut form.use_password, false, "Kunci SSH");
                ui.radio_value(&mut form.use_password, true, "Password");
            });
            ui.end_row();

            if form.use_password {
                ui.label("Password");
                let hint = if form.has_saved_password {
                    "Kosongkan agar tidak berubah"
                } else {
                    "Kosongkan untuk mengetik saat login"
                };
                ui.add(
                    TextEdit::singleline(&mut form.password)
                        .password(true)
                        .hint_text(hint)
                        .desired_width(FIELD_WIDTH),
                );
            } else {
                ui.label("File kunci");
                ui.add(
                    TextEdit::singleline(&mut form.identity_file)
                        .hint_text("Kosongkan untuk kunci bawaan")
                        .desired_width(FIELD_WIDTH),
                );
            }
            ui.end_row();
        });

        ui.add_space(6.0);
        show_error(ui, form.error.as_deref());
        buttons(ui, "Simpan")
    });
    finish(response.inner, response.should_close())
}

pub fn show_folder_form(ctx: &egui::Context, id: Id, form: &mut FolderForm) -> DialogResult {
    let response = Modal::new(id).show(ctx, |ui| {
        let title = if form.editing.is_some() {
            "Ganti nama folder"
        } else {
            "Tambah folder"
        };
        ui.heading(title);
        ui.add_space(6.0);
        text_field(ui, &mut form.name);
        ui.add_space(6.0);
        show_error(ui, form.error.as_deref());
        buttons(ui, "Simpan")
    });
    finish(response.inner, response.should_close())
}

pub fn show_confirmation(ctx: &egui::Context, id: Id, message: &str) -> DialogResult {
    let response = Modal::new(id).show(ctx, |ui| {
        ui.label(message);
        ui.add_space(6.0);
        buttons(ui, "Hapus")
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
        /// `ValidatedServer` sengaja tanpa `Debug` karena memuat password.
        fn validated(&self, id: u64) -> ValidatedServer {
            match self.validate(ServerId(id)) {
                Ok(validated) => validated,
                Err(error) => panic!("form harus valid: {error}"),
            }
        }
    }

    fn error_of(form: &ServerForm) -> String {
        form.validate(ServerId(1)).err().expect("harus ditolak")
    }

    #[test]
    fn form_valid_menghasilkan_server() {
        let result = valid_form().validated(1);
        assert_eq!(result.server.name, "cj", "spasi tepi dibuang");
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
    fn field_wajib_dan_format_divalidasi() {
        let mut form = valid_form();
        form.name.clear();
        assert!(error_of(&form).contains("Nama"));

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
    fn injeksi_opsi_ssh_ditolak() {
        let mut form = valid_form();
        form.host = "-oProxyCommand=calc".to_owned();
        assert!(error_of(&form).contains("Host"));

        let mut form = valid_form();
        form.username = "-oProxyCommand=calc".to_owned();
        assert!(error_of(&form).contains("Username"));
    }

    #[test]
    fn password_hanya_terbawa_jika_metode_password() {
        let mut form = valid_form();
        form.password = "nilai".to_owned();
        let result = form.validated(1);
        assert!(result.new_password.is_none());

        form.use_password = true;
        let result = form.validated(1);
        assert_eq!(result.server.auth, AuthMethod::Password);
        assert_eq!(result.new_password.as_deref(), Some("nilai"));
    }

    #[test]
    fn edit_tidak_memuat_password() {
        let mut server = valid_form().validated(3).server;
        server.auth = AuthMethod::Password;
        let form = ServerForm::edit(&server, true);

        assert!(form.password.is_empty());
        assert!(form.has_saved_password);
        assert_eq!(form.editing, Some(ServerId(3)));
        let again = form.validated(3);
        assert_eq!(again.server, server);
        assert!(again.new_password.is_none(), "password lama tidak diubah");
    }

    #[test]
    fn nama_folder_wajib_diisi() {
        let mut form = FolderForm::new();
        assert!(form.validated_name().is_err());
        form.name = " cj ".to_owned();
        assert_eq!(form.validated_name().as_deref(), Ok("cj"));
    }
}
