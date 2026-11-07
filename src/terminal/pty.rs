//! Proses shell di dalam ConPTY beserta thread pendukungnya.
//!
//! Semua operasi yang bisa memblokir terjadi di thread terpisah supaya UI tidak pernah
//! menunggu PTY:
//! - `pty-reader` membaca output dan mengirimnya ke UI.
//! - `pty-control` memegang master PTY: menulis input, resize, lalu menutup ConPTY.
//! - `pty-waiter` menunggu proses selesai, karena ConPTY tidak mengirim EOF saat shell
//!   keluar sampai pseudoconsole ditutup.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

const READ_BUFFER_SIZE: usize = 64 * 1024;
/// Jumlah potongan output yang boleh antre. Saat penuh, pembaca menunggu UI (backpressure).
const OUTPUT_QUEUE_SIZE: usize = 32;

/// Dipanggil dari thread latar setelah ada kejadian baru untuk UI.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// Kejadian dari PTY untuk diproses di UI thread.
pub enum PtyEvent {
    Output(Vec<u8>),
    /// Proses selesai. `None` jika kode keluarnya tidak diketahui.
    Exited(Option<u32>),
}

enum Control {
    Write(Vec<u8>),
    Resize(PtySize),
    Shutdown,
}

/// Saat di-drop, proses dimatikan dan ketiga thread berhenti dengan sendirinya.
pub struct Pty {
    control: Sender<Control>,
    events: Receiver<PtyEvent>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    #[cfg(test)]
    threads_alive: std::sync::Weak<()>,
}

/// Shell bawaan: PowerShell di folder home user.
pub fn default_shell() -> CommandBuilder {
    let mut command = CommandBuilder::new("powershell.exe");
    command.arg("-NoLogo");
    if let Some(home) = std::env::var_os("USERPROFILE") {
        command.cwd(home);
    }
    command
}

impl Pty {
    pub fn spawn(mut command: CommandBuilder, size: PtySize, notify: Notify) -> Result<Self> {
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        enable_ctrl_c_for_children();

        let pair = native_pty_system()
            .openpty(size)
            .map_err(|err| anyhow!("Gagal membuat ConPTY: {err}"))?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| anyhow!("Gagal menjalankan shell: {err}"))?;
        // Handle slave di proses ini harus ditutup agar hanya shell yang memakainya.
        drop(pair.slave);

        let killer = child.clone_killer();
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| anyhow!("Gagal membaca PTY: {err}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| anyhow!("Gagal menulis ke PTY: {err}"))?;

        let (control_tx, control_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::sync_channel(OUTPUT_QUEUE_SIZE);
        let alive = Arc::new(());
        #[cfg(test)]
        let threads_alive = Arc::downgrade(&alive);

        let reader_thread = {
            let (event_tx, notify, alive) = (event_tx.clone(), notify.clone(), alive.clone());
            thread::Builder::new()
                .name("pty-reader".into())
                .spawn(move || {
                    let _alive = alive;
                    read_loop(reader, event_tx, notify);
                })
                .context("Gagal membuat thread pembaca PTY")?
        };

        {
            let alive = alive.clone();
            let master = pair.master;
            thread::Builder::new()
                .name("pty-control".into())
                .spawn(move || {
                    let _alive = alive;
                    control_loop(control_rx, master, writer);
                })
                .context("Gagal membuat thread kontrol PTY")?;
        }

        {
            let control_tx = control_tx.clone();
            thread::Builder::new()
                .name("pty-waiter".into())
                .spawn(move || {
                    let _alive = alive;
                    let code = child.wait().ok().map(|status| status.exit_code());
                    finish(control_tx, reader_thread, event_tx, notify, code);
                })
                .context("Gagal membuat thread penunggu proses")?;
        }

        Ok(Self {
            control: control_tx,
            events: event_rx,
            killer,
            #[cfg(test)]
            threads_alive,
        })
    }

    /// Mengambil kejadian berikutnya tanpa menunggu.
    pub fn try_next_event(&self) -> Option<PtyEvent> {
        self.events.try_recv().ok()
    }

    pub fn write(&self, bytes: Vec<u8>) {
        // Gagal hanya jika thread kontrol sudah berhenti, artinya proses sudah selesai.
        let _ = self.control.send(Control::Write(bytes));
    }

    pub fn resize(&self, size: PtySize) {
        let _ = self.control.send(Control::Resize(size));
    }

    #[cfg(test)]
    pub fn threads_alive(&self) -> std::sync::Weak<()> {
        self.threads_alive.clone()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Gagal jika proses sudah selesai, dan itu memang kondisi yang diinginkan.
        let _ = self.killer.kill();
    }
}

/// Status "abaikan Ctrl+C" diwariskan dari proses induk ke anaknya. Jika ctermin dibuka oleh
/// launcher yang punya status itu, Ctrl+C di shell akan mati tanpa pesan apa pun.
fn enable_ctrl_c_for_children() {
    // SAFETY: handler NULL hanya mengubah atribut proses, tanpa pointer yang didereferensi.
    // Hasilnya sengaja diabaikan: jika gagal, shell tetap berjalan seperti biasa.
    unsafe {
        SetConsoleCtrlHandler(None, 0);
    }
}

fn read_loop(mut reader: Box<dyn Read + Send>, events: SyncSender<PtyEvent>, notify: Notify) {
    let mut buffer = vec![0; READ_BUFFER_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let output = PtyEvent::Output(buffer[..count].to_vec());
                if events.send(output).is_err() {
                    break;
                }
                notify();
            }
        }
    }
}

fn control_loop(
    commands: Receiver<Control>,
    master: Box<dyn MasterPty + Send>,
    mut writer: Box<dyn Write + Send>,
) {
    for command in commands {
        match command {
            Control::Write(bytes) => {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
            Control::Resize(size) => {
                let _ = master.resize(size);
            }
            Control::Shutdown => break,
        }
    }
    // Menutup writer dan master menutup ConPTY, sehingga pembaca mendapat EOF.
}

/// Menutup ConPTY, menunggu sisa output terbaca, baru mengabarkan proses selesai.
fn finish(
    control: Sender<Control>,
    reader_thread: JoinHandle<()>,
    events: SyncSender<PtyEvent>,
    notify: Notify,
    code: Option<u32>,
) {
    let _ = control.send(Control::Shutdown);
    drop(control);
    let _ = reader_thread.join();
    if events.send(PtyEvent::Exited(code)).is_ok() {
        notify();
    }
}
