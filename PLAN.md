# ctermin - Project Plan

A native Windows terminal written in Rust, with three main strengths:
1. SSH Server Manager (click a folder or server to connect right away)
2. Tabs and Split Panes
3. Clean AI integration

Goal: lightweight (RAM under 50MB for one tab, binary under 15MB), fast, stable.

---

## Phase 0 - Setup
- [x] `cargo new ctermin` with a workspace structure
- [x] Add crates: eframe, alacritty_terminal, portable-pty, serde, toml, keyring, tokio, reqwest
- [x] Set up rustfmt and clippy
- [x] Make sure the release build runs on Windows (target x86_64-pc-windows-msvc)

**Done when:** `cargo run` opens an empty window.

## Phase 1 - Basic Terminal
- [x] Spawn PowerShell through portable-pty (ConPTY)
- [x] Read PTY output on a separate thread and feed it to alacritty_terminal
- [x] Render the character grid, colors, and cursor in egui
- [x] Send keyboard input to the PTY (including Ctrl, Alt, arrows)
- [x] Resize the terminal with the window
- [x] Copy/paste and scrollback

**Done when:** `dir`, `git status`, `vim`, `htop` (through WSL) display correctly.

## Phase 2 - Tabs
- [x] Create, close, and switch tabs
- [x] Shortcuts: Ctrl+T (new), Ctrl+W (close), Ctrl+Tab (switch)
- [x] Automatic tab titles (shell or server name)
- [x] PTY process is terminated properly when a tab closes

**Done when:** 5+ tabs run at once with no memory or thread leaks.

## Phase 3 - Split Panes
- [x] Layout stored as a tree (Split / Leaf)
- [x] Left-right split (Ctrl+Shift+D)
- [x] Top-bottom split (Ctrl+Shift+E)
- [x] Nested splits
- [x] Move focus between panes (Alt+Arrow)
- [x] Drag a divider to resize
- [x] Close a pane without breaking the layout
- [x] Tab right-click menu: duplicate tab, split right/down, close split, close tab

**Done when:** a 3-4 pane layout is stable and resizing is smooth.

## Phase 4 - SSH Server Manager
- [x] Sidebar with folders and servers
- [x] Add, edit, and delete folders and servers
- [x] Data stored in `%APPDATA%\ctermin\servers.toml`
- [x] Fields: name, host, port, username, login method (key/password)
- [x] Click a server to open a new tab running `ssh.exe` to it
- [x] Click a folder to open all its servers in splits (optional)
- [x] Import from `~/.ssh/config`
- [x] Passwords stored through keyring (Windows Credential Manager)
- [x] Connection status and reconnect
- [x] Dropdown next to the + button with PowerShell and the server list by folder

**Done when:** clicking the `cj` folder connects straight to the cj servers.

## Phase 5 - AI Integration
- [x] AI bar opened with Ctrl+K
- [x] AI requests run async (tokio) and the UI never freezes
- [x] Plain language to a command suggestion (inserted into the terminal, NOT run)
- [x] "Ask AI" to explain an error from the latest output
- [x] Context: active server, OS, current folder
- [x] Warning for dangerous commands (rm -rf, DROP, mkfs, format, etc.)
- [x] AI provider choice: Claude Code (subscription quota) or a Claude API key, changeable in settings
- [ ] More providers: OpenAI and Ollama (later, once the two providers above are stable)
- [x] API key stored through keyring

**Done when:** "check disk usage" puts `df -h` in the terminal, waiting for Enter.

## Phase 6 - Settings and Polish
- [ ] Config in `%APPDATA%\ctermin\config.toml` (theme, font, default shell, shortcuts)
- [ ] Save and restore sessions (last tabs and splits)
- [ ] Shell choice: PowerShell, CMD, WSL, Git Bash
- [ ] Windows installer (cargo-wix or Inno Setup)
- [ ] ctermin icon and branding

---

## Out of Scope (for now)
- macOS/Linux
- SFTP file transfer
- Cloud sync
- Plugin system

## Risks
| Risk | Mitigation |
|---|---|
| Terminal rendering in egui is too slow | Redraw only on changes; evaluate a custom wgpu renderer if needed |
| Incomplete keyboard input (Ctrl/Alt/function keys) | Keep a key-to-escape-sequence table and test with vim and htop |
| ConPTY bugs on older Windows | Minimum Windows 10 1809+ |
| Password or API key leaks | Always use keyring; never write them to files or logs |
| AI runs a dangerous command | The AI only suggests; the user always runs commands |
