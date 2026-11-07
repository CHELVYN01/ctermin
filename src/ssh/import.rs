//! Import host dari `~/.ssh/config`.
//!
//! Host disimpan memakai alias-nya, sehingga ssh.exe tetap membaca config itu sendiri dan opsi
//! seperti ProxyJump, HostName, atau IdentityFile tetap berlaku.

use super::model::{AuthMethod, FolderId, Server, ServerStore, DEFAULT_PORT};

const IMPORT_FOLDER: &str = "SSH Config";

#[derive(PartialEq, Eq, Debug)]
pub struct ConfigHost {
    pub alias: String,
    pub user: String,
    pub port: u16,
}

#[derive(PartialEq, Eq, Debug, Default)]
pub struct ImportSummary {
    pub added: usize,
    pub skipped: usize,
}

/// `Include` dan blok `Match` tidak diikuti; host di dalamnya perlu ditambahkan manual.
pub fn parse_config(text: &str) -> Vec<ConfigHost> {
    let mut hosts: Vec<ConfigHost> = Vec::new();
    let mut current_start = None;

    for line in text.lines() {
        let Some((keyword, value)) = split_line(line) else {
            continue;
        };
        match keyword.to_ascii_lowercase().as_str() {
            "host" => {
                current_start = Some(hosts.len());
                hosts.extend(
                    value
                        .split_whitespace()
                        .filter(|p| is_plain(p))
                        .map(|alias| ConfigHost {
                            alias: alias.trim_matches('"').to_owned(),
                            user: String::new(),
                            port: DEFAULT_PORT,
                        }),
                );
            }
            "match" => current_start = None,
            "user" => {
                for host in current_block(&mut hosts, current_start) {
                    host.user = value.trim_matches('"').to_owned();
                }
            }
            "port" => {
                if let Ok(port) = value.parse() {
                    for host in current_block(&mut hosts, current_start) {
                        host.port = port;
                    }
                }
            }
            _ => {}
        }
    }
    hosts
}

fn current_block(hosts: &mut [ConfigHost], start: Option<usize>) -> &mut [ConfigHost] {
    match start {
        Some(start) => &mut hosts[start..],
        None => &mut [],
    }
}

/// Memisahkan `Kata nilai` atau `Kata=nilai`, mengabaikan komentar dan baris kosong.
fn split_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let (keyword, rest) = line.split_at(end);
    let value = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '=');
    Some((keyword, value.trim_end()))
}

/// Pola wildcard dan negasi bukan host yang bisa disambungkan.
fn is_plain(pattern: &str) -> bool {
    !pattern.contains(['*', '?', '!'])
}

/// Menambah host baru ke folder "SSH Config". Host yang sudah ada dilewati, jadi import aman
/// dijalankan berulang.
pub fn merge_hosts(store: &mut ServerStore, hosts: Vec<ConfigHost>) -> ImportSummary {
    let mut summary = ImportSummary::default();
    let mut folder = existing_folder(store);

    for host in hosts {
        let exists = store.servers.iter().any(|server| {
            server.host == host.alias && server.port == host.port && server.username == host.user
        });
        if exists {
            summary.skipped += 1;
            continue;
        }
        let folder_id = *folder.get_or_insert_with(|| store.add_folder(IMPORT_FOLDER.to_owned()));
        let id = store.new_server_id();
        store.upsert_server(Server {
            id,
            name: host.alias.clone(),
            host: host.alias,
            port: host.port,
            username: host.user,
            folder: Some(folder_id),
            auth: AuthMethod::Key {
                identity_file: None,
            },
        });
        summary.added += 1;
    }
    summary
}

fn existing_folder(store: &ServerStore) -> Option<FolderId> {
    store
        .folders
        .iter()
        .find(|folder| folder.name == IMPORT_FOLDER)
        .map(|folder| folder.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "
# komentar
Host *
    ServerAliveInterval 30

Host cj cj-backup
    HostName 10.0.0.5
    User root
    Port 2222
    IdentityFile ~/.ssh/kunci

host=web
  user=deploy

Host !larangan pola-* satu
  User \"ada kutip\"

Match host khusus
  User tidakdipakai
";

    #[test]
    fn host_dan_opsi_terbaca() {
        let hosts = parse_config(CONFIG);
        assert_eq!(
            hosts,
            vec![
                ConfigHost {
                    alias: "cj".to_owned(),
                    user: "root".to_owned(),
                    port: 2222
                },
                ConfigHost {
                    alias: "cj-backup".to_owned(),
                    user: "root".to_owned(),
                    port: 2222
                },
                ConfigHost {
                    alias: "web".to_owned(),
                    user: "deploy".to_owned(),
                    port: 22
                },
                ConfigHost {
                    alias: "satu".to_owned(),
                    user: "ada kutip".to_owned(),
                    port: 22
                },
            ]
        );
    }

    #[test]
    fn config_kosong_tidak_panik() {
        assert!(parse_config("").is_empty());
        assert!(parse_config("User tanpa-host\nHost").is_empty());
        assert!(parse_config("Port bukan-angka\n# saja").is_empty());
    }

    #[test]
    fn import_masuk_ke_satu_folder_dan_memakai_alias() {
        let mut store = ServerStore::default();
        let summary = merge_hosts(&mut store, parse_config(CONFIG));

        assert_eq!(summary.added, 4);
        assert_eq!(store.folders.len(), 1);
        let server = &store.servers[0];
        assert_eq!(server.host, "cj");
        assert_eq!(server.port, 2222);
        assert_eq!(server.folder, Some(store.folders[0].id));
    }

    #[test]
    fn import_kedua_kali_tidak_menggandakan() {
        let mut store = ServerStore::default();
        merge_hosts(&mut store, parse_config(CONFIG));
        let summary = merge_hosts(&mut store, parse_config(CONFIG));

        assert_eq!(summary.added, 0);
        assert_eq!(summary.skipped, 4);
        assert_eq!(store.servers.len(), 4);
        assert_eq!(store.folders.len(), 1);
    }

    #[test]
    fn tanpa_host_baru_tidak_membuat_folder() {
        let mut store = ServerStore::default();
        merge_hosts(&mut store, Vec::new());
        assert!(store.folders.is_empty());
    }
}
