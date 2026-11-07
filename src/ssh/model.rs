//! Folder and server data. Passwords are not here: they live in Credential Manager.

use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 22;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerId(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FolderId(pub u64);

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "lowercase")]
pub enum AuthMethod {
    /// An SSH key. Without `identity_file`, ssh uses its default keys or ssh-agent.
    Key {
        identity_file: Option<String>,
    },
    Password,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Server {
    pub id: ServerId,
    pub name: String,
    pub host: String,
    pub port: u16,
    /// Empty means following ssh config or the Windows user.
    pub username: String,
    pub folder: Option<FolderId>,
    pub auth: AuthMethod,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct ServerStore {
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub servers: Vec<Server>,
}

impl ServerStore {
    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn new_server_id(&mut self) -> ServerId {
        ServerId(self.take_id())
    }

    pub fn add_folder(&mut self, name: String) -> FolderId {
        let id = FolderId(self.take_id());
        self.folders.push(Folder { id, name });
        id
    }

    pub fn rename_folder(&mut self, id: FolderId, name: String) {
        if let Some(folder) = self.folders.iter_mut().find(|folder| folder.id == id) {
            folder.name = name;
        }
    }

    /// Its servers are not deleted; they just leave the folder.
    pub fn remove_folder(&mut self, id: FolderId) {
        self.folders.retain(|folder| folder.id != id);
        for server in &mut self.servers {
            if server.folder == Some(id) {
                server.folder = None;
            }
        }
    }

    /// Adds a new server, or replaces the server with the same id.
    pub fn upsert_server(&mut self, server: Server) {
        match self.servers.iter_mut().find(|item| item.id == server.id) {
            Some(existing) => *existing = server,
            None => self.servers.push(server),
        }
    }

    pub fn remove_server(&mut self, id: ServerId) {
        self.servers.retain(|server| server.id != id);
    }

    pub fn server(&self, id: ServerId) -> Option<&Server> {
        self.servers.iter().find(|server| server.id == id)
    }

    pub fn folder(&self, id: FolderId) -> Option<&Folder> {
        self.folders.iter().find(|folder| folder.id == id)
    }

    pub fn servers_in(&self, folder: Option<FolderId>) -> impl Iterator<Item = &Server> {
        self.servers
            .iter()
            .filter(move |server| server.folder == folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(store: &mut ServerStore, name: &str, folder: Option<FolderId>) -> ServerId {
        let id = store.new_server_id();
        store.upsert_server(Server {
            id,
            name: name.to_owned(),
            host: "example.local".to_owned(),
            port: DEFAULT_PORT,
            username: "user".to_owned(),
            folder,
            auth: AuthMethod::Password,
        });
        id
    }

    #[test]
    fn ids_are_not_reused_after_deletion() {
        let mut store = ServerStore::default();
        let first = server(&mut store, "a", None);
        store.remove_server(first);
        assert_ne!(server(&mut store, "b", None), first);
    }

    #[test]
    fn upsert_replaces_the_same_server() {
        let mut store = ServerStore::default();
        let id = server(&mut store, "old", None);
        let mut edited = store.server(id).cloned().expect("server exists");
        edited.name = "new".to_owned();
        store.upsert_server(edited);

        assert_eq!(store.servers.len(), 1);
        assert_eq!(store.server(id).map(|s| s.name.as_str()), Some("new"));
    }

    #[test]
    fn deleting_a_folder_keeps_its_servers() {
        let mut store = ServerStore::default();
        let folder = store.add_folder("cj".to_owned());
        let id = server(&mut store, "a", Some(folder));

        store.remove_folder(folder);

        assert!(store.folder(folder).is_none());
        assert_eq!(store.server(id).and_then(|s| s.folder), None);
    }

    #[test]
    fn servers_are_grouped_by_folder() {
        let mut store = ServerStore::default();
        let folder = store.add_folder("cj".to_owned());
        server(&mut store, "a", Some(folder));
        server(&mut store, "b", None);

        assert_eq!(store.servers_in(Some(folder)).count(), 1);
        assert_eq!(store.servers_in(None).count(), 1);
    }
}
