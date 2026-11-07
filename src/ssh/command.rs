//! Menyusun perintah `ssh.exe` untuk sebuah server.

use std::ffi::OsString;

use portable_pty::CommandBuilder;

use super::askpass;
use super::model::{AuthMethod, Server, DEFAULT_PORT};

/// Menolak nilai yang bisa dibaca ssh sebagai opsi, misalnya `-oProxyCommand=...`.
pub fn is_safe_ssh_value(value: &str) -> bool {
    !value.starts_with('-')
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

fn ssh_args(server: &Server, has_password: bool) -> Vec<String> {
    let mut args = Vec::new();
    if server.port != DEFAULT_PORT {
        args.extend(["-p".to_owned(), server.port.to_string()]);
    }
    if !server.username.is_empty() {
        args.extend(["-l".to_owned(), server.username.clone()]);
    }
    if let AuthMethod::Key {
        identity_file: Some(path),
    } = &server.auth
    {
        args.extend(["-i".to_owned(), path.clone()]);
    }
    if has_password {
        // Password tersimpan yang salah tidak boleh dicoba berulang sampai akun terkunci.
        args.extend(["-o".to_owned(), "NumberOfPasswordPrompts=1".to_owned()]);
    }
    args.extend(["--".to_owned(), server.host.clone()]);
    args
}

/// Membuat perintah ssh. `askpass_program` diisi jika password tersimpan di Credential Manager.
pub fn ssh_command(server: &Server, askpass_program: Option<OsString>) -> CommandBuilder {
    let mut command = CommandBuilder::new("ssh.exe");
    command.args(ssh_args(server, askpass_program.is_some()));
    if let Some(program) = askpass_program {
        askpass::configure(&mut command, &program, server.id);
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        command.cwd(home);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::model::ServerId;

    fn server() -> Server {
        Server {
            id: ServerId(1),
            name: "cj".to_owned(),
            host: "10.0.0.5".to_owned(),
            port: DEFAULT_PORT,
            username: "root".to_owned(),
            folder: None,
            auth: AuthMethod::Key {
                identity_file: None,
            },
        }
    }

    #[test]
    fn perintah_dasar() {
        assert_eq!(
            ssh_args(&server(), false),
            vec!["-l", "root", "--", "10.0.0.5"]
        );
    }

    #[test]
    fn port_dan_kunci_ikut_jika_diisi() {
        let mut server = server();
        server.port = 2222;
        server.auth = AuthMethod::Key {
            identity_file: Some(r"C:\kunci saya\id".to_owned()),
        };
        assert_eq!(
            ssh_args(&server, false),
            vec![
                "-p",
                "2222",
                "-l",
                "root",
                "-i",
                r"C:\kunci saya\id",
                "--",
                "10.0.0.5"
            ]
        );
    }

    #[test]
    fn username_kosong_tidak_mengirim_opsi() {
        let mut server = server();
        server.username.clear();
        assert_eq!(ssh_args(&server, false), vec!["--", "10.0.0.5"]);
    }

    #[test]
    fn password_tersimpan_hanya_dicoba_sekali() {
        let args = ssh_args(&server(), true);
        assert!(args.contains(&"NumberOfPasswordPrompts=1".to_owned()));
    }

    #[test]
    fn host_selalu_setelah_pemisah_opsi() {
        let args = ssh_args(&server(), true);
        assert_eq!(args[args.len() - 2], "--");
        assert_eq!(args[args.len() - 1], "10.0.0.5");
    }

    #[test]
    fn nilai_berbahaya_ditolak() {
        assert!(is_safe_ssh_value("10.0.0.5"));
        assert!(is_safe_ssh_value("server.contoh.id"));
        assert!(is_safe_ssh_value("user_1"));
        assert!(!is_safe_ssh_value("-oProxyCommand=calc"));
        assert!(!is_safe_ssh_value("host dengan spasi"));
        assert!(!is_safe_ssh_value("host\nbaris"));
    }

    #[test]
    fn password_tidak_pernah_ada_di_argumen() {
        let mut server = server();
        server.auth = AuthMethod::Password;
        let command = ssh_command(&server, Some("ctermin.exe".into()));
        let argv = command.get_argv();
        assert!(argv
            .iter()
            .all(|arg| !arg.to_string_lossy().contains("rahasia")));
    }
}
