//! Lokasi dan penyimpanan file data di `%APPDATA%\ctermin`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::ssh::model::ServerStore;

const APP_FOLDER: &str = "ctermin";
const SERVERS_FILE: &str = "servers.toml";
const DAMAGED_SUFFIX: &str = "rusak";
const TEMPORARY_SUFFIX: &str = "tmp";

/// Data server beserta lokasi filenya. Tanpa lokasi, perubahan hanya tinggal di memori.
#[derive(Default)]
pub struct ServerRepository {
    pub store: ServerStore,
    path: Option<PathBuf>,
}

impl ServerRepository {
    /// Membuka `%APPDATA%\ctermin\servers.toml`. String kedua berisi peringatan untuk user.
    pub fn open_default() -> (Self, Option<String>) {
        let Some(app_data) = std::env::var_os("APPDATA") else {
            let warning = "Folder APPDATA tidak ditemukan, data server tidak akan disimpan.";
            return (Self::default(), Some(warning.to_owned()));
        };
        let path = PathBuf::from(app_data).join(APP_FOLDER).join(SERVERS_FILE);
        Self::open(path)
    }

    fn open(path: PathBuf) -> (Self, Option<String>) {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let store = ServerStore::default();
                let path = Some(path);
                return (Self { store, path }, None);
            }
            Err(_) => {
                // Lokasi sengaja dibuang agar file yang gagal dibaca tidak tertimpa data kosong.
                let warning = format!("{} gagal dibaca, perubahan tidak disimpan.", path.display());
                return (Self::default(), Some(warning));
            }
        };

        match toml::from_str(&text) {
            Ok(store) => {
                let path = Some(path);
                (Self { store, path }, None)
            }
            Err(_) => {
                let backup = path.with_extension(DAMAGED_SUFFIX);
                let copied = fs::copy(&path, &backup).is_ok();
                let warning = format!(
                    "{} rusak dan tidak bisa dibaca. {}",
                    path.display(),
                    if copied {
                        format!("Salinannya ada di {}.", backup.display())
                    } else {
                        "Salinannya gagal dibuat.".to_owned()
                    }
                );
                let path = copied.then_some(path);
                let store = ServerStore::default();
                (Self { store, path }, Some(warning))
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        match &self.path {
            Some(path) => write_atomic(path, &self.store),
            None => Ok(()),
        }
    }
}

/// Menulis lewat file sementara, supaya aplikasi yang mati di tengah penulisan tidak merusak
/// data lama.
fn write_atomic(path: &Path, store: &ServerStore) -> Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).context("Folder data gagal dibuat")?;
    }
    let text = toml::to_string_pretty(store).context("Data server gagal disusun")?;
    let temporary = path.with_extension(TEMPORARY_SUFFIX);
    fs::write(&temporary, text).context("Data server gagal ditulis")?;
    fs::rename(&temporary, path).context("Data server gagal disimpan")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::ssh::model::{AuthMethod, Server, DEFAULT_PORT};

    /// Folder sementara yang dibersihkan saat test selesai.
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
            name: "cj-utama".to_owned(),
            host: "10.0.0.5".to_owned(),
            port: DEFAULT_PORT,
            username: "root".to_owned(),
            folder: Some(folder),
            auth: AuthMethod::Password,
        });
        let id = store.new_server_id();
        store.upsert_server(Server {
            id,
            name: "lain".to_owned(),
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
    fn data_tersimpan_dan_terbaca_kembali() {
        let folder = TempFolder::new();
        let (mut repository, warning) = ServerRepository::open(folder.file());
        assert_eq!(warning, None);
        repository.store = sample_store();
        repository.save().expect("bisa disimpan");

        let (reopened, warning) = ServerRepository::open(folder.file());
        assert_eq!(warning, None);
        assert_eq!(reopened.store, sample_store());
    }

    #[test]
    fn file_toml_tidak_punya_ruang_untuk_password() {
        let text = toml::to_string_pretty(&sample_store()).expect("bisa disusun");
        assert!(!text.to_lowercase().contains("secret"));
        assert!(!text.contains("password ="));
        assert!(!text.contains("password="));
    }

    #[test]
    fn file_rusak_dicadangkan_dan_tidak_hilang() {
        let folder = TempFolder::new();
        let path = folder.file();
        fs::create_dir_all(path.parent().expect("ada induk")).expect("folder dibuat");
        fs::write(&path, "ini [bukan toml").expect("file ditulis");

        let (repository, warning) = ServerRepository::open(path.clone());

        assert!(warning.expect("ada peringatan").contains("rusak"));
        assert!(repository.store.servers.is_empty());
        let backup = fs::read_to_string(path.with_extension(DAMAGED_SUFFIX));
        assert_eq!(backup.expect("cadangan ada"), "ini [bukan toml");
    }

    #[test]
    fn tanpa_lokasi_simpan_tidak_menulis_apa_pun() {
        ServerRepository::default().save().expect("tidak error");
    }

    #[test]
    fn penyimpanan_tidak_meninggalkan_file_sementara() {
        let folder = TempFolder::new();
        let (repository, _) = ServerRepository::open(folder.file());
        repository.save().expect("bisa disimpan");
        assert!(!folder.file().with_extension(TEMPORARY_SUFFIX).exists());
    }
}
