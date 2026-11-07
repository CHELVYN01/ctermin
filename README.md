# ctermin

A native Windows terminal written in Rust, with an SSH server manager, tabs and split panes, and an AI assistant that only ever suggests commands.

## Features

- **SSH server manager:** folders and servers in a sidebar. Click a server to connect, or a folder to open all its servers in splits. Import hosts from `~/.ssh/config`.
- **Tabs and split panes:** nested splits, draggable dividers, and session restore.
- **AI assistant:** turn plain language into a command, or explain the last output. Commands are inserted into the terminal and wait for you to press Enter. Dangerous commands need confirmation first.
- **Your choice of AI access:** use a signed-in Claude Code installation, or a Claude API key.
- **Shells:** PowerShell, Command Prompt, Git Bash, WSL, and PowerShell 7 when installed.
- **Secure by default:** passwords and API keys live in Windows Credential Manager, never in files or logs.

## Requirements

- Windows 10 version 1809 or later
- Rust stable with the MSVC toolchain, to build from source
- OpenSSH client (included with Windows), for SSH
- Optional: Claude Code or a Claude API key, for AI features

## Build and run

```powershell
cargo run --release
```

## Shortcuts

| Action | Default |
|---|---|
| New tab | `Ctrl+T` |
| Close pane | `Ctrl+W` |
| Next / previous tab | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Split right / down | `Ctrl+Shift+D` / `Ctrl+Shift+E` |
| Move focus between panes | `Alt+Arrow` |
| Toggle sidebar | `Ctrl+Shift+B` |
| AI bar | `Ctrl+K` |
| Settings | `Ctrl+,` |
| Copy | `Ctrl+C` with a selection, or `Ctrl+Shift+C` |
| Paste | `Ctrl+V` or `Ctrl+Shift+V` |

Change shortcuts in the `[shortcuts]` section of `%APPDATA%\ctermin\config.toml`, then restart ctermin.

## Data locations

| Data | Location |
|---|---|
| Settings | `%APPDATA%\ctermin\config.toml` |
| Servers | `%APPDATA%\ctermin\servers.toml` |
| Last session | `%APPDATA%\ctermin\session.toml` |
| Passwords and API key | Windows Credential Manager |

## Installer

With [Inno Setup](https://jrsoftware.org/isinfo.php) installed:

```powershell
cargo build --release
iscc installer\ctermin.iss
```

The installer is written to `target\installer`. The builds are not code signed yet, so Windows Smart App Control can block them.

## License

[MIT](LICENSE)
