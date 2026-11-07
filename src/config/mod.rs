//! Location and storage of data files in `%APPDATA%\ctermin`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::ai::settings::AppConfig;
use crate::ssh::model::ServerStore;

const APP_FOLDER: &str = "ctermin";
pub const SERVERS_FILE: &str = "servers.toml";
pub const CONFIG_FILE: &str = "config.toml";
const DAMAGED_SUFFIX: &str = "damaged";
const TEMPORARY_SUFFIX: &str = "tmp";

/// The contents of a toml file and its path. Without a path, changes stay in memory.
#[derive(Default)]
pub struct TomlFile<T> {
    pub data: T,
    path: Option<PathBuf>,
}

pub type ServerRepository = TomlFile<ServerStore>;
pub type ConfigFile = TomlFile<AppConfig>;

impl<T: Default + Serialize + DeserializeOwned> TomlFile<T> {
    /// Opens a file in `%APPDATA%\ctermin`. The second value holds a warning for the user.
    pub fn open_default(file_name: &str) -> (Self, Option<String>) {
        let Some(app_data) = std::env::var_os("APPDATA") else {
            let warning = "The APPDATA folder was not found, so data will not be saved.";
            return (Self::default(), Some(warning.to_owned()));
        };
        let path = PathBuf::from(app_data).join(APP_FOLDER).join(file_name);
        Self::open(path)
    }

    fn open(path: PathBuf) -> (Self, Option<String>) {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let data = T::default();
                let path = Some(path);
                return (Self { data, path }, None);
            }
            Err(_) => {
                // The path is dropped on purpose, so an unreadable file is not overwritten with empty data.
                let warning = format!(
                    "Could not read {}, so changes will not be saved.",
                    path.display()
                );
                return (Self::default(), Some(warning));
            }
        };

        match toml::from_str(&text) {
            Ok(data) => {
                let path = Some(path);
                (Self { data, path }, None)
            }
            Err(_) => {
                let backup = path.with_extension(DAMAGED_SUFFIX);
                let copied = fs::copy(&path, &backup).is_ok();
                let warning = format!(
                    "{} is damaged and cannot be read. {}",
                    path.display(),
                    if copied {
                        format!("A copy is at {}.", backup.display())
                    } else {
                        "Could not create a copy.".to_owned()
                    }
                );
                let path = copied.then_some(path);
                let data = T::default();
                (Self { data, path }, Some(warning))
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        match &self.path {
            Some(path) => write_atomic(path, &self.data),
            None => Ok(()),
        }
    }
}

/// Writes through a temporary file, so a crash in the middle of writing cannot damage the
/// old data.
fn write_atomic<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).context("Could not create the data folder")?;
    }
    let text = toml::to_string_pretty(data).context("Could not serialize the data")?;
    let temporary = path.with_extension(TEMPORARY_SUFFIX);
    fs::write(&temporary, text).context("Could not write the data")?;
    fs::rename(&temporary, path).context("Could not save the data")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::ssh::model::{AuthMethod, Server, DEFAULT_PORT};

    /// A temporary folder removed when the test ends.
    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("ctermin-test-{}-{unique}", std::process::id());
            Self(std::env::temp_dir().join(name))
        }

        fn file(&self) -> PathBuf {
            self.0.join("data").join(SERVERS_FILE)
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample_store() -> ServerStore {
        let mut store = ServerStore::default();
        let folder = store.add_folder("cj".to_owned());
        let id = store.new_server_id();
        store.upsert_server(Server {
            id,
            name: "cj-main".to_owned(),
            host: "10.0.0.5".to_owned(),
            port: DEFAULT_PORT,
            username: "root".to_owned(),
            folder: Some(folder),
            auth: AuthMethod::Password,
        });
        let id = store.new_server_id();
        store.upsert_server(Server {
            id,
            name: "other".to_owned(),
            host: "10.0.0.6".to_owned(),
            port: 2222,
            username: String::new(),
            folder: None,
            auth: AuthMethod::Key {
                identity_file: Some(r"C:\kunci\id".to_owned()),
            },
        });
        store
    }

    #[test]
    fn data_saves_and_loads_back() {
        let folder = TempFolder::new();
        let (mut repository, warning) = ServerRepository::open(folder.file());
        assert_eq!(warning, None);
        repository.data = sample_store();
        repository.save().expect("saved");

        let (reopened, warning) = ServerRepository::open(folder.file());
        assert_eq!(warning, None);
        assert_eq!(reopened.data, sample_store());
    }

    #[test]
    fn toml_file_has_no_place_for_passwords() {
        let text = toml::to_string_pretty(&sample_store()).expect("serializable");
        assert!(!text.to_lowercase().contains("secret"));
        assert!(!text.contains("password ="));
        assert!(!text.contains("password="));
    }

    #[test]
    fn damaged_file_is_backed_up_not_lost() {
        let folder = TempFolder::new();
        let path = folder.file();
        fs::create_dir_all(path.parent().expect("has a parent")).expect("folder created");
        fs::write(&path, "this [is not toml").expect("file written");

        let (repository, warning) = ServerRepository::open(path.clone());

        assert!(warning.expect("has a warning").contains("damaged"));
        assert!(repository.data.servers.is_empty());
        let backup = fs::read_to_string(path.with_extension(DAMAGED_SUFFIX));
        assert_eq!(backup.expect("backup exists"), "this [is not toml");
    }

    #[test]
    fn saving_without_a_path_writes_nothing() {
        ServerRepository::default().save().expect("no error");
    }

    #[test]
    fn saving_leaves_no_temporary_file() {
        let folder = TempFolder::new();
        let (repository, _) = ServerRepository::open(folder.file());
        repository.save().expect("saved");
        assert!(!folder.file().with_extension(TEMPORARY_SUFFIX).exists());
    }
}
