//! Mode helper `SSH_ASKPASS`: ssh.exe menjalankan ctermin.exe saat butuh jawaban dari user.
//!
//! Dengan cara ini password hanya diberikan saat ssh sendiri yang meminta. Teks `password:` di
//! layar tidak pernah dipakai sebagai pemicu, karena server bisa memalsukannya. Password juga
//! tidak lewat argumen, environment, maupun terminal.

use std::ffi::OsStr;
use std::io::Write;

use portable_pty::CommandBuilder;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
};

use super::credentials;
use super::model::ServerId;

/// Hanya berisi id server, bukan rahasia.
const SERVER_VARIABLE: &str = "CTERMIN_ASKPASS_SERVER";

const EXIT_OK: i32 = 0;
const EXIT_REFUSED: i32 = 1;

#[derive(PartialEq, Eq, Debug)]
enum Prompt {
    Password,
    /// Pertanyaan ya/tidak, misalnya fingerprint host yang belum dikenal.
    Confirm,
    /// Prompt lain (passphrase, kode OTP). Tidak boleh dijawab dengan password.
    Unknown,
}

fn classify(prompt: &str) -> Prompt {
    let lower = prompt.to_lowercase();
    if lower.contains("(yes/no") {
        Prompt::Confirm
    } else if lower.contains("password") && lower.trim_end().ends_with(':') {
        Prompt::Password
    } else {
        Prompt::Unknown
    }
}

pub fn configure(command: &mut CommandBuilder, program: &OsStr, server: ServerId) {
    command.env("SSH_ASKPASS", program);
    command.env("SSH_ASKPASS_REQUIRE", "force");
    command.env(SERVER_VARIABLE, server.0.to_string());
}

/// Dijalankan paling awal di `main`. `Some` berisi kode keluar jika proses ini adalah helper.
pub fn run_if_requested() -> Option<i32> {
    let server = std::env::var(SERVER_VARIABLE).ok()?;
    let prompt = std::env::args().nth(1).unwrap_or_default();
    Some(answer(&server, &prompt))
}

fn answer(server: &str, prompt: &str) -> i32 {
    match classify(prompt) {
        Prompt::Confirm => print_answer(if confirm(prompt) { "yes" } else { "no" }),
        Prompt::Password => {
            let stored = server
                .parse()
                .ok()
                .and_then(|id| credentials::load_password(ServerId(id)).ok().flatten());
            match stored {
                Some(password) => print_answer(&password),
                None => EXIT_REFUSED,
            }
        }
        Prompt::Unknown => EXIT_REFUSED,
    }
}

fn print_answer(answer: &str) -> i32 {
    let mut stdout = std::io::stdout().lock();
    match writeln!(stdout, "{answer}").and_then(|()| stdout.flush()) {
        Ok(()) => EXIT_OK,
        Err(_) => EXIT_REFUSED,
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Dialog Ya/Tidak. Fingerprint tetap diputuskan oleh user, tidak diterima otomatis.
fn confirm(prompt: &str) -> bool {
    let text = wide(prompt);
    let title = wide("ctermin - konfirmasi SSH");
    // SAFETY: kedua string diakhiri nol dan hidup selama panggilan; handle NULL berarti tanpa
    // jendela induk.
    let result = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND,
        )
    };
    result == IDYES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_password_dikenali() {
        assert_eq!(classify("root@10.0.0.5's password: "), Prompt::Password);
        assert_eq!(classify("(root@host) Password: "), Prompt::Password);
    }

    #[test]
    fn prompt_fingerprint_menjadi_konfirmasi() {
        let prompt = "The authenticity of host 'x' can't be established.\n\
                      Are you sure you want to continue connecting (yes/no/[fingerprint])? ";
        assert_eq!(classify(prompt), Prompt::Confirm);
    }

    #[test]
    fn prompt_lain_tidak_pernah_menerima_password() {
        assert_eq!(classify("Verification code: "), Prompt::Unknown);
        assert_eq!(
            classify("Enter passphrase for key 'id_ed25519': "),
            Prompt::Unknown
        );
        assert_eq!(classify("Your password has expired"), Prompt::Unknown);
    }

    #[test]
    fn server_tanpa_password_tersimpan_ditolak() {
        let unused = (u64::MAX - 2 - u64::from(std::process::id())).to_string();
        assert_eq!(answer(&unused, "root@host's password: "), EXIT_REFUSED);
        assert_eq!(
            answer("bukan-angka", "root@host's password: "),
            EXIT_REFUSED
        );
    }

    #[test]
    fn perintah_ssh_diberi_askpass_tanpa_password() {
        let mut command = CommandBuilder::new("ssh.exe");
        configure(&mut command, OsStr::new("ctermin.exe"), ServerId(7));
        let environment: Vec<_> = command.iter_extra_env_as_str().collect();
        assert!(environment.contains(&("SSH_ASKPASS_REQUIRE", "force")));
        assert!(environment.contains(&(SERVER_VARIABLE, "7")));
    }
}
