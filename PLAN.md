# ctermin - Project Plan

Terminal native untuk Windows, ditulis dengan Rust, dengan 3 keunggulan utama:
1. SSH Server Manager (klik folder/server  langsung tersambung)
2. Tab & Split Pane
3. Integrasi AI yang rapi

Target: ringan (RAM < 50MB untuk 1 tab, binary < 15MB), cepat, stabil.

---

## Fase 0 - Persiapan
- [x] `cargo new ctermin` dengan struktur workspace
- [x] Tambah crate: eframe, alacritty_terminal, portable-pty, serde, toml, keyring, tokio, reqwest
- [x] Setup rustfmt + clippy
- [x] Pastikan build release berjalan di Windows (target x86_64-pc-windows-msvc)

**Selesai jika:** `cargo run` membuka jendela kosong.

## Fase 1 - Terminal Dasar
- [x] Spawn PowerShell via portable-pty (ConPTY)
- [x] Baca output PTY di thread terpisah, kirim ke alacritty_terminal
- [x] Render grid karakter + warna + kursor di egui
- [x] Kirim input keyboard ke PTY (termasuk Ctrl, Alt, panah)
- [x] Resize terminal mengikuti ukuran jendela
- [x] Copy/paste dan scrollback

**Selesai jika:** `dir`, `git status`, `vim`, `htop` (via WSL) tampil benar.

## Fase 2 - Tab
- [x] Buat, tutup, dan pindah tab
- [x] Shortcut: Ctrl+T (baru), Ctrl+W (tutup), Ctrl+Tab (pindah)
- [x] Judul tab otomatis (nama shell/server)
- [x] Proses PTY dimatikan dengan benar saat tab ditutup

**Selesai jika:** 5+ tab berjalan bersamaan tanpa bocor memori/thread.

## Fase 3 - Split Pane
- [x] Layout disimpan sebagai tree (Split / Leaf)
- [x] Split kanan-kiri (Ctrl+Shift+D)
- [x] Split atas-bawah (Ctrl+Shift+E)
- [x] Split bertingkat
- [x] Pindah fokus antar panel (Alt+Panah)
- [x] Drag pembatas untuk ubah ukuran
- [x] Tutup panel tanpa merusak layout
- [x] Menu klik kanan tab: duplikat tab, split kanan/bawah, tutup split, tutup tab

**Selesai jika:** layout 3-4 panel stabil, resize lancar.

## Fase 4 - SSH Server Manager
- [x] Sidebar berisi folder dan server
- [x] Tambah/edit/hapus folder dan server
- [x] Data disimpan di `%APPDATA%\ctermin\servers.toml`
- [x] Field: nama, host, port, username, metode login (key/password)
- [x] Klik server  tab baru menjalankan `ssh.exe` ke server tersebut
- [x] Klik folder  buka semua server di folder dalam split (opsional)
- [x] Import dari `~/.ssh/config`
- [x] Password disimpan via keyring (Windows Credential Manager)
- [x] Status koneksi + tombol reconnect
- [x] Dropdown di sebelah tombol + berisi PowerShell dan daftar server per folder

**Selesai jika:** klik folder `cj` langsung masuk ke server cj.

## Fase 5 - Integrasi AI
- [ ] AI bar dipanggil dengan Ctrl+K
- [ ] Request AI berjalan async (tokio), UI tidak boleh freeze
- [ ] Bahasa biasa  saran perintah (dimasukkan ke terminal, TIDAK dijalankan)
- [ ] "Tanya AI" untuk menjelaskan error dari output terakhir
- [ ] Konteks: server aktif, OS, folder saat ini
- [ ] Peringatan untuk perintah berbahaya (rm -rf, DROP, mkfs, format, dll.)
- [ ] Provider: Claude / OpenAI / Ollama (bisa diatur)
- [ ] API key disimpan via keyring

**Selesai jika:** "cek disk penuh"  `df -h` muncul di terminal, menunggu Enter.

## Fase 6 - Pengaturan & Polishing
- [ ] Config `%APPDATA%\ctermin\config.toml` (tema, font, shell default, shortcut)
- [ ] Simpan & pulihkan sesi (tab/split terakhir)
- [ ] Pilihan shell: PowerShell, CMD, WSL, Git Bash
- [ ] Installer Windows (cargo-wix atau Inno Setup)
- [ ] Ikon & branding ctermin

---

## Di Luar Cakupan (untuk saat ini)
- macOS/Linux
- Transfer file SFTP
- Sinkronisasi cloud
- Sistem plugin

## Risiko
| Risiko | Mitigasi |
|---|---|
| Rendering terminal di egui kurang cepat | Batasi redraw hanya saat ada perubahan; evaluasi wgpu custom renderer jika perlu |
| Input keyboard tidak lengkap (Ctrl/Alt/fungsi) | Buat tabel mapping key  escape sequence dan uji dengan vim/htop |
| Bug ConPTY di Windows lama | Minimum Windows 10 1809+ |
| Kebocoran password/API key | Wajib pakai keyring, tidak pernah ditulis ke file/log |
| AI menjalankan perintah berbahaya | AI hanya menyarankan, eksekusi selalu oleh user |
