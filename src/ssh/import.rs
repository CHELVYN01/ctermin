//! Imports hosts from `~/.ssh/config`.
//!
//! Hosts are stored by alias, so ssh.exe still reads the config itself and options
//! such as ProxyJump, HostName, or IdentityFile keep working.

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

/// `Include` and `Match` blocks are not followed; hosts in them must be added by hand.
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

/// Splits `Keyword value` or `Keyword=value`, skipping comments and blank lines.
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

/// Wildcard and negated patterns are not hosts you can connect to.
fn is_plain(pattern: &str) -> bool {
    !pattern.contains(['*', '?', '!'])
}

/// Adds new hosts to the "SSH Config" folder. Existing hosts are skipped, so importing is safe to
/// run repeatedly.
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
# a comment
Host *
    ServerAliveInterval 30

Host cj cj-backup
    HostName 10.0.0.5
    User root
    Port 2222
    IdentityFile ~/.ssh/my_key

host=web
  user=deploy

Host !blocked pattern-* one
  User \"has quotes\"

Match host khusus
  User tidakdipakai
";

    #[test]
    fn hosts_and_options_are_read() {
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
                    alias: "one".to_owned(),
                    user: "has quotes".to_owned(),
                    port: 22
                },
            ]
        );
    }

    #[test]
    fn empty_config_does_not_panic() {
        assert!(parse_config("").is_empty());
        assert!(parse_config("User no-host\nHost").is_empty());
        assert!(parse_config("Port not-a-number\n# only").is_empty());
    }

    #[test]
    fn import_uses_one_folder_and_aliases() {
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
    fn second_import_adds_no_duplicates() {
        let mut store = ServerStore::default();
        merge_hosts(&mut store, parse_config(CONFIG));
        let summary = merge_hosts(&mut store, parse_config(CONFIG));

        assert_eq!(summary.added, 0);
        assert_eq!(summary.skipped, 4);
        assert_eq!(store.servers.len(), 4);
        assert_eq!(store.folders.len(), 1);
    }

    #[test]
    fn no_new_hosts_creates_no_folder() {
        let mut store = ServerStore::default();
        merge_hosts(&mut store, Vec::new());
        assert!(store.folders.is_empty());
    }
}
