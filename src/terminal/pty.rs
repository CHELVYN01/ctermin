//! The shell process inside ConPTY and its supporting threads.
//!
//! Every blocking operation happens on a separate thread, so the UI never
//! waits on the PTY:
//! - `pty-reader` reads output and sends it to the UI.
//! - `pty-control` owns the master PTY: writes input, resizes, then closes ConPTY.
//! - `pty-waiter` waits for the process, because ConPTY sends no EOF when the shell
//!   exits until the pseudoconsole is closed.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

const READ_BUFFER_SIZE: usize = 64 * 1024;
/// How many output chunks may queue. When full, the reader waits for the UI (backpressure).
const OUTPUT_QUEUE_SIZE: usize = 32;

/// Called from a background thread after a new event for the UI.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// Events from the PTY to process on the UI thread.
pub enum PtyEvent {
    Output(Vec<u8>),
    /// The process exited. `None` if the exit code is unknown.
    Exited(Option<u32>),
}

enum Control {
    Write(Vec<u8>),
    Resize(PtySize),
    Shutdown,
}

/// When dropped, the process is killed and all three threads stop on their own.
pub struct Pty {
    control: Sender<Control>,
    events: Receiver<PtyEvent>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    #[cfg(test)]
    threads_alive: std::sync::Weak<()>,
}

impl Pty {
    pub fn spawn(mut command: CommandBuilder, size: PtySize, notify: Notify) -> Result<Self> {
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        enable_ctrl_c_for_children();

        let pair = native_pty_system()
            .openpty(size)
            .map_err(|err| anyhow!("Could not create ConPTY: {err}"))?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| anyhow!("Could not start the shell: {err}"))?;
        // The slave handle in this process must close so only the shell holds it.
        drop(pair.slave);

        let killer = child.clone_killer();
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| anyhow!("Could not read from the PTY: {err}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| anyhow!("Could not write to the PTY: {err}"))?;

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
                .context("Could not create the PTY reader thread")?
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
                .context("Could not create the PTY control thread")?;
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
                .context("Could not create the process waiter thread")?;
        }

        Ok(Self {
            control: control_tx,
            events: event_rx,
            killer,
            #[cfg(test)]
            threads_alive,
        })
    }

    /// Takes the next event without waiting.
    pub fn try_next_event(&self) -> Option<PtyEvent> {
        self.events.try_recv().ok()
    }

    pub fn write(&self, bytes: Vec<u8>) {
        // Fails only if the control thread has stopped, meaning the process has exited.
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
        // Fails if the process has already exited, which is the desired state anyway.
        let _ = self.killer.kill();
    }
}

/// The "ignore Ctrl+C" state is inherited from parent to child. If ctermin is started by a
/// launcher with that state, Ctrl+C in the shell silently stops working.
fn enable_ctrl_c_for_children() {
    // SAFETY: a NULL handler only changes a process attribute; no pointer is dereferenced.
    // The result is ignored on purpose: on failure the shell still runs normally.
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
    // Closing the writer and master closes ConPTY, so the reader gets EOF.
}

/// Closes ConPTY, waits for the remaining output, then reports that the process exited.
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
